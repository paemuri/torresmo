use std::error::Error;
use std::path::{Path, PathBuf};
use std::time::Duration;

use reqwest::StatusCode;
use reqwest::blocking::{Client, Response};
use reqwest::header::HeaderValue;
use serde_json::{Value, json};

const URL: &str = "http://localhost:9091/transmission/rpc";
const SOCKET_NAME: &str = "transmission.sock";
const SESSION_HEADER: &str = "X-Transmission-Session-Id";
const TIMEOUT: Duration = Duration::from_secs(5);

pub struct Rpc {
  client: Client,
  session_id: Option<HeaderValue>,
}

impl Rpc {
  pub fn new() -> Result<Self, Box<dyn Error>> {
    let mut builder = Client::builder().timeout(TIMEOUT);
    // If there's any $XDG_RUNTIME_DIR/transmission.sock, use it. Otherwise,
    // connect through default localhost:9091.
    #[cfg(unix)]
    if let Some(socket) = socket_path() {
      builder = builder.unix_socket(socket);
    }
    let client = builder.build()?;
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

fn socket_path() -> Option<PathBuf> {
  let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR")?;
  socket_in(Path::new(&runtime_dir))
}

fn socket_in(runtime_dir: &Path) -> Option<PathBuf> {
  let socket = runtime_dir.join(SOCKET_NAME);
  socket.exists().then_some(socket)
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

  #[test]
  fn socket_is_used_only_when_it_exists() {
    let dir = std::env::temp_dir().join("torresmo-socket-test");
    std::fs::create_dir_all(&dir).unwrap();
    let socket = dir.join(SOCKET_NAME);
    let _ = std::fs::remove_file(&socket);
    assert_eq!(socket_in(&dir), None);
    std::fs::write(&socket, "").unwrap();
    assert_eq!(socket_in(&dir), Some(socket));
    std::fs::remove_dir_all(&dir).unwrap();
  }
}
