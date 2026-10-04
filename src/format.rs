use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::transmission::TorrentStatus;

pub fn format_bytes(bytes: u64) -> String {
  // Decimal units (1 kB = 1000 B), the same as Transmission's own clients.
  const UNITS: [&str; 4] = ["kB", "MB", "GB", "TB"];
  if bytes < 1000 {
    return format!("{bytes} B");
  }
  let mut value = bytes as f64 / 1000.0;
  let mut unit = 0;
  // `{:.1}` rounds 999.95 up to "1000.0", so that already counts as the next
  // unit.
  while value >= 999.95 && unit < UNITS.len() - 1 {
    value /= 1000.0;
    unit += 1;
  }
  format!("{value:.1} {}", UNITS[unit])
}

pub fn format_speed(bytes_per_second: u64) -> String {
  if bytes_per_second == 0 {
    return String::new();
  }
  format!("{}/s", format_bytes(bytes_per_second))
}

pub fn format_duration(seconds: i64) -> String {
  // Negative means unknown: Transmission sends -1 (not available) and -2
  // (unknown) for an ETA it can't compute.
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

pub fn format_ratio(ratio: f64) -> String {
  // Transmission sends -1 (not available) and -2 (infinite) for the ratio.
  match ratio {
    -1.0 => "-".to_owned(),
    -2.0 => "∞".to_owned(),
    _ => format!("{ratio:.2}"),
  }
}

pub fn status_label(status: TorrentStatus) -> &'static str {
  match status {
    TorrentStatus::Stopped => "Stopped",
    TorrentStatus::VerifyQueued
    | TorrentStatus::DownloadQueued
    | TorrentStatus::SeedQueued => "Queued",
    TorrentStatus::Verifying => "Verifying",
    TorrentStatus::Downloading => "Downloading",
    TorrentStatus::Seeding => "Seeding",
    TorrentStatus::Unknown => "Unknown",
  }
}

pub fn truncate(text: &str, width: usize) -> String {
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
