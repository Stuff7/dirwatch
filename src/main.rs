mod channels;
mod cli;
mod dirwatch;
mod error;
mod http;
mod server;

use cli::Cli;
use error::Error;

#[cfg(unix)]
fn main() -> Result<(), Error> {
  let args = Cli::parse()?;
  server::run_server(&args)?;
  println!("Main exit");

  Ok(())
}

#[cfg(not(unix))]
fn main() {
  println!("Only Unix systems are supported")
}
