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
  let title = format!("Transmission is up (version {version})");

  let fields = [
    "id",
    "name",
    "status",
    "sizeWhenDone",
    "percentDone",
    "eta",
    "rateDownload",
    "rateUpload",
    "uploadRatio",
  ];
  let reply = rpc.call("torrent-get", json!({ "fields": fields }))?;
  let torrents = reply["torrents"]
    .as_array()
    .ok_or("torrent-get reply without a torrent list")?;

  let mut rows = Vec::new();
  for torrent in torrents {
    let id = torrent["id"].as_u64().unwrap_or(0);
    let name = torrent["name"].as_str().unwrap_or("?");
    let done = torrent["percentDone"].as_f64().unwrap_or(0.0) * 100.0;
    let status = status_label(torrent["status"].as_u64());
    let size = torrent["sizeWhenDone"].as_u64().unwrap_or(0);
    let down = torrent["rateDownload"].as_u64().unwrap_or(0);
    let up = torrent["rateUpload"].as_u64().unwrap_or(0);
    let eta = torrent["eta"].as_i64().unwrap_or(-1);
    let ratio = torrent["uploadRatio"].as_f64().unwrap_or(-1.0);
    rows.push([
      id.to_string(),
      name.to_owned(),
      status.to_owned(),
      format_bytes(size),
      format!("{done:.1}%"),
      format_eta(eta),
      format_speed(down),
      format_speed(up),
      format_ratio(ratio),
    ]);
  }

  ratatui::run(|terminal| show(terminal, &title, &rows))?;

  Ok(())
}

fn show(
  terminal: &mut DefaultTerminal,
  title: &str,
  rows: &[[String; 9]],
) -> io::Result<()> {
  let widths = [
    Constraint::Length(4),
    Constraint::Fill(1),
    Constraint::Length(15),
    Constraint::Length(8),
    Constraint::Length(8),
    Constraint::Length(7),
    Constraint::Length(10),
    Constraint::Length(10),
    Constraint::Length(6),
  ];
  let header_row = table_row([
    "#", "Name", "State", "Size", "Progress", "ETA", "Down", "Up", "Ratio",
  ]);
  let table = Table::new(
    rows
      .iter()
      .map(|row| table_row(row.each_ref().map(String::as_str))),
    widths,
  )
  .header(header_row.bold())
  .row_highlight_style(Style::new().bg(Color::White).fg(Color::Black));
  let mut selection = TableState::default().with_selected(Some(0));

  loop {
    terminal.draw(|frame| {
      let [title_area, table_area] =
        Layout::vertical([Constraint::Length(1), Constraint::Fill(1)])
          .areas(frame.area());
      frame.render_widget(title, title_area);
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

fn table_row(cells: [&str; 9]) -> Row<'_> {
  let [id, name, status, size, progress, eta, down, up, ratio] = cells;
  // Every column except State and Name holds a number, so it's right-aligned.
  Row::new([
    Line::from(id).right_aligned(),
    Line::from(name),
    Line::from(status),
    Line::from(size).right_aligned(),
    Line::from(progress).right_aligned(),
    Line::from(eta).right_aligned(),
    Line::from(down).right_aligned(),
    Line::from(up).right_aligned(),
    Line::from(ratio).right_aligned(),
  ])
}

fn format_bytes(bytes: u64) -> String {
  // Decimal units (1 kB = 1000 B), the same as Transmission's own clients.
  const UNITS: [&str; 4] = ["kB", "MB", "GB", "TB"];
  if bytes < 1000 {
    return format!("{bytes} B");
  }
  let mut value = bytes as f64 / 1000.0;
  let mut unit = 0;
  while value >= 1000.0 && unit < UNITS.len() - 1 {
    value /= 1000.0;
    unit += 1;
  }
  format!("{value:.1} {}", UNITS[unit])
}

fn format_speed(bytes_per_second: u64) -> String {
  if bytes_per_second == 0 {
    return String::new();
  }
  format!("{}/s", format_bytes(bytes_per_second))
}

fn format_eta(seconds: i64) -> String {
  // Transmission sends -1 (not available) and -2 (unknown) for the ETA.
  if seconds < 0 {
    return "-".to_owned();
  }
  let (days, hours) = (seconds / 86_400, seconds / 3_600 % 24);
  let (minutes, secs) = (seconds / 60 % 60, seconds % 60);
  match (days, hours, minutes) {
    (0, 0, 0) => format!("{secs}s"),
    (0, 0, _) => format!("{minutes}m {secs}s"),
    (0, _, _) => format!("{hours}h {minutes}m"),
    _ => format!("{days}d {hours}h"),
  }
}

fn format_ratio(ratio: f64) -> String {
  // Transmission sends -1 (not available) and -2 (infinite) for the ratio.
  match ratio {
    -1.0 => "-".to_owned(),
    -2.0 => "∞".to_owned(),
    _ => format!("{ratio:.2}"),
  }
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
