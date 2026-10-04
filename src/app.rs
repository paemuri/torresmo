use std::io;
use std::path::Path;
use std::time::{Duration, Instant};

use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{
  self, Event, KeyCode, KeyEventKind, KeyModifiers,
};
use ratatui::widgets::TableState;

use crate::prompt::{expand_home, read_line};
use crate::rpc::Rpc;
use crate::transmission::{
  Snapshot, Torrent, TorrentDetails, TorrentStatus, add_torrent,
  auto_sort_by_ratio, delete_torrent, fetch, fetch_details, move_torrent,
  rename_torrent, torrent_action,
};

const REFRESH: Duration = Duration::from_secs(1);

#[derive(Clone, PartialEq)]
pub enum Mode {
  List,
  Help,
  Details(String),
  DeleteAskData(String),
  DeleteConfirm { hash: String, delete_data: bool },
}

pub fn show(terminal: &mut DefaultTerminal, rpc: &mut Rpc) -> io::Result<()> {
  let mut app = App::new();
  while !app.exit {
    if Instant::now() >= app.next_refresh {
      app.refresh(rpc);
    }
    terminal.draw(|frame| app.draw(frame))?;

    let timeout = app.next_refresh.saturating_duration_since(Instant::now());
    if event::poll(timeout)?
      && let Event::Key(key) = event::read()?
      && key.kind == KeyEventKind::Press
    {
      app.handle_key(key, rpc, terminal);
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
  pub exit: bool,
}

impl App {
  fn new() -> Self {
    Self {
      snapshot: Snapshot::default(),
      selection: TableState::default().with_selected(Some(0)),
      mode: Mode::List,
      details: None,
      refresh_error: None,
      action_error: None,
      next_refresh: Instant::now(),
      exit: false,
    }
  }

  fn refresh(&mut self, rpc: &mut Rpc) {
    match fetch(rpc) {
      Ok(new) => {
        keep_selected_torrent(
          &mut self.selection,
          &self.snapshot.torrents,
          &new.torrents,
        );
        self.snapshot = new;
        self.refresh_error = None;
      }
      Err(e) => self.refresh_error = Some(e.to_string()),
    }
    if let Mode::Details(hash) = &self.mode {
      match fetch_details(rpc, hash) {
        Ok(Some(new)) => self.details = Some(new),
        Ok(None) => self.mode = Mode::List,
        Err(e) => self.refresh_error = Some(e.to_string()),
      }
    }
    self.next_refresh = Instant::now() + REFRESH;
  }

  fn handle_key(
    &mut self,
    key: event::KeyEvent,
    rpc: &mut Rpc,
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
        match delete_torrent(rpc, hash, *delete_data) {
          Ok(()) => self.next_refresh = Instant::now(),
          Err(e) => self.action_error = Some(e.to_string()),
        }
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
      (Mode::Help | Mode::Details(_), _) => {}
      (_, KeyCode::Up) if shift => action = Some("queue-move-up"),
      (_, KeyCode::Down) if shift => action = Some("queue-move-down"),
      (_, KeyCode::Char('K')) => action = Some("queue-move-up"),
      (_, KeyCode::Char('J')) => action = Some("queue-move-down"),
      (_, KeyCode::Up | KeyCode::Char('k')) => self.selection.select_previous(),
      (_, KeyCode::Down | KeyCode::Char('j')) => self.selection.select_next(),
      (_, KeyCode::Enter | KeyCode::Char('i')) => {
        if let Some(torrent) = selected {
          match fetch_details(rpc, &torrent.hash_string) {
            Ok(Some(new)) => {
              self.details = Some(new);
              self.mode = Mode::Details(torrent.hash_string.clone());
            }
            Ok(None) => {}
            Err(e) => self.action_error = Some(e.to_string()),
          }
        }
      }
      (_, KeyCode::Char('a')) => {
        match read_line(terminal, "Add torrent (magnet, URL or path)", "") {
          Ok(Some(source)) if !source.trim().is_empty() => {
            match add_torrent(rpc, &expand_home(source.trim())) {
              Ok(()) => self.next_refresh = Instant::now(),
              Err(e) => self.action_error = Some(e.to_string()),
            }
          }
          Ok(_) => {}
          Err(e) => self.action_error = Some(e.to_string()),
        }
      }
      (_, KeyCode::Char('m')) => {
        if let Some(torrent) = selected {
          match read_line(terminal, "Move data to", &torrent.download_dir) {
            Ok(Some(location)) if !location.trim().is_empty() => {
              let location = expand_home(location.trim());
              if Path::new(&location) != Path::new(&torrent.download_dir) {
                match move_torrent(rpc, &torrent.hash_string, &location) {
                  Ok(()) => self.next_refresh = Instant::now(),
                  Err(e) => self.action_error = Some(e.to_string()),
                }
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
              let hash = &torrent.hash_string;
              match rename_torrent(rpc, hash, &torrent.name, name.trim()) {
                Ok(()) => self.next_refresh = Instant::now(),
                Err(e) => self.action_error = Some(e.to_string()),
              }
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
        match auto_sort_by_ratio(rpc, &self.snapshot.torrents) {
          Ok(()) => self.next_refresh = Instant::now(),
          Err(e) => self.action_error = Some(e.to_string()),
        }
      }
      _ => {}
    }

    if let (Some(method), Some(torrent)) = (action, selected) {
      match torrent_action(rpc, method, &torrent.hash_string) {
        Ok(()) => self.next_refresh = Instant::now(),
        Err(e) => self.action_error = Some(e.to_string()),
      }
    }
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
}
