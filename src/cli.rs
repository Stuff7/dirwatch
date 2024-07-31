use crate::error::Error;
use std::process::{Command, ExitStatus, Stdio};
use std::{env, path::PathBuf, str::FromStr};

pub struct Cli {
  pub dir_watch: PathBuf,
  pub dir_serve: PathBuf,
  pub cmd: Cmd,
  pub port: String,
}

impl Cli {
  pub fn parse() -> Result<Self, Error> {
    // Usage dirwatch -watch <dir> -serve <dir> -run <cmd> -port <port>
    Ok(Self {
      dir_watch: find_arg("-watch").unwrap_or_else(|| ".".to_string()).into(),
      dir_serve: find_arg("-serve").unwrap_or_else(|| ".".to_string()).into(),
      port: find_arg("-port").unwrap_or_else(|| "8080".to_string()),
      cmd: find_arg("-run").unwrap_or_else(|| Cmd(None)),
    })
  }
}

fn find_arg<F: FromStr>(arg_name: &str) -> Option<F> {
  let mut args = env::args();
  args
    .position(|arg| arg == arg_name)
    .and_then(|_| args.next())
    .and_then(|n| n.parse::<F>().ok())
}

pub struct Cmd(Option<Command>);

impl Cmd {
  pub fn run_wait(&mut self) -> Result<ExitStatus, Error> {
    if let Some(ref mut cmd) = self.0 {
      return Ok(cmd.stdout(Stdio::null()).status()?);
    }

    Ok(ExitStatus::default())
  }
}

impl FromStr for Cmd {
  type Err = Error;
  fn from_str(cmd: &str) -> Result<Self, Self::Err> {
    let mut cmd_iter = cmd.split_whitespace();
    let Some(exe) = cmd_iter.next()
    else {
      return Ok(Self(None));
    };
    let mut cmd = Command::new(exe);
    cmd.args(cmd_iter);
    Ok(Self(Some(cmd)))
  }
}
