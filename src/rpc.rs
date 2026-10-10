use std::error::Error;
use std::time::Duration;

use reqwest::StatusCode;
use reqwest::blocking::{Client, Response};
use reqwest::header::HeaderValue;
use serde_json::{Value, json};

const URL: &str = "http://localhost:9091/transmission/rpc";
const SESSION_HEADER: &str = "X-Transmission-Session-Id";
const TIMEOUT: Duration = Duration::from_secs(5);

pub struct Rpc {
  client: Client,
  session_id: Option<HeaderValue>,
}

impl Rpc {
  pub fn new() -> Result<Self, Box<dyn Error>> {
    let client = Client::builder().timeout(TIMEOUT).build()?;
    Ok(Self {
      client,
      session_id: None,
    })
  }

  pub fn call(
    &mut self,
    method: &str,
    arguments: Value,
  ) -> Result<Value, Box<dyn Error>> {
    let body = json!({ "method": method, "arguments": arguments });
    let mut response = self.send(&body)?;

    // Transmission's CSRF protection: a call without a valid session ID is
    // rejected with 409, which carries the ID to retry with.
    if response.status() == StatusCode::CONFLICT {
      let session_id = response
        .headers()
        .get(SESSION_HEADER)
        .ok_or("409 response without a session id")?
        .clone();
      self.session_id = Some(session_id);
      response = self.send(&body)?;
    }

    let status = response.status();
    if !status.is_success() {
      return Err(format!("Transmission answered {status}").into());
    }
    let mut reply: Value = response.json().map_err(|error| {
      format!("Invalid reply from Transmission: {}", root_cause(&error))
    })?;

    match reply["result"].as_str() {
      Some("success") => Ok(reply["arguments"].take()),
      Some(other) => Err(format!("RPC error: {other}").into()),
      None => Err("unexpected response shape".into()),
    }
  }

  fn send(&self, body: &Value) -> Result<Response, String> {
    let mut request = self.client.post(URL).json(body);
    if let Some(session_id) = &self.session_id {
      request = request.header(SESSION_HEADER, session_id);
    }
    request.send().map_err(|error| {
      if error.is_timeout() {
        format!("Transmission didn't answer within {}s", TIMEOUT.as_secs())
      } else if error.is_connect() {
        format!("Can't connect to Transmission: {}", root_cause(&error))
      } else {
        root_cause(&error)
      }
    })
  }
}

fn root_cause(error: &dyn Error) -> String {
  let mut cause = error;
  while let Some(source) = cause.source() {
    cause = source;
  }
  cause.to_string()
}

#[cfg(test)]
mod tests {
  use std::{fmt, io};

  use super::*;

  #[derive(Debug)]
  struct Wrapper(io::Error);

  impl fmt::Display for Wrapper {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
      write!(f, "error sending request")
    }
  }

  impl Error for Wrapper {
    fn source(&self) -> Option<&(dyn 'static + Error)> {
      Some(&self.0)
    }
  }

  #[test]
  fn root_cause_is_the_deepest_error() {
    let error = Wrapper(io::Error::other("Connection refused"));
    assert_eq!(root_cause(&error), "Connection refused");
  }

  #[test]
  fn root_cause_of_a_plain_error_is_itself() {
    let error = io::Error::other("timed out");
    assert_eq!(root_cause(&error), "timed out");
  }
}
