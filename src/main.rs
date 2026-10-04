use std::error::Error;
use std::io;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use jiff::Timestamp;
use jiff::tz::TimeZone;
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
  Block, Clear, Padding, Paragraph, Row, Table, TableState, Wrap,
};
use reqwest::StatusCode;
use reqwest::blocking::{Client, Response};
use reqwest::header::HeaderValue;
use serde::Deserialize;
use serde_json::{Value, json};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

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
  id: u64,
  name: String,
  status: u64,
  size_when_done: u64,
  percent_done: f64,
  eta: i64,
  rate_download: u64,
  rate_upload: u64,
  upload_ratio: f64,
  error: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TorrentDetails {
  name: String,
  hash_string: String,
  download_dir: String,
  labels: Option<Vec<String>>,
  is_private: bool,
  creator: String,
  comment: String,
  status: u64,
  error: u64,
  error_string: String,
  percent_done: f64,
  eta: i64,
  rate_download: u64,
  rate_upload: u64,
  peers_connected: u64,
  peers_sending_to_us: u64,
  peers_getting_from_us: u64,
  queue_position: i64,
  bandwidth_priority: i64,
  total_size: u64,
  size_when_done: u64,
  left_until_done: u64,
  downloaded_ever: u64,
  uploaded_ever: u64,
  corrupt_ever: u64,
  upload_ratio: f64,
  piece_count: u64,
  piece_size: u64,
  file_count: Option<u64>,
  added_date: i64,
  done_date: i64,
  activity_date: i64,
  seconds_downloading: i64,
  seconds_seeding: i64,
  tracker_stats: Vec<TrackerStats>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TrackerStats {
  host: String,
  seeder_count: i64,
  leecher_count: i64,
  has_announced: bool,
  last_announce_succeeded: bool,
  last_announce_result: String,
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
  List,
  Help,
  Details(u64),
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
    "error",
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

fn fetch_details(
  rpc: &mut Rpc,
  id: u64,
) -> Result<Option<TorrentDetails>, Box<dyn Error>> {
  let fields = [
    "name",
    "hashString",
    "downloadDir",
    "labels",
    "isPrivate",
    "creator",
    "comment",
    "status",
    "error",
    "errorString",
    "percentDone",
    "eta",
    "rateDownload",
    "rateUpload",
    "peersConnected",
    "peersSendingToUs",
    "peersGettingFromUs",
    "queuePosition",
    "bandwidthPriority",
    "totalSize",
    "sizeWhenDone",
    "leftUntilDone",
    "downloadedEver",
    "uploadedEver",
    "corruptEver",
    "uploadRatio",
    "pieceCount",
    "pieceSize",
    "fileCount",
    "addedDate",
    "doneDate",
    "activityDate",
    "secondsDownloading",
    "secondsSeeding",
    "trackerStats",
  ];
  let mut reply = rpc.call(
    "torrent-get",
    json!({ "ids": [id], "fields": fields.as_slice() }),
  )?;
  let torrents: Vec<TorrentDetails> =
    serde_json::from_value(reply["torrents"].take())?;
  Ok(torrents.into_iter().next())
}

fn show(
  terminal: &mut DefaultTerminal,
  rpc: &mut Rpc,
  mut snapshot: Snapshot,
) -> io::Result<()> {
  let header_row = table_row(HEADERS).bold();
  let mut selection = TableState::default().with_selected(Some(0));
  let mut mode = Mode::List;
  let mut details: Option<TorrentDetails> = None;
  let mut error: Option<String> = None;
  let mut next_refresh = Instant::now() + REFRESH;

  loop {
    terminal.draw(|frame| {
      let [table_area, footer_area] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(1)])
          .areas(frame.area());

      let mut cells: Vec<[String; 8]> =
        snapshot.torrents.iter().map(torrent_cells).collect();
      let widths = column_widths(&cells);
      let [name_area, ..] =
        Layout::horizontal(widths).spacing(1).areas::<8>(table_area);
      for row in &mut cells {
        row[0] = truncate(&row[0], name_area.width.into());
      }

      let rows = cells.iter().zip(&snapshot.torrents).map(|(row, torrent)| {
        table_row(row.each_ref().map(String::as_str))
          .style(error_style(torrent.error))
      });
      let table = Table::new(rows, widths)
        .header(header_row.clone())
        .row_highlight_style(Style::new().bg(Color::White).fg(Color::Black));
      let footer = match &error {
        Some(error) => Line::from(format!("Error: {error}")).red(),
        None => Line::from(footer_text(&snapshot)),
      };

      frame.render_stateful_widget(table, table_area, &mut selection);
      frame.render_widget(footer, footer_area);

      match (mode, &details) {
        (Mode::Help, _) => {
          let help = help_text(&snapshot);
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
        (Mode::Details(_), Some(details)) => {
          let text = details_text(details);
          let popup = frame.area().centered(
            Constraint::Percentage(80),
            Constraint::Max(text.height() as u16 + 2),
          );
          let text = Paragraph::new(text).wrap(Wrap { trim: false }).block(
            Block::bordered()
              .title(" Details ")
              .padding(Padding::horizontal(1)),
          );
          frame.render_widget(Clear, popup);
          frame.render_widget(text, popup);
        }
        _ => {}
      }
    })?;

    let timeout = next_refresh.saturating_duration_since(Instant::now());
    if event::poll(timeout)?
      && let Event::Key(key) = event::read()?
      && key.kind == KeyEventKind::Press
    {
      match key.code {
        KeyCode::Char('q') => return Ok(()),
        KeyCode::Char('h') | KeyCode::F(1) if mode == Mode::Help => {
          mode = Mode::List
        }
        KeyCode::Char('h') | KeyCode::F(1) => mode = Mode::Help,
        KeyCode::Esc => mode = Mode::List,
        _ if mode != Mode::List => {}
        KeyCode::Up => selection.select_previous(),
        KeyCode::Down => selection.select_next(),
        KeyCode::Enter => {
          if let Some(torrent) =
            selection.selected().and_then(|i| snapshot.torrents.get(i))
          {
            match fetch_details(rpc, torrent.id) {
              Ok(Some(new)) => {
                details = Some(new);
                mode = Mode::Details(torrent.id);
              }
              Ok(None) => {}
              Err(e) => error = Some(e.to_string()),
            }
          }
        }
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
      if let Mode::Details(id) = mode {
        match fetch_details(rpc, id) {
          Ok(Some(new)) => details = Some(new),
          Ok(None) => mode = Mode::List,
          Err(e) => error = Some(e.to_string()),
        }
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

fn help_text(snapshot: &Snapshot) -> Text<'static> {
  Text::from(vec![
    field("↑ ↓", "Move the selection".into(), 7),
    field("Enter", "Show the torrent details".into(), 7),
    field("Esc", "Close the popup".into(), 7),
    field("h", "Show help/info".into(), 7),
    field("q", "Quit".into(), 7),
    Line::default(),
    field("Location", snapshot.download_dir.clone(), 13),
    field("Transmission", snapshot.version.clone(), 13),
    field("Torresmo", env!("CARGO_PKG_VERSION").into(), 13),
  ])
}

fn details_text(details: &TorrentDetails) -> Text<'static> {
  let now = Timestamp::now().as_second();
  let time_zone = TimeZone::system();
  let or_dash = |text: &str| match text {
    "" => "-".to_owned(),
    text => text.to_owned(),
  };
  let date = |timestamp: i64| match Timestamp::from_second(timestamp) {
    Ok(date) if timestamp > 0 => format!(
      "{}, {} ago",
      date
        .to_zoned(time_zone.clone())
        .strftime("%Y-%m-%d %H:%M:%S"),
      format_eta(now - timestamp),
    ),
    _ => "-".to_owned(),
  };
  let count = |count: i64| match count {
    ..0 => "?".to_owned(),
    count => count.to_string(),
  };
  let size = if details.size_when_done == details.total_size {
    format_bytes(details.total_size)
  } else {
    format!(
      "{} selected of {}",
      format_bytes(details.size_when_done),
      format_bytes(details.total_size),
    )
  };
  let priority = match details.bandwidth_priority {
    ..0 => "low",
    0 => "normal",
    _ => "high",
  };

  let width = 13;
  let mut lines = vec![
    field("Name", details.name.clone(), width),
    field("Hash", details.hash_string.clone(), width),
    field("Location", details.download_dir.clone(), width),
    field(
      "Labels",
      or_dash(&details.labels.as_deref().unwrap_or_default().join(", ")),
      width,
    ),
    field(
      "Private",
      if details.is_private { "yes" } else { "no" }.into(),
      width,
    ),
    field("Creator", or_dash(&details.creator), width),
    field("Comment", or_dash(&details.comment), width),
    Line::default(),
    field("State", status_label(details.status).to_owned(), width),
  ];
  if details.error != 0 {
    lines.push(
      field("Error", details.error_string.clone(), width)
        .style(error_style(details.error)),
    );
  }
  lines.extend([
    field(
      "Progress",
      format!("{:.0}%", (details.percent_done * 100.0).floor()),
      width,
    ),
    field("ETA", format_eta(details.eta), width),
    field(
      "Speed",
      format!(
        "↓ {}/s ↑ {}/s",
        format_bytes(details.rate_download),
        format_bytes(details.rate_upload),
      ),
      width,
    ),
    field(
      "Peers",
      format!(
        "{} connected, {} sending to us, {} receiving from us",
        details.peers_connected,
        details.peers_sending_to_us,
        details.peers_getting_from_us,
      ),
      width,
    ),
    field(
      "Queue",
      format!("position {}, {priority} priority", details.queue_position),
      width,
    ),
    Line::default(),
    field("Size", size, width),
    field("Left", format_bytes(details.left_until_done), width),
    field(
      "Downloaded",
      format!(
        "{} ({} corrupt)",
        format_bytes(details.downloaded_ever),
        format_bytes(details.corrupt_ever),
      ),
      width,
    ),
    field("Uploaded", format_bytes(details.uploaded_ever), width),
    field("Ratio", format_ratio(details.upload_ratio), width),
    field(
      "Pieces",
      format!(
        "{} × {}",
        details.piece_count,
        format_bytes(details.piece_size),
      ),
      width,
    ),
    field(
      "Files",
      details.file_count.map_or("?".into(), |n| n.to_string()),
      width,
    ),
    Line::default(),
    field("Added", date(details.added_date), width),
    field("Completed", date(details.done_date), width),
    field("Last active", date(details.activity_date), width),
    field(
      "Downloading",
      format_eta(details.seconds_downloading),
      width,
    ),
    field("Seeding", format_eta(details.seconds_seeding), width),
    Line::default(),
    Line::from("Trackers").bold(),
  ]);
  for tracker in &details.tracker_stats {
    let result = if tracker.has_announced {
      &tracker.last_announce_result
    } else {
      "not announced yet"
    };
    let line = field(
      &tracker.host,
      format!(
        "{} seeders, {} leechers, {result}",
        count(tracker.seeder_count),
        count(tracker.leecher_count),
      ),
      width,
    );
    if tracker.has_announced && !tracker.last_announce_succeeded {
      lines.push(line.yellow());
    } else {
      lines.push(line);
    }
  }
  Text::from(lines)
}

fn field(label: &str, value: String, width: usize) -> Line<'static> {
  Line::from(vec![
    Span::from(format!("{label:<width$}")).bold(),
    Span::from(value),
  ])
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

fn error_style(error: u64) -> Style {
  // Transmission: 1 and 2 are tracker warnings/errors, 3 is a local error.
  match error {
    1 | 2 => Style::new().yellow(),
    3 => Style::new().red(),
    _ => Style::new(),
  }
}

fn truncate(text: &str, width: usize) -> String {
  if text.width() <= width {
    return text.to_owned();
  }
  let mut truncated = String::new();
  let mut used = 0;
  for c in text.chars() {
    let char_width = c.width().unwrap_or(0);
    if used + char_width >= width {
      break;
    }
    truncated.push(c);
    used += char_width;
  }
  if width > 0 {
    truncated.push('…');
  }
  truncated
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
