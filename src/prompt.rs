use std::error::Error;
use std::io;

use ratatui::DefaultTerminal;
use ratatui::crossterm::cursor::{MoveTo, Show};
use ratatui::crossterm::execute;
use ratatui::crossterm::style::Print;
use ratatui::crossterm::terminal::{
  Clear as ClearTerminal, ClearType, disable_raw_mode, enable_raw_mode,
};
use rustyline::completion::FilenameCompleter;
use rustyline::error::ReadlineError;
use rustyline::{
  Cmd, Completer, CompletionType, Config, Editor, Helper, Highlighter, Hinter,
  KeyEvent, Modifiers, Validator,
};

#[derive(Helper, Completer, Highlighter, Hinter, Validator)]
struct PathHelper {
  #[rustyline(Completer)]
  completer: FilenameCompleter,
}

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

fn edit_line(initial: &str) -> rustyline::Result<String> {
  let mut editor = Editor::with_config(
    Config::builder()
      .completion_type(CompletionType::List)
      .keyseq_timeout(Some(25))
      .build(),
  )?;
  editor.set_helper(Some(PathHelper {
    completer: FilenameCompleter::new(),
  }));
  editor.bind_sequence(
    KeyEvent(rustyline::KeyCode::Esc, Modifiers::NONE),
    Cmd::Interrupt,
  );
  editor.readline_with_initial("", (initial, ""))
}

pub fn resolve_source(input: &str) -> String {
  // Magnet links and URLs go to the daemon as typed; anything else is a local
  // path.
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
  let expanded = unescape_completion(expanded);
  // Resolve to abs path for the daemon. Rel paths should be accepted as input
  // and, obviously, that messes up the daemon.
  std::path::absolute(&expanded)
    .map_or(expanded, |path| path.to_string_lossy().into_owned())
}

fn unescape_completion(path: String) -> String {
  #[cfg(unix)]
  {
    // Rustyline outputs a shell-escaped string. This simply removes any `\`
    // from the path.
    if path.contains('\\') && !std::path::Path::new(&path).exists() {
      return rustyline::completion::unescape(&path, Some('\\')).into_owned();
    }
  }
  path
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
  fn completion_escapes_are_removed() {
    let dir = fresh_dir("unescape");
    let file = dir.join("a b.torrent");
    std::fs::write(&file, "").unwrap();
    assert_eq!(
      resolve_path(dir.join("a\\ b.torrent").to_str().unwrap()),
      file.to_string_lossy()
    );
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[cfg(unix)]
  #[test]
  fn several_completion_escapes_are_removed() {
    let dir = fresh_dir("many-escapes");
    let file = dir.join("[AUTHOR] Name of the media.torrent");
    std::fs::write(&file, "").unwrap();
    let completed =
      dir.join("[AUTHOR]\\ Name\\ of\\ the\\ media.torrent");
    assert_eq!(
      resolve_path(completed.to_str().unwrap()),
      file.to_string_lossy()
    );
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[cfg(unix)]
  #[test]
  fn unescaped_paths_with_spaces_are_untouched() {
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
  fn completion_of_a_backslash_filename_is_unescaped_once() {
    let dir = fresh_dir("backslash-completion");
    // The real name has both a backslash and a space.
    let file = dir.join("a\\ b.torrent");
    std::fs::write(&file, "").unwrap();
    // Rustyline escapes the backslash as `\\` and the space as `\ `.
    let completed = dir.join("a\\\\\\ b.torrent");
    assert_eq!(
      resolve_path(completed.to_str().unwrap()),
      file.to_string_lossy()
    );
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[cfg(unix)]
  #[test]
  fn nonexistent_escaped_paths_are_unescaped() {
    let dir = fresh_dir("nonexistent");
    let completed = dir.join("missing\\ file.torrent");
    assert_eq!(
      resolve_path(completed.to_str().unwrap()),
      dir.join("missing file.torrent").to_string_lossy()
    );
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[cfg(unix)]
  #[test]
  fn literal_escaped_path_wins_when_both_forms_exist() {
    let dir = fresh_dir("ambiguous");
    let literal = dir.join("a\\ b.torrent");
    let plain = dir.join("a b.torrent");
    std::fs::write(&literal, "").unwrap();
    std::fs::write(&plain, "").unwrap();
    assert_eq!(
      resolve_path(dir.join("a\\ b.torrent").to_str().unwrap()),
      literal.to_string_lossy()
    );
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[cfg(unix)]
  #[test]
  fn resolve_source_unescapes_local_paths() {
    let dir = fresh_dir("source");
    let file = dir.join("a b.torrent");
    std::fs::write(&file, "").unwrap();
    assert_eq!(
      resolve_source(dir.join("a\\ b.torrent").to_str().unwrap()),
      file.to_string_lossy()
    );
    std::fs::remove_dir_all(&dir).unwrap();
  }
}
