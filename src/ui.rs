use jiff::Timestamp;
use jiff::tz::TimeZone;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Clear, Padding, Paragraph, Row, Table, Wrap};

use crate::app::{App, Mode};
use crate::format::{
  format_bytes, format_eta, format_ratio, format_speed, status_label, truncate,
};
use crate::transmission::{Snapshot, Torrent, TorrentDetails};

const COLUMNS: usize = 9;
const NAME_COLUMN: usize = 1;
const HEADERS: [&str; COLUMNS] = [
  "#", "Name", "State", "Size", "%", "ETA", "Down", "Up", "Ratio",
];

impl App {
  pub fn draw(&mut self, frame: &mut Frame) {
    let [table_area, footer_area] =
      Layout::vertical([Constraint::Fill(1), Constraint::Length(1)])
        .areas(frame.area());

    let mut cells: Vec<[String; COLUMNS]> =
      self.snapshot.torrents.iter().map(torrent_cells).collect();
    let widths = column_widths(&cells);
    let [_, name_area, ..] = Layout::horizontal(widths)
      .spacing(1)
      .areas::<COLUMNS>(table_area);
    for row in &mut cells {
      row[NAME_COLUMN] = truncate(&row[NAME_COLUMN], name_area.width.into());
    }

    let rows =
      cells
        .iter()
        .zip(&self.snapshot.torrents)
        .map(|(row, torrent)| {
          table_row(row.each_ref().map(String::as_str))
            .style(error_style(torrent.error))
        });
    let table = Table::new(rows, widths)
      .header(table_row(HEADERS).bold())
      .row_highlight_style(Style::new().bg(Color::White).fg(Color::Black));
    let footer =
      match self.refresh_error.as_ref().or(self.action_error.as_ref()) {
        Some(error) => Line::from(format!("Error: {error}")).red(),
        None => Line::from(footer_text(&self.snapshot)),
      };

    frame.render_stateful_widget(table, table_area, &mut self.selection);
    frame.render_widget(footer, footer_area);

    match (&self.mode, &self.details) {
      (Mode::Help, _) => {
        small_popup(frame, " Help ", help_text(&self.snapshot))
      }
      (Mode::Details(_), Some(details)) => popup(
        frame,
        " Details ",
        details_text(details),
        Constraint::Percentage(80),
      ),
      (Mode::DeleteAskData(hash), _) => {
        let text = Text::from(vec![
          Line::from(torrent_name(&self.snapshot, hash)).bold(),
          Line::from(
            "Also delete its downloaded data? (y)es / (n)o / Esc to cancel",
          ),
        ]);
        small_popup(frame, " Delete torrent ", text);
      }
      (Mode::DeleteConfirm { hash, delete_data }, _) => {
        let mut text = Text::from(vec![
          Line::from(torrent_name(&self.snapshot, hash)).bold(),
          Line::from("Are you sure?"),
        ]);
        let title = if *delete_data {
          text = text.red();
          " Delete torrent and data "
        } else {
          " Delete torrent, keep data "
        };
        small_popup(frame, title, text);
      }
      _ => {}
    }
  }
}

fn popup(frame: &mut Frame, title: &str, text: Text, width: Constraint) {
  let area = frame
    .area()
    .centered(width, Constraint::Length(text.height() as u16 + 2));
  let paragraph = Paragraph::new(text).wrap(Wrap { trim: false }).block(
    Block::bordered()
      .title(title)
      .padding(Padding::horizontal(1)),
  );
  frame.render_widget(Clear, area);
  frame.render_widget(paragraph, area);
}

fn small_popup(frame: &mut Frame, title: &str, text: Text) {
  let width = Constraint::Length(text.width() as u16 + 4);
  popup(frame, title, text, width);
}

fn torrent_name(snapshot: &Snapshot, hash: &str) -> String {
  snapshot
    .torrents
    .iter()
    .find(|torrent| torrent.hash_string == hash)
    .map_or_else(|| hash.to_owned(), |torrent| torrent.name.clone())
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
    field("↑ ↓ / k j", "Move the selection".into(), 17),
    field("Shift+↑ ↓ / K J", "Move in the queue".into(), 17),
    field("i / Enter", "Show the torrent details".into(), 17),
    field("s", "Toggle pause (start/stop)".into(), 17),
    field("S", "Start now, skipping the queue".into(), 17),
    field("v", "Verify the downloaded data".into(), 17),
    field("a", "Add a torrent".into(), 17),
    field("m", "Move the data".into(), 17),
    field("r", "Rename the torrent".into(), 17),
    field("d / Del / Bksp", "Delete the torrent".into(), 17),
    field("Esc", "Close the popup".into(), 17),
    field("h / F1", "Show help".into(), 17),
    field("q", "Quit".into(), 17),
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
  for (number, tracker) in details.tracker_stats.iter().enumerate() {
    let result = if tracker.has_announced {
      &tracker.last_announce_result
    } else {
      "not announced yet"
    };
    let line = Line::from(format!(
      "{}. {} ({} seeders, {} leechers, {result})",
      number + 1,
      tracker.host,
      count(tracker.seeder_count),
      count(tracker.leecher_count),
    ));
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

fn torrent_cells(torrent: &Torrent) -> [String; COLUMNS] {
  [
    (torrent.queue_position + 1).to_string(),
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

fn column_widths(cells: &[[String; COLUMNS]]) -> [Constraint; COLUMNS] {
  let mut widths = HEADERS.map(|header| Line::from(header).width());
  for row in cells {
    for (width, cell) in widths.iter_mut().zip(row) {
      *width = (*width).max(Line::from(cell.as_str()).width());
    }
  }
  let mut constraints = widths.map(|width| Constraint::Length(width as u16));
  constraints[NAME_COLUMN] = Constraint::Fill(1);
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

fn table_row<'a, T: Into<Line<'a>>>(cells: [T; COLUMNS]) -> Row<'a> {
  let [queue, name, status, size, progress, eta, down, up, ratio] =
    cells.map(Into::<Line>::into);
  // Every column except Name and State holds a number, so it's right-aligned.
  Row::new([
    queue.right_aligned(),
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
