mod dirwatch;
mod error;
mod http;
mod server;

use std::{env, path::PathBuf, str::FromStr};

use error::Error;

#[cfg(unix)]
fn main() -> Result<(), Error> {
  let args = Cli::parse()?;
  server::run_server(&args)
}

pub struct Cli {
  pub dir_watch: PathBuf,
  pub dir_serve: PathBuf,
  pub cmd: String,
  pub port: String,
}

impl Cli {
  pub fn parse() -> Result<Self, Error> {
    // Usage dirwatch -watch <dir> -serve <dir> -run <cmd> -port <port>
    Ok(Self {
      dir_watch: Self::find_arg("-watch").unwrap_or_else(|| ".".to_string()).into(),
      dir_serve: Self::find_arg("-serve").unwrap_or_else(|| ".".to_string()).into(),
      port: Self::find_arg("-port").unwrap_or_else(|| "8080".to_string()),
      cmd: Self::find_arg("-run").unwrap_or_default(),
    })
  }

  fn find_arg<F: FromStr>(arg_name: &str) -> Option<F> {
    let mut args = env::args();
    args
      .position(|arg| arg == arg_name)
      .and_then(|_| args.next())
      .and_then(|n| n.parse::<F>().ok())
  }
}

#[cfg(not(unix))]
fn main() {
  panic!("Only Unix systems are supported")
}
