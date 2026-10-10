use std::error::Error;
use std::io;
use std::path::Path;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{
  self, Event, KeyCode, KeyEventKind, KeyModifiers,
};
use ratatui::widgets::TableState;

use crate::prompt::{read_line, resolve_path, resolve_source};
use crate::rpc::Rpc;
use crate::transmission::{
  Snapshot, Torrent, TorrentDetails, TorrentStatus, add_torrent,
  auto_sort_by_ratio, delete_torrent, fetch, fetch_details, move_torrent,
  rename_torrent, torrent_action,
};
use crate::worker::{self, Job, Reply};

const REFRESH: Duration = Duration::from_secs(1);
// How long to wait for a key before checking the worker's replies again.
const POLL: Duration = Duration::from_millis(50);

pub enum Mode {
  List,
  Help,
  Details(String),
  DeleteAskData(String),
  DeleteConfirm { hash: String, delete_data: bool },
}

pub fn show(terminal: &mut DefaultTerminal, rpc: Rpc) -> io::Result<()> {
  let mut app = App::new(rpc);
  while !app.exit {
    app.receive_replies()?;
    app.refresh_if_due();
    terminal.draw(|frame| app.draw(frame))?;

    if event::poll(POLL)?
      && let Event::Key(key) = event::read()?
      && key.kind == KeyEventKind::Press
    {
      app.handle_key(key, terminal);
    }
  }
  Ok(())
}

pub struct App {
  pub snapshot: Snapshot,
  pub selection: TableState,
  pub mode: Mode,
  pub details: Option<TorrentDetails>,
  pub refresh_error: Option<String>,
  pub action_error: Option<String>,
  pub next_refresh: Instant,
  pub refreshing: bool,
  pub exit: bool,
  job_sender: Sender<Job>,
  reply_receiver: Receiver<Reply>,
}

impl App {
  fn new(rpc: Rpc) -> Self {
    let (job_sender, reply_receiver) = worker::spawn(rpc);
    Self {
      snapshot: Snapshot::default(),
      selection: TableState::default(),
      mode: Mode::List,
      details: None,
      refresh_error: None,
      action_error: None,
      next_refresh: Instant::now(),
      refreshing: false,
      exit: false,
      job_sender,
      reply_receiver,
    }
  }

  fn handle_key(
    &mut self,
    key: event::KeyEvent,
    terminal: &mut DefaultTerminal,
  ) {
    self.action_error = None;
    let selected = self
      .selection
      .selected()
      .and_then(|index| self.snapshot.torrents.get(index));
    let mut action = None;
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);

    match (&self.mode, key.code) {
      (Mode::DeleteAskData(hash), KeyCode::Char(answer @ ('y' | 'n'))) => {
        self.mode = Mode::DeleteConfirm {
          hash: hash.clone(),
          delete_data: answer == 'y',
        }
      }
      (Mode::DeleteAskData(_), KeyCode::Esc) => self.mode = Mode::List,
      (Mode::DeleteAskData(_), _) => {}
      (Mode::DeleteConfirm { hash, delete_data }, KeyCode::Char('y')) => {
        let (hash, delete_data) = (hash.clone(), *delete_data);
        self.act(move |rpc| delete_torrent(rpc, &hash, delete_data));
        self.mode = Mode::List;
      }
      // Just for safety measures, anything that isn't `y` during deletion
      // confirmation cancels the action.
      (Mode::DeleteConfirm { .. }, _) => self.mode = Mode::List,
      (_, KeyCode::Char('q')) => self.exit = true,
      (Mode::Help, KeyCode::Char('h') | KeyCode::F(1)) => {
        self.mode = Mode::List
      }
      (_, KeyCode::Char('h') | KeyCode::F(1)) => self.mode = Mode::Help,
      (_, KeyCode::Esc) => self.mode = Mode::List,
      (Mode::Details(_), KeyCode::Char('i')) => self.mode = Mode::List,
      (Mode::Help | Mode::Details(_), _) => {}
      (_, KeyCode::Up) if shift => action = Some("queue-move-up"),
      (_, KeyCode::Down) if shift => action = Some("queue-move-down"),
      (_, KeyCode::Char('K')) => action = Some("queue-move-up"),
      (_, KeyCode::Char('J')) => action = Some("queue-move-down"),
      (_, KeyCode::Up | KeyCode::Char('k')) => self.selection.select_previous(),
      (_, KeyCode::Down | KeyCode::Char('j')) => self.selection.select_next(),
      (_, KeyCode::Enter | KeyCode::Char('i')) => {
        if let Some(torrent) = selected {
          let hash = torrent.hash_string.clone();
          // The popup appears once the details arrive.
          self.details = None;
          self.mode = Mode::Details(hash.clone());
          self.request_details(hash);
        }
      }
      (_, KeyCode::Char('a')) => {
        match read_line(terminal, "Add torrent (magnet, URL or path)", "") {
          Ok(Some(source)) if !source.trim().is_empty() => {
            let source = resolve_source(source.trim());
            self.act(move |rpc| add_torrent(rpc, &source));
          }
          Ok(_) => {}
          Err(e) => self.action_error = Some(e.to_string()),
        }
      }
      (_, KeyCode::Char('m')) => {
        if let Some(torrent) = selected {
          match read_line(terminal, "Move data to", &torrent.download_dir) {
            Ok(Some(location)) if !location.trim().is_empty() => {
              let location = resolve_path(location.trim());
              if Path::new(&location) != Path::new(&torrent.download_dir) {
                let hash = torrent.hash_string.clone();
                self.act(move |rpc| move_torrent(rpc, &hash, &location));
              }
            }
            Ok(_) => {}
            Err(e) => self.action_error = Some(e.to_string()),
          }
        }
      }
      (_, KeyCode::Char('r')) => {
        if let Some(torrent) = selected {
          match read_line(terminal, "Rename torrent", &torrent.name) {
            Ok(Some(name))
              if !name.trim().is_empty() && name.trim() != torrent.name =>
            {
              let hash = torrent.hash_string.clone();
              let old_name = torrent.name.clone();
              let new_name = name.trim().to_owned();
              self.act(move |rpc| {
                rename_torrent(rpc, &hash, &old_name, &new_name)
              });
            }
            Ok(_) => {}
            Err(e) => self.action_error = Some(e.to_string()),
          }
        }
      }
      (_, KeyCode::Char('d') | KeyCode::Delete | KeyCode::Backspace) => {
        if let Some(torrent) = selected {
          self.mode = Mode::DeleteAskData(torrent.hash_string.clone());
        }
      }
      (_, KeyCode::Char('s')) => {
        action = selected.map(|torrent| match torrent.status {
          TorrentStatus::Stopped => "torrent-start",
          _ => "torrent-stop",
        })
      }
      (_, KeyCode::Char('S')) => action = Some("torrent-start-now"),
      (_, KeyCode::Char('v')) => action = Some("torrent-verify"),
      // Intentionally hidden from docs / Help popup.
      (_, KeyCode::Char('ç')) => {
        let torrents = self.snapshot.torrents.clone();
        self.act(move |rpc| auto_sort_by_ratio(rpc, &torrents));
      }
      _ => {}
    }

    if let (Some(method), Some(torrent)) = (action, selected) {
      let hash = torrent.hash_string.clone();
      self.act(move |rpc| torrent_action(rpc, method, &hash));
    }
  }

  fn send(&self, job: impl 'static + Send + FnOnce(&mut Rpc) -> Reply) {
    // A send only fails once the worker has stopped, which receive_replies
    // reports.
    let _ = self.job_sender.send(Box::new(job));
  }

  fn act<F>(&self, action: F)
  where
    F: 'static + Send + FnOnce(&mut Rpc) -> Result<(), Box<dyn Error>>,
  {
    self.send(move |rpc| Reply::Done(action(rpc).map_err(|e| e.to_string())));
  }

  fn request_details(&self, hash: String) {
    self.send(move |rpc| {
      let details = fetch_details(rpc, &hash)
        .map(|details| details.map(Box::new))
        .map_err(|e| e.to_string());
      Reply::Details(hash, details)
    });
  }

  fn refresh_if_due(&mut self) {
    // One refresh at a time, so a slow daemon doesn't pile up requests.
    if self.refreshing || Instant::now() < self.next_refresh {
      return;
    }
    self.refreshing = true;
    self.send(|rpc| Reply::Snapshot(fetch(rpc).map_err(|e| e.to_string())));
    if let Mode::Details(hash) = &self.mode {
      self.request_details(hash.clone());
    }
  }

  fn receive_replies(&mut self) -> io::Result<()> {
    loop {
      let reply = match self.reply_receiver.try_recv() {
        Ok(reply) => reply,
        Err(TryRecvError::Empty) => return Ok(()),
        Err(TryRecvError::Disconnected) => {
          return Err(io::Error::other("the RPC worker stopped unexpectedly"));
        }
      };

      match reply {
        Reply::Snapshot(result) => {
          self.refreshing = false;
          self.next_refresh = Instant::now() + REFRESH;
          match result {
            Ok(new) => {
              keep_selected_torrent(
                &mut self.selection,
                &self.snapshot.torrents,
                &new.torrents,
              );
              self.snapshot = new;
              self.refresh_error = None;
            }
            Err(e) => self.refresh_error = Some(e),
          }
        }
        // Details can arrive after the popup was closed or switched to another
        // torrent; only the open popup's torrent counts.
        Reply::Details(hash, result) if self.showing_details_for(&hash) => {
          match result {
            Ok(Some(new)) => self.details = Some(*new),
            Ok(None) => self.mode = Mode::List,
            Err(e) => self.action_error = Some(e),
          }
        }
        Reply::Details(..) => {}
        Reply::Done(result) => {
          if let Err(e) = result {
            self.action_error = Some(e);
          }
          self.next_refresh = Instant::now();
        }
      }
    }
  }

  fn showing_details_for(&self, hash: &str) -> bool {
    matches!(&self.mode, Mode::Details(open) if open == hash)
  }
}

fn keep_selected_torrent(
  selection: &mut TableState,
  old: &[Torrent],
  new: &[Torrent],
) {
  let selected = selection.selected().and_then(|index| old.get(index));

  // If the torrent is gone, the selection keeps its row number.
  if let Some(index) = selected.and_then(|selected| {
    new
      .iter()
      .position(|torrent| torrent.hash_string == selected.hash_string)
  }) {
    selection.select(Some(index));
  }

  // Start at the top when nothing's selected (e.g. on first refresh).
  if selection.selected().is_none() && !new.is_empty() {
    selection.select(Some(0));
  }
}

#[cfg(test)]
mod tests {
  use serde_json::json;

  use super::*;

  fn torrent(hash: &str) -> Torrent {
    serde_json::from_value(json!({
      "hashString": hash,
      "name": hash,
      "downloadDir": "/downloads",
      "status": 6,
      "sizeWhenDone": 0,
      "percentDone": 1,
      "eta": -1,
      "rateDownload": 0,
      "rateUpload": 0,
      "uploadRatio": 0,
      "error": 0,
      "queuePosition": 0
    }))
    .unwrap()
  }

  fn torrents(hashes: &[&str]) -> Vec<Torrent> {
    hashes.iter().map(|hash| torrent(hash)).collect()
  }

  #[test]
  fn selection_follows_a_moved_torrent() {
    let mut selection = TableState::default().with_selected(Some(1));
    let old = torrents(&["a", "b", "c"]);
    let new = torrents(&["b", "a", "c"]);
    keep_selected_torrent(&mut selection, &old, &new);
    assert_eq!(selection.selected(), Some(0));
  }

  #[test]
  fn selection_keeps_its_row_when_the_torrent_is_gone() {
    let mut selection = TableState::default().with_selected(Some(1));
    let old = torrents(&["a", "b", "c"]);
    let new = torrents(&["a", "c"]);
    keep_selected_torrent(&mut selection, &old, &new);
    assert_eq!(selection.selected(), Some(1));
  }

  #[test]
  fn selection_stays_on_the_first_row_after_an_empty_list() {
    let mut selection = TableState::default().with_selected(Some(0));
    keep_selected_torrent(&mut selection, &[], &torrents(&["a", "b"]));
    assert_eq!(selection.selected(), Some(0));
  }

  #[test]
  fn selection_starts_on_the_first_torrent() {
    let mut selection = TableState::default();
    keep_selected_torrent(&mut selection, &[], &torrents(&["a", "b"]));
    assert_eq!(selection.selected(), Some(0));
  }

  #[test]
  fn no_selection_without_torrents() {
    let mut selection = TableState::default();
    keep_selected_torrent(&mut selection, &[], &[]);
    assert_eq!(selection.selected(), None);
  }
}
