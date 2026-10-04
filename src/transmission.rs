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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Torrent {
  pub hash_string: String,
  pub name: String,
  pub status: TorrentStatus,
  pub size_when_done: u64,
  pub percent_done: f64,
  pub eta: i64,
  pub rate_download: u64,
  pub rate_upload: u64,
  pub upload_ratio: f64,
  pub error: u64,
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
