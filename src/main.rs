mod format;
mod rpc;
mod transmission;
mod ui;

use std::error::Error;
use std::process::ExitCode;

use rpc::Rpc;

fn main() -> ExitCode {
  match run() {
    Ok(()) => ExitCode::SUCCESS,
    Err(e) => {
      eprintln!("Error: {e}");
      ExitCode::FAILURE
    }
  }
}

fn run() -> Result<(), Box<dyn Error>> {
  let mut rpc = Rpc::new()?;
  ratatui::run(|terminal| ui::show(terminal, &mut rpc))?;
  Ok(())
}
