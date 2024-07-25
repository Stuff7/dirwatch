mod dirwatch;
mod encoding;
mod error;
mod ws;

use error::Error;

#[cfg(unix)]
fn main() -> Result<(), Error> {
  let args = Cli::parse()?;
  println!("cmd: {:?}", args.cmd);
  ws::run_server(&args.dir)
}

struct Cli {
  dir: String,
  cmd: String,
}

impl Cli {
  pub fn parse() -> Result<Self, Error> {
    let mut raw_args = std::env::args();
    let dir = raw_args.nth(1).ok_or(Error::MissingArg("directory"))?;
    let cmd = raw_args.next().ok_or(Error::MissingArg("command to run"))?;
    Ok(Cli { dir, cmd })
  }
}

#[cfg(not(unix))]
fn main() {
  panic!("Only Unix systems are supported")
}
