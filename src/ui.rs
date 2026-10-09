use jiff::Timestamp;
use jiff::tz::TimeZone;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Margin};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Clear, Padding, Paragraph, Row, Table, Wrap};

use crate::app::{App, Mode};
use crate::format::{
  format_bytes, format_duration, format_ratio, format_speed, status_label,
  truncate,
};
use crate::transmission::{Snapshot, Torrent, TorrentDetails};

const COLUMNS: usize = 9;
const NAME_COLUMN: usize = 1;
const COLUMN_SPACING: u16 = 2;
const KEY_WIDTH: usize = 17;
const LABEL_WIDTH: usize = 13;
const HEADERS: [&str; COLUMNS] = [
  "#", "Name", "State", "Size", "%", "ETA", "Down", "Up", "Ratio",
];

impl App {
  pub fn draw(&mut self, frame: &mut Frame) {
    let [table_area, footer_area] =
      Layout::vertical([Constraint::Fill(1), Constraint::Length(1)])
        .areas(frame.area());
    let table_area = table_area.inner(Margin::new(1, 0));

    let mut cells: Vec<[String; COLUMNS]> =
      self.snapshot.torrents.iter().map(torrent_cells).collect();
    let widths = column_widths(&cells);
    let [_, name_area, ..] = Layout::horizontal(widths)
      .spacing(COLUMN_SPACING)
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
    let highlight = self
      .selection
      .selected()
      .and_then(|index| self.snapshot.torrents.get(index))
      .and_then(|torrent| error_color(torrent.error))
      .unwrap_or(Color::White);
    let table = Table::new(rows, widths)
      .column_spacing(COLUMN_SPACING)
      .header(table_row(HEADERS).bold())
      .row_highlight_style(Style::new().bg(highlight).fg(Color::Black));
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
    field("↑ ↓ / k j", "Move the selection".into(), KEY_WIDTH),
    field("Shift+↑ ↓ / K J", "Move in the queue".into(), KEY_WIDTH),
    field("i / Enter", "Show the torrent details".into(), KEY_WIDTH),
    field("s", "Toggle pause (start/stop)".into(), KEY_WIDTH),
    field("S", "Start now, skipping the queue".into(), KEY_WIDTH),
    field("v", "Verify the downloaded data".into(), KEY_WIDTH),
    field("a", "Add a torrent".into(), KEY_WIDTH),
    field("m", "Move the data".into(), KEY_WIDTH),
    field("r", "Rename the torrent".into(), KEY_WIDTH),
    field("d / Del / Bksp", "Delete the torrent".into(), KEY_WIDTH),
    field("Esc", "Close the popup".into(), KEY_WIDTH),
    field("h / F1", "Show help".into(), KEY_WIDTH),
    field("q", "Quit".into(), KEY_WIDTH),
    Line::default(),
    field("Location", snapshot.download_dir.clone(), LABEL_WIDTH),
    field("Transmission", snapshot.version.clone(), LABEL_WIDTH),
    field("Torresmo", env!("CARGO_PKG_VERSION").into(), LABEL_WIDTH),
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
      format_duration(now - timestamp),
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

  let mut lines = vec![
    field("Name", details.name.clone(), LABEL_WIDTH),
    field("Hash", details.hash_string.clone(), LABEL_WIDTH),
    field("Location", details.download_dir.clone(), LABEL_WIDTH),
    field(
      "Labels",
      or_dash(&details.labels.as_deref().unwrap_or_default().join(", ")),
      LABEL_WIDTH,
    ),
    field(
      "Private",
      if details.is_private { "yes" } else { "no" }.into(),
      LABEL_WIDTH,
    ),
    field("Creator", or_dash(&details.creator), LABEL_WIDTH),
    field("Comment", or_dash(&details.comment), LABEL_WIDTH),
    Line::default(),
    field(
      "State",
      status_label(details.status).to_owned(),
      LABEL_WIDTH,
    ),
  ];
  if details.error != 0 {
    lines.push(
      field("Error", details.error_string.clone(), LABEL_WIDTH)
        .style(error_style(details.error)),
    );
  }
  lines.extend([
    field(
      "Progress",
      format!("{:.0}%", (details.percent_done * 100.0).floor()),
      LABEL_WIDTH,
    ),
    field("ETA", format_duration(details.eta), LABEL_WIDTH),
    field(
      "Speed",
      format!(
        "↓ {}/s ↑ {}/s",
        format_bytes(details.rate_download),
        format_bytes(details.rate_upload),
      ),
      LABEL_WIDTH,
    ),
    field(
      "Peers",
      format!(
        "{} connected, {} sending to us, {} receiving from us",
        details.peers_connected,
        details.peers_sending_to_us,
        details.peers_getting_from_us,
      ),
      LABEL_WIDTH,
    ),
    field(
      "Queue",
      format!("position {}, {priority} priority", details.queue_position),
      LABEL_WIDTH,
    ),
    Line::default(),
    field("Size", size, LABEL_WIDTH),
    field("Left", format_bytes(details.left_until_done), LABEL_WIDTH),
    field(
      "Downloaded",
      format!(
        "{} ({} corrupt)",
        format_bytes(details.downloaded_ever),
        format_bytes(details.corrupt_ever),
      ),
      LABEL_WIDTH,
    ),
    field("Uploaded", format_bytes(details.uploaded_ever), LABEL_WIDTH),
    field("Ratio", format_ratio(details.upload_ratio), LABEL_WIDTH),
    field(
      "Pieces",
      format!(
        "{} × {}",
        details.piece_count,
        format_bytes(details.piece_size),
      ),
      LABEL_WIDTH,
    ),
    field(
      "Files",
      details.file_count.map_or("?".into(), |n| n.to_string()),
      LABEL_WIDTH,
    ),
    Line::default(),
    field("Added", date(details.added_date), LABEL_WIDTH),
    field("Completed", date(details.done_date), LABEL_WIDTH),
    field("Last active", date(details.activity_date), LABEL_WIDTH),
    field(
      "Downloading",
      format_duration(details.seconds_downloading),
      LABEL_WIDTH,
    ),
    field(
      "Seeding",
      format_duration(details.seconds_seeding),
      LABEL_WIDTH,
    ),
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
    format_duration(torrent.eta),
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

fn error_color(error: u64) -> Option<Color> {
  // Transmission: 1 and 2 are tracker warnings/errors, 3 is a local error.
  match error {
    1 | 2 => Some(Color::Yellow),
    3 => Some(Color::Red),
    _ => None,
  }
}

fn error_style(error: u64) -> Style {
  error_color(error).map_or(Style::new(), |color| Style::new().fg(color))
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

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn columns_fit_their_widest_cell_and_name_fills_the_rest() {
    let cells = [[
      "10",
      "Foo",
      "Downloading",
      "1.5 GB",
      "42",
      "1h 23m",
      "1.3 MB/s",
      "",
      "0.05",
    ]
    .map(String::from)];
    let widths = column_widths(&cells);
    assert_eq!(widths[NAME_COLUMN], Constraint::Fill(1));
    assert_eq!(widths[0], Constraint::Length(2));
    assert_eq!(widths[2], Constraint::Length(11));
    // Columns wider in the header than in any cell keep the header's width.
    assert_eq!(widths[4], Constraint::Length(2));
    assert_eq!(widths[7], Constraint::Length(2));
  }
}
