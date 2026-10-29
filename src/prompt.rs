use std::error::Error;
use std::fs;
use std::io;

use ratatui::DefaultTerminal;
use ratatui::crossterm::cursor::{MoveTo, Show};
use ratatui::crossterm::execute;
use ratatui::crossterm::style::Print;
use ratatui::crossterm::terminal::{
  Clear as ClearTerminal, ClearType, disable_raw_mode, enable_raw_mode,
};
use rustyline::Context;
use rustyline::completion::{Completer as CompleterTrait, Pair};
use rustyline::error::ReadlineError;
use rustyline::{
  Cmd, Completer, CompletionType, Config, Editor, Helper, Highlighter, Hinter,
  KeyEvent, Modifiers, Validator,
};

pub fn read_line(
  terminal: &mut DefaultTerminal,
  label: &str,
  initial: &str,
) -> Result<Option<String>, Box<dyn Error>> {
  disable_raw_mode()?;
  // Clear screen, show label in first line, prompt in the second line.
  let line = execute!(
    io::stdout(),
    ClearTerminal(ClearType::All),
    MoveTo(0, 0),
    Print(label),
    MoveTo(0, 1),
    Show
  )
  // Raw mode has to come back even if the prompt fails, or the TUI breaks.
  .map_err(ReadlineError::from)
  .and_then(|()| edit_line(initial));
  enable_raw_mode()?;
  terminal.clear()?;
  match line {
    Ok(line) => Ok(Some(line)),
    Err(ReadlineError::Interrupted | ReadlineError::Eof) => Ok(None),
    Err(e) => Err(e.into()),
  }
}

pub fn resolve_source(input: &str) -> String {
  // Magnet links and URLs go to the daemon as typed; anything else is a local
  // path and should be resolved.
  if !is_local(input) {
    return input.to_owned();
  }
  resolve_path(input)
}

pub fn is_local(input: &str) -> bool {
  !input.starts_with("magnet:") && !input.contains("://")
}

pub fn resolve_path(input: &str) -> String {
  let expanded = expand_home(input);

  // Resolve to abs path for the daemon. Rel paths should be accepted as input
  // and, obviously, that messes up the daemon.
  std::path::absolute(&expanded)
    .map_or(expanded, |path| path.to_string_lossy().into_owned())
}

fn edit_line(initial: &str) -> rustyline::Result<String> {
  let mut editor = Editor::with_config(
    Config::builder()
      .completion_type(CompletionType::List)
      .keyseq_timeout(Some(25))
      .build(),
  )?;
  editor.set_helper(Some(PathHelper {
    completer: PathCompleter,
  }));
  editor.bind_sequence(
    KeyEvent(rustyline::KeyCode::Esc, Modifiers::NONE),
    Cmd::Interrupt,
  );
  editor.readline_with_initial("", (initial, ""))
}

/// Custom path completer. Completes paths without shell escaping. See
/// path_completions.
struct PathCompleter;

impl CompleterTrait for PathCompleter {
  type Candidate = Pair;

  fn complete(
    &self,
    line: &str,
    pos: usize,
    _ctx: &Context<'_>,
  ) -> rustyline::Result<(usize, Vec<Pair>)> {
    // 0 here forces Rustyline to replace the whole line.
    Ok((0, path_completions(&line[..pos])))
  }
}

/// Just helps wiring PathCompleter into Rustyline.
#[derive(Helper, Completer, Highlighter, Hinter, Validator)]
struct PathHelper {
  #[rustyline(Completer)]
  completer: PathCompleter,
}

// Tab completion during input (Rustyline).
fn path_completions(input: &str) -> Vec<Pair> {
  let expanded = expand_home(input);

  // Split full dir path and the typed entry based on the last `/`. The latter
  // is the one to be tab-completed.
  let (dir, typed_entry) = match expanded.rfind('/') {
    Some(index) => (&expanded[..=index], &expanded[index + 1..]),
    None => ("", expanded.as_str()),
  };

  // Keep the directory as typed, so `~/` stays `~/` in the replacement.
  let typed_dir = &input[..input.len() - typed_entry.len()];

  // Simply run `ls` on dir (or `.`, if empty) and compare the entries. If 
  // typed_entry is a prefix of the actual entry, add it as a possible match. In
  // the end, those matches will be shown as candidates during tab completion.
  let mut matches = Vec::new();
  if let Ok(entries) = fs::read_dir(if dir.is_empty() { "." } else { dir }) {
    for entry in entries.flatten() {
      let Ok(name) = entry.file_name().into_string() else {
        continue;
      };
      if !name.starts_with(typed_entry) {
        continue;
      }
      let mut replacement = format!("{typed_dir}{name}");
      if entry.path().is_dir() {
        replacement.push('/');
      }
      matches.push(Pair {
        display: name,
        replacement,
      });
    }
  }

  matches.sort_by(|a, b| a.display.cmp(&b.display));
  matches
}

fn expand_home(path: &str) -> String {
  match (path.strip_prefix("~/"), std::env::var("HOME")) {
    (Some(rest), Ok(home)) => format!("{home}/{rest}"),
    _ => path.to_owned(),
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn expands_a_leading_tilde_slash() {
    let home = std::env::var("HOME").unwrap();
    assert_eq!(expand_home("~/movies"), format!("{home}/movies"));
  }

  #[test]
  fn leaves_other_paths_alone() {
    assert_eq!(expand_home("/downloads"), "/downloads");
    assert_eq!(
      expand_home("magnet:?xt=urn:btih:abc"),
      "magnet:?xt=urn:btih:abc"
    );
    assert_eq!(expand_home("~other/movies"), "~other/movies");
    assert_eq!(expand_home("~"), "~");
  }

  #[test]
  fn relative_paths_become_absolute() {
    let current = std::env::current_dir().unwrap();
    let expected = current.join("movies/x.torrent");
    assert_eq!(resolve_path("movies/x.torrent"), expected.to_string_lossy());
  }

  #[test]
  fn absolute_and_home_paths_stay_absolute() {
    let home = std::env::var("HOME").unwrap();
    assert_eq!(resolve_path("/downloads"), "/downloads");
    assert_eq!(resolve_path("~/movies"), format!("{home}/movies"));
  }

  #[test]
  fn magnets_and_urls_are_sent_as_typed() {
    let magnet = "magnet:?xt=urn:btih:abc";
    let url = "https://example.org/x.torrent";
    assert_eq!(resolve_source(magnet), magnet);
    assert_eq!(resolve_source(url), url);
    assert!(resolve_source("x.torrent").starts_with('/'));
  }

  #[test]
  fn only_paths_are_local() {
    assert!(!is_local("magnet:?xt=urn:btih:abc"));
    assert!(!is_local("https://example.org/x.torrent"));
    assert!(is_local("x.torrent"));
    assert!(is_local("~/x.torrent"));
  }

  #[cfg(unix)]
  fn fresh_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir()
      .join(format!("torresmo-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
  }

  #[cfg(unix)]
  #[test]
  fn paths_with_spaces_are_untouched() {
    let dir = fresh_dir("spaces");
    let file = dir.join("a b.torrent");
    std::fs::write(&file, "").unwrap();
    assert_eq!(resolve_path(file.to_str().unwrap()), file.to_string_lossy());
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[cfg(unix)]
  #[test]
  fn a_real_backslash_in_a_filename_is_not_an_escape() {
    let dir = fresh_dir("backslash");
    let file = dir.join("a\\ b.torrent");
    std::fs::write(&file, "").unwrap();
    assert_eq!(resolve_path(file.to_str().unwrap()), file.to_string_lossy());
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[cfg(unix)]
  #[test]
  fn shell_escapes_are_not_interpreted() {
    let dir = fresh_dir("literal-escapes");
    let escaped = dir.join("a\\ b.torrent");
    assert_eq!(
      resolve_path(escaped.to_str().unwrap()),
      escaped.to_string_lossy()
    );
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[cfg(unix)]
  #[test]
  fn completer_returns_unescaped_paths() {
    let dir = fresh_dir("completer");
    let file = dir.join("a b.torrent");
    std::fs::write(&file, "").unwrap();
    let matches = path_completions(&format!("{}/a", dir.to_str().unwrap()));
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].display, "a b.torrent");
    assert_eq!(matches[0].replacement, file.to_string_lossy());
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[cfg(unix)]
  #[test]
  fn completer_appends_a_slash_to_directories() {
    let dir = fresh_dir("completer-dir");
    std::fs::create_dir_all(dir.join("sub dir")).unwrap();
    let matches = path_completions(&format!("{}/su", dir.to_str().unwrap()));
    assert_eq!(matches.len(), 1);
    assert_eq!(
      matches[0].replacement,
      format!("{}/sub dir/", dir.to_str().unwrap())
    );
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[cfg(unix)]
  #[test]
  fn completer_keeps_the_whole_line_as_the_path() {
    // The directory itself contains spaces, which a word-splitting completer
    // would treat as separate words.
    let dir = fresh_dir("completer spaces");
    let sub = dir.join("sub dir");
    std::fs::create_dir_all(&sub).unwrap();
    let file = sub.join("a b.torrent");
    std::fs::write(&file, "").unwrap();
    let partial = format!("{}/a", sub.to_str().unwrap());
    let matches = path_completions(&partial);
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].replacement, file.to_string_lossy());
    std::fs::remove_dir_all(&dir).unwrap();
  }
}
