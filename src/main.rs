use std::error::Error;
use std::process::ExitCode;
use std::time::Duration;

use reqwest::StatusCode;
use reqwest::blocking::Client;
use serde_json::{Value, json};

const URL: &str = "http://localhost:9091/transmission/rpc";
const SESSION_HEADER: &str = "X-Transmission-Session-Id";

fn main() -> ExitCode {
  match ping() {
    Ok(version) => {
      println!("Transmission is up (version {version})");
      ExitCode::SUCCESS
    }
    Err(e) => {
      eprintln!("Transmission is not responding: {e}");
      ExitCode::FAILURE
    }
  }
}

fn ping() -> Result<String, Box<dyn Error>> {
  let client = Client::builder().timeout(Duration::from_secs(5)).build()?;
  let body = json!({
    "method": "session-get",
    "arguments": { "fields": ["version"] }
  });

  let mut response = client.post(URL).json(&body).send()?;

  // Transmission's CSRF protection: the first call is rejected with 409 and
  // carries the session id to retry with.
  if response.status() == StatusCode::CONFLICT {
    let session_id = response
      .headers()
      .get(SESSION_HEADER)
      .ok_or("409 response without a session id")?
      .clone();
    response = client
      .post(URL)
      .header(SESSION_HEADER, session_id)
      .json(&body)
      .send()?;
  }

  let reply: Value = response.error_for_status()?.json()?;

  match reply["result"].as_str() {
    Some("success") => Ok(
      reply["arguments"]["version"]
        .as_str()
        .unwrap_or("unknown")
        .to_owned(),
    ),
    Some(other) => Err(format!("RPC error: {other}").into()),
    None => Err("unexpected response shape".into()),
  }
}
