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
  let height = terminal.size()?.height;
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

pub fn expand_home(path: &str) -> String {
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
}
