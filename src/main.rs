use std::error::Error;
use std::io;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Text};
use ratatui::widgets::{
  Block, Clear, Padding, Paragraph, Row, Table, TableState,
};
use reqwest::StatusCode;
use reqwest::blocking::{Client, Response};
use reqwest::header::HeaderValue;
use serde::Deserialize;
use serde_json::{Value, json};

const URL: &str = "http://localhost:9091/transmission/rpc";
const SESSION_HEADER: &str = "X-Transmission-Session-Id";
const REFRESH: Duration = Duration::from_secs(1);
const HEADERS: [&str; 8] =
  ["Name", "State", "Size", "%", "ETA", "Down", "Up", "Ratio"];

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
  let snapshot = fetch(&mut rpc)?;
  ratatui::run(|terminal| show(terminal, &mut rpc, snapshot))?;
  Ok(())
}

struct Snapshot {
  version: String,
  download_dir: String,
  speed_down: u64,
  speed_up: u64,
  total_down: u64,
  total_up: u64,
  torrents: Vec<Torrent>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Torrent {
  #[expect(dead_code, reason = "torrent actions will target torrents by id")]
  id: u64,
  name: String,
  status: u64,
  size_when_done: u64,
  percent_done: f64,
  eta: i64,
  rate_download: u64,
  rate_upload: u64,
  upload_ratio: f64,
}

fn fetch(rpc: &mut Rpc) -> Result<Snapshot, Box<dyn Error>> {
  let session = rpc.call(
    "session-get",
    json!({ "fields": ["version", "download-dir"] }),
  )?;
  // The version looks like "1.2.3 (hash)"; the build hash is noise.
  let version = session["version"]
    .as_str()
    .and_then(|version| version.split_whitespace().next())
    .unwrap_or("unknown")
    .to_owned();
  let download_dir = session["download-dir"].as_str().unwrap_or("?").to_owned();

  let stats = rpc.call("session-stats", json!({}))?;
  let all_time = &stats["cumulative-stats"];

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
  let mut reply = rpc.call("torrent-get", json!({ "fields": fields }))?;
  let torrents = serde_json::from_value(reply["torrents"].take())?;

  Ok(Snapshot {
    version,
    download_dir,
    speed_down: stats["downloadSpeed"].as_u64().unwrap_or(0),
    speed_up: stats["uploadSpeed"].as_u64().unwrap_or(0),
    total_down: all_time["downloadedBytes"].as_u64().unwrap_or(0),
    total_up: all_time["uploadedBytes"].as_u64().unwrap_or(0),
    torrents,
  })
}

fn show(
  terminal: &mut DefaultTerminal,
  rpc: &mut Rpc,
  mut snapshot: Snapshot,
) -> io::Result<()> {
  let header_row = table_row(HEADERS).bold();
  let mut selection = TableState::default().with_selected(Some(0));
  let mut show_help = false;
  let mut error: Option<String> = None;
  let mut next_refresh = Instant::now() + REFRESH;

  loop {
    terminal.draw(|frame| {
      let cells: Vec<[String; 8]> =
        snapshot.torrents.iter().map(torrent_cells).collect();
      let widths = column_widths(&cells);
      let rows = cells
        .iter()
        .map(|row| table_row(row.each_ref().map(String::as_str)));
      let table = Table::new(rows, widths)
        .header(header_row.clone())
        .row_highlight_style(Style::new().bg(Color::White).fg(Color::Black));
      let footer = match &error {
        Some(error) => Line::from(format!("Error: {error}")).red(),
        None => Line::from(footer_text(&snapshot)),
      };

      let [table_area, footer_area] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(1)])
          .areas(frame.area());
      frame.render_stateful_widget(table, table_area, &mut selection);
      frame.render_widget(footer, footer_area);

      if show_help {
        let help = Text::from(help_text(&snapshot));
        let popup = frame.area().centered(
          Constraint::Length(help.width() as u16 + 4),
          Constraint::Length(help.height() as u16 + 2),
        );
        let help = Paragraph::new(help).block(
          Block::bordered()
            .title(" Help ")
            .padding(Padding::horizontal(1)),
        );
        frame.render_widget(Clear, popup);
        frame.render_widget(help, popup);
      }
    })?;

    let timeout = next_refresh.saturating_duration_since(Instant::now());
    if event::poll(timeout)?
      && let Event::Key(key) = event::read()?
      && key.kind == KeyEventKind::Press
    {
      match key.code {
        KeyCode::Char('h') => show_help = !show_help,
        KeyCode::F(1) => show_help = true,
        KeyCode::Esc => show_help = false,
        KeyCode::Char('q') => return Ok(()),
        _ if show_help => {}
        KeyCode::Up => selection.select_previous(),
        KeyCode::Down => selection.select_next(),
        _ => {}
      }
    }

    if Instant::now() >= next_refresh {
      match fetch(rpc) {
        Ok(new) => {
          snapshot = new;
          error = None;
        }
        Err(e) => error = Some(e.to_string()),
      }
      next_refresh = Instant::now() + REFRESH;
    }
  }
}

fn footer_text(snapshot: &Snapshot) -> String {
  format!(
    "press h for help | ↓ {}/s ↑ {}/s | ↓ {} ↑ {} total",
    format_bytes(snapshot.speed_down),
    format_bytes(snapshot.speed_up),
    format_bytes(snapshot.total_down),
    format_bytes(snapshot.total_up),
  )
}

fn help_text(snapshot: &Snapshot) -> String {
  format!(
    "↑ ↓  Move the selection\n\
     h    Show help/info\n\
     q    Quit\n\n\
     Download folder  {}\n\
     Transmission     {}\n\
     Torresmo         {}",
    snapshot.download_dir,
    snapshot.version,
    env!("CARGO_PKG_VERSION"),
  )
}

fn torrent_cells(torrent: &Torrent) -> [String; 8] {
  [
    torrent.name.clone(),
    status_label(torrent.status).to_owned(),
    format_bytes(torrent.size_when_done),
    format!("{:.0}", (torrent.percent_done * 100.0).floor()),
    format_eta(torrent.eta),
    format_speed(torrent.rate_download),
    format_speed(torrent.rate_upload),
    format_ratio(torrent.upload_ratio),
  ]
}

fn column_widths(cells: &[[String; 8]]) -> [Constraint; 8] {
  let mut widths = HEADERS.map(|header| Line::from(header).width());
  for row in cells {
    for (width, cell) in widths.iter_mut().zip(row) {
      *width = (*width).max(Line::from(cell.as_str()).width());
    }
  }
  let mut constraints = widths.map(|width| Constraint::Length(width as u16));
  constraints[0] = Constraint::Fill(1);
  constraints
}

fn table_row<'a, T: Into<Line<'a>>>(cells: [T; 8]) -> Row<'a> {
  let [name, status, size, progress, eta, down, up, ratio] =
    cells.map(Into::<Line>::into);
  // Every column except Name and State holds a number, so it's right-aligned.
  Row::new([
    name,
    status,
    size.right_aligned(),
    progress.right_aligned(),
    eta.right_aligned(),
    down.right_aligned(),
    up.right_aligned(),
    ratio.right_aligned(),
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

fn status_label(status: u64) -> &'static str {
  match status {
    0 => "Stopped",
    1 => "Queued", // Verify queued.
    2 => "Verifying",
    3 => "Queued", // Download queued.
    4 => "Downloading",
    5 => "Queued", // Seed queued.
    6 => "Seeding",
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
