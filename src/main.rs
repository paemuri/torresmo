use std::error::Error;
use std::io;
use std::process::ExitCode;
use std::time::Duration;

use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::Line;
use ratatui::widgets::{Row, Table, TableState};
use reqwest::StatusCode;
use reqwest::blocking::{Client, Response};
use reqwest::header::HeaderValue;
use serde_json::{Value, json};

const URL: &str = "http://localhost:9091/transmission/rpc";
const SESSION_HEADER: &str = "X-Transmission-Session-Id";

fn main() -> ExitCode {
  match run() {
    Ok(()) => ExitCode::SUCCESS,
    Err(e) => {
      eprintln!("Error: {e}");
      ExitCode::FAILURE
    }
  }
}

fn run() -> Result<(), Box<dyn Error>> {
  let mut rpc = Rpc::new()?;

  let session = rpc.call("session-get", json!({ "fields": ["version"] }))?;
  let version = session["version"].as_str().unwrap_or("unknown");
  let header = format!("Transmission is up (version {version})");

  let reply = rpc.call(
    "torrent-get",
    json!({ "fields": ["id", "name", "percentDone", "status"] }),
  )?;
  let torrents = reply["torrents"]
    .as_array()
    .ok_or("torrent-get reply without a torrent list")?;

  let mut rows = Vec::new();
  for torrent in torrents {
    let id = torrent["id"].as_u64().unwrap_or(0);
    let name = torrent["name"].as_str().unwrap_or("?");
    let done = torrent["percentDone"].as_f64().unwrap_or(0.0) * 100.0;
    let status = status_label(torrent["status"].as_u64());
    rows.push([
      id.to_string(),
      status.to_owned(),
      format!("{done:.1}%"),
      name.to_owned(),
    ]);
  }

  ratatui::run(|terminal| show(terminal, &header, &rows))?;

  Ok(())
}

fn show(
  terminal: &mut DefaultTerminal,
  header: &str,
  rows: &[[String; 4]],
) -> io::Result<()> {
  let widths = [
    Constraint::Length(4),
    Constraint::Length(15),
    Constraint::Length(8),
    Constraint::Fill(1),
  ];
  let header_row = Row::new([
    Line::from("#").right_aligned(),
    Line::from("State"),
    Line::from("Progress").right_aligned(),
    Line::from("Name"),
  ]);
  let table = Table::new(rows.iter().map(table_row), widths)
    .header(header_row.bold())
    .row_highlight_style(Style::new().bg(Color::White).fg(Color::Black));
  let mut selection = TableState::default().with_selected(Some(0));

  loop {
    terminal.draw(|frame| {
      let [header_area, table_area] =
        Layout::vertical([Constraint::Length(1), Constraint::Fill(1)])
          .areas(frame.area());
      frame.render_widget(header, header_area);
      frame.render_stateful_widget(&table, table_area, &mut selection);
    })?;

    if let Event::Key(key) = event::read()?
      && key.kind == KeyEventKind::Press
    {
      match key.code {
        KeyCode::Char('q') => return Ok(()),
        KeyCode::Up => selection.select_previous(),
        KeyCode::Down => selection.select_next(),
        _ => {}
      }
    }
  }
}

fn table_row([id, status, progress, name]: &[String; 4]) -> Row<'_> {
  Row::new([
    Line::from(id.as_str()).right_aligned(),
    Line::from(status.as_str()),
    Line::from(progress.as_str()).right_aligned(),
    Line::from(name.as_str()),
  ])
}

fn status_label(status: Option<u64>) -> &'static str {
  match status {
    Some(0) => "Stopped",
    Some(1) => "Verify queued",
    Some(2) => "Verifying",
    Some(3) => "Download queued",
    Some(4) => "Downloading",
    Some(5) => "Seed queued",
    Some(6) => "Seeding",
    _ => "Unknown",
  }
}

struct Rpc {
  client: Client,
  session_id: Option<HeaderValue>,
}

impl Rpc {
  fn new() -> Result<Self, Box<dyn Error>> {
    let client = Client::builder().timeout(Duration::from_secs(5)).build()?;
    Ok(Self {
      client,
      session_id: None,
    })
  }

  fn call(
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
