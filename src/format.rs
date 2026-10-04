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

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn bytes_use_decimal_units() {
    assert_eq!(format_bytes(0), "0 B");
    assert_eq!(format_bytes(999), "999 B");
    assert_eq!(format_bytes(1000), "1.0 kB");
    assert_eq!(format_bytes(1_234_567), "1.2 MB");
    assert_eq!(format_bytes(4_200_000_000), "4.2 GB");
  }

  #[test]
  fn bytes_round_up_into_the_next_unit() {
    assert_eq!(format_bytes(999_949), "999.9 kB");
    assert_eq!(format_bytes(999_950), "1.0 MB");
  }

  #[test]
  fn bytes_stop_at_terabytes() {
    assert_eq!(format_bytes(u64::MAX), "18446744.1 TB");
  }

  #[test]
  fn speed_hides_zero() {
    assert_eq!(format_speed(0), "");
    assert_eq!(format_speed(1500), "1.5 kB/s");
  }

  #[test]
  fn duration_shows_the_two_largest_units() {
    assert_eq!(format_duration(0), "0s");
    assert_eq!(format_duration(59), "59s");
    assert_eq!(format_duration(61), "1m 1s");
    assert_eq!(format_duration(3_600), "1h 0m");
    assert_eq!(format_duration(5_000), "1h 23m");
    assert_eq!(format_duration(90_000), "1d 1h");
  }

  #[test]
  fn duration_is_a_dash_when_unknown() {
    assert_eq!(format_duration(-1), "-");
    assert_eq!(format_duration(-2), "-");
  }

  #[test]
  fn ratio_handles_special_values() {
    assert_eq!(format_ratio(-1.0), "-");
    assert_eq!(format_ratio(-2.0), "∞");
    assert_eq!(format_ratio(0.0), "0.00");
    assert_eq!(format_ratio(15.1), "15.10");
  }

  #[test]
  fn queued_states_share_one_label() {
    assert_eq!(status_label(TorrentStatus::VerifyQueued), "Queued");
    assert_eq!(status_label(TorrentStatus::DownloadQueued), "Queued");
    assert_eq!(status_label(TorrentStatus::SeedQueued), "Queued");
    assert_eq!(status_label(TorrentStatus::Seeding), "Seeding");
  }

  #[test]
  fn truncate_keeps_text_that_fits() {
    assert_eq!(truncate("abc", 3), "abc");
    assert_eq!(truncate("abc", 10), "abc");
  }

  #[test]
  fn truncate_ends_with_an_ellipsis() {
    assert_eq!(truncate("abcdef", 4), "abc…");
    assert_eq!(truncate("abcdef", 1), "…");
    assert_eq!(truncate("abcdef", 0), "");
  }

  #[test]
  fn truncate_counts_terminal_cells() {
    // Each of these characters takes two cells.
    assert_eq!(truncate("日本語", 5), "日本…");
    assert_eq!(truncate("日本語", 4), "日…");
  }
}
