mod app;
mod format;
mod prompt;
mod rpc;
mod transmission;
mod ui;
mod worker;

use std::error::Error;
use std::process::ExitCode;

use rpc::Rpc;

const HELP: &str = "\
Dead-simple and minimal TUI client for the Transmission daemon.

Usage: torresmo [OPTION]

Options:
  -h, --help     Print this help and exit
  -v, --version  Print the version and exit

Inside Torresmo, press h for the key bindings.";

fn main() -> ExitCode {
  let args: Vec<String> = std::env::args().skip(1).collect();
  let args: Vec<&str> = args.iter().map(String::as_str).collect();
  match args.as_slice() {
    [] => {}
    ["-v" | "--version" | "version"] => {
      println!("torresmo {}", env!("CARGO_PKG_VERSION"));
      return ExitCode::SUCCESS;
    }
    ["-h" | "--help" | "help"] => {
      println!("{HELP}");
      return ExitCode::SUCCESS;
    }
    [_, ..] => {
      eprintln!(
        "unknown arguments: {}\nRun `torresmo --help` for usage.",
        args.join(" "),
      );
      return ExitCode::from(2);
    }
  }

  match run() {
    Ok(()) => ExitCode::SUCCESS,
    Err(e) => {
      eprintln!("Error: {e}");
      ExitCode::FAILURE
    }
  }
}

fn run() -> Result<(), Box<dyn Error>> {
  let rpc = Rpc::new()?;
  ratatui::run(|terminal| app::show(terminal, rpc))?;
  Ok(())
}
