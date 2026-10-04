use std::error::Error;
use std::time::Duration;

use reqwest::StatusCode;
use reqwest::blocking::{Client, Response};
use reqwest::header::HeaderValue;
use serde_json::{Value, json};

const URL: &str = "http://localhost:9091/transmission/rpc";
const SESSION_HEADER: &str = "X-Transmission-Session-Id";

pub struct Rpc {
  client: Client,
  session_id: Option<HeaderValue>,
}

impl Rpc {
  pub fn new() -> Result<Self, Box<dyn Error>> {
    let client = Client::builder().timeout(Duration::from_secs(5)).build()?;
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

    // Transmission's CSRF protection: a call without a valid session id is
    // rejected with 409, which carries the id to retry with.
    if response.status() == StatusCode::CONFLICT {
      let session_id = response
        .headers()
        .get(SESSION_HEADER)
        .ok_or("409 response without a session id")?
        .clone();
      self.session_id = Some(session_id);
      response = self.send(&body)?;
    }

    let mut reply: Value = response.error_for_status()?.json()?;

    match reply["result"].as_str() {
      Some("success") => Ok(reply["arguments"].take()),
      Some(other) => Err(format!("RPC error: {other}").into()),
      None => Err("unexpected response shape".into()),
    }
  }

  fn send(&self, body: &Value) -> reqwest::Result<Response> {
    let mut request = self.client.post(URL).json(body);
    if let Some(session_id) = &self.session_id {
      request = request.header(SESSION_HEADER, session_id);
    }
    request.send()
  }
}
