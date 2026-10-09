use std::error::Error;

use serde::Deserialize;
use serde_json::json;

use crate::rpc::Rpc;

#[derive(Default)]
pub struct Snapshot {
  pub version: String,
  pub download_dir: String,
  pub speed_down: u64,
  pub speed_up: u64,
  pub total_down: u64,
  pub total_up: u64,
  pub torrents: Vec<Torrent>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Torrent {
  pub hash_string: String,
  pub name: String,
  pub download_dir: String,
  pub status: TorrentStatus,
  pub size_when_done: u64,
  pub percent_done: f64,
  pub eta: i64,
  pub rate_download: u64,
  pub rate_upload: u64,
  pub upload_ratio: f64,
  pub error: u64,
  pub queue_position: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TorrentDetails {
  pub name: String,
  pub hash_string: String,
  pub download_dir: String,
  pub labels: Option<Vec<String>>,
  pub is_private: bool,
  pub creator: String,
  pub comment: String,
  pub status: TorrentStatus,
  pub error: u64,
  pub error_string: String,
  pub percent_done: f64,
  pub eta: i64,
  pub rate_download: u64,
  pub rate_upload: u64,
  pub peers_connected: u64,
  pub peers_sending_to_us: u64,
  pub peers_getting_from_us: u64,
  pub queue_position: i64,
  pub bandwidth_priority: i64,
  pub total_size: u64,
  pub size_when_done: u64,
  pub left_until_done: u64,
  pub downloaded_ever: u64,
  pub uploaded_ever: u64,
  pub corrupt_ever: u64,
  pub upload_ratio: f64,
  pub piece_count: u64,
  pub piece_size: u64,
  pub file_count: Option<u64>,
  pub added_date: i64,
  pub done_date: i64,
  pub activity_date: i64,
  pub seconds_downloading: i64,
  pub seconds_seeding: i64,
  pub tracker_stats: Vec<TrackerStats>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackerStats {
  pub host: String,
  pub seeder_count: i64,
  pub leecher_count: i64,
  pub has_announced: bool,
  pub last_announce_succeeded: bool,
  pub last_announce_result: String,
}

#[derive(Clone, Copy, PartialEq, Deserialize)]
#[serde(from = "u64")]
pub enum TorrentStatus {
  Stopped,
  VerifyQueued,
  Verifying,
  DownloadQueued,
  Downloading,
  SeedQueued,
  Seeding,
  Unknown,
}

impl From<u64> for TorrentStatus {
  fn from(code: u64) -> Self {
    match code {
      0 => Self::Stopped,
      1 => Self::VerifyQueued,
      2 => Self::Verifying,
      3 => Self::DownloadQueued,
      4 => Self::Downloading,
      5 => Self::SeedQueued,
      6 => Self::Seeding,
      _ => Self::Unknown,
    }
  }
}

pub fn fetch(rpc: &mut Rpc) -> Result<Snapshot, Box<dyn Error>> {
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
    "hashString",
    "name",
    "status",
    "sizeWhenDone",
    "percentDone",
    "eta",
    "rateDownload",
    "rateUpload",
    "uploadRatio",
    "error",
    "queuePosition",
    "downloadDir",
  ];
  let mut reply = rpc.call("torrent-get", json!({ "fields": fields }))?;
  let mut torrents: Vec<Torrent> =
    serde_json::from_value(reply["torrents"].take())?;
  torrents.sort_by_key(|torrent| torrent.queue_position);

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

pub fn torrent_action(
  rpc: &mut Rpc,
  method: &str,
  hash: &str,
) -> Result<(), Box<dyn Error>> {
  rpc.call(method, json!({ "ids": [hash] }))?;
  Ok(())
}

pub fn add_torrent(rpc: &mut Rpc, source: &str) -> Result<(), Box<dyn Error>> {
  let reply = rpc.call("torrent-add", json!({ "filename": source }))?;
  if let Some(name) = reply["torrent-duplicate"]["name"].as_str() {
    return Err(format!("Already added: {name}").into());
  }
  Ok(())
}

pub fn move_torrent(
  rpc: &mut Rpc,
  hash: &str,
  location: &str,
) -> Result<(), Box<dyn Error>> {
  rpc.call(
    "torrent-set-location",
    json!({ "ids": [hash], "location": location, "move": true }),
  )?;
  Ok(())
}

pub fn rename_torrent(
  rpc: &mut Rpc,
  hash: &str,
  old_name: &str,
  new_name: &str,
) -> Result<(), Box<dyn Error>> {
  rpc.call(
    "torrent-rename-path",
    json!({ "ids": [hash], "path": old_name, "name": new_name }),
  )?;
  Ok(())
}

pub fn delete_torrent(
  rpc: &mut Rpc,
  hash: &str,
  delete_data: bool,
) -> Result<(), Box<dyn Error>> {
  rpc.call(
    "torrent-remove",
    json!({ "ids": [hash], "delete-local-data": delete_data }),
  )?;
  Ok(())
}

pub fn auto_sort_by_ratio(
  rpc: &mut Rpc,
  torrents: &[Torrent],
) -> Result<(), Box<dyn Error>> {
  // Transmission sends -1 (not available) and -2 (infinite) for the ratio.
  let rank = |torrent: &Torrent| match torrent.upload_ratio {
    -2.0 => f64::INFINITY,
    ratio => ratio.max(0.0),
  };

  let mut by_ratio: Vec<&Torrent> = torrents.iter().collect();
  by_ratio.sort_by(|a, b| rank(a).total_cmp(&rank(b)));

  // Each move puts the torrent at #1, so the lowest ratio has to go last.
  for torrent in by_ratio.iter().rev() {
    rpc.call("queue-move-top", json!({ "ids": [torrent.hash_string] }))?;
  }
  Ok(())
}

pub fn fetch_details(
  rpc: &mut Rpc,
  hash: &str,
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
    json!({ "ids": [hash], "fields": fields.as_slice() }),
  )?;
  let torrents: Vec<TorrentDetails> =
    serde_json::from_value(reply["torrents"].take())?;
  Ok(torrents.into_iter().next())
}

#[cfg(test)]
mod tests {
  use serde_json::json;

  use super::*;

  #[test]
  fn status_codes_map_to_variants() {
    assert!(TorrentStatus::from(0) == TorrentStatus::Stopped);
    assert!(TorrentStatus::from(4) == TorrentStatus::Downloading);
    assert!(TorrentStatus::from(6) == TorrentStatus::Seeding);
    assert!(TorrentStatus::from(7) == TorrentStatus::Unknown);
  }

  #[test]
  fn torrent_parses_from_transmission_json() {
    let torrent: Torrent = serde_json::from_value(json!({
      "hashString": "c9e15763f722f23e98a29decdfae341b98d53056",
      "name": "Foo",
      "downloadDir": "/downloads",
      "status": 6,
      "sizeWhenDone": 1_500_000_000u64,
      "percentDone": 1,
      "eta": -1,
      "rateDownload": 0,
      "rateUpload": 17_600,
      "uploadRatio": 15.1,
      "error": 0,
      "queuePosition": 3
    }))
    .unwrap();

    assert_eq!(
      torrent.hash_string,
      "c9e15763f722f23e98a29decdfae341b98d53056"
    );
    assert_eq!(torrent.name, "Foo");
    assert!(torrent.status == TorrentStatus::Seeding);
    assert_eq!(torrent.percent_done, 1.0);
    assert_eq!(torrent.eta, -1);
    assert_eq!(torrent.rate_upload, 17_600);
    assert_eq!(torrent.queue_position, 3);
  }

  #[test]
  fn details_parse_without_version_dependent_fields() {
    let details: TorrentDetails = serde_json::from_value(json!({
      "name": "Foo",
      "hashString": "c9e15763f722f23e98a29decdfae341b98d53056",
      "downloadDir": "/downloads",
      "isPrivate": false,
      "creator": "",
      "comment": "",
      "status": 3,
      "error": 3,
      "errorString": "Please Verify Local Data! Piece #1591 is corrupt.",
      "percentDone": 0.42,
      "eta": 5000,
      "rateDownload": 1_300_000,
      "rateUpload": 0,
      "peersConnected": 3,
      "peersSendingToUs": 1,
      "peersGettingFromUs": 0,
      "queuePosition": 0,
      "bandwidthPriority": 0,
      "totalSize": 1_600_000_000u64,
      "sizeWhenDone": 1_500_000_000u64,
      "leftUntilDone": 870_000_000,
      "downloadedEver": 630_000_000,
      "uploadedEver": 0,
      "corruptEver": 0,
      "uploadRatio": -1,
      "pieceCount": 1432,
      "pieceSize": 1_048_576,
      "addedDate": 1_700_000_000,
      "doneDate": 0,
      "activityDate": 1_700_000_100,
      "secondsDownloading": 3600,
      "secondsSeeding": 0,
      "trackerStats": [{
        "host": "tracker.example.org:1337",
        "seederCount": -1,
        "leecherCount": -1,
        "hasAnnounced": false,
        "lastAnnounceSucceeded": false,
        "lastAnnounceResult": ""
      }]
    }))
    .unwrap();

    assert!(details.labels.is_none());
    assert!(details.file_count.is_none());
    assert!(details.status == TorrentStatus::DownloadQueued);
    assert_eq!(details.upload_ratio, -1.0);
    assert_eq!(details.tracker_stats.len(), 1);
    assert_eq!(details.tracker_stats[0].host, "tracker.example.org:1337");
  }
}
