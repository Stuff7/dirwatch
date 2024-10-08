use crate::error::Error;
use std::process::{Command, ExitStatus, Stdio};
use std::{env, io::Write, path::PathBuf, str::FromStr};

pub struct Cli {
  pub dir_watch: PathBuf,
  pub dir_serve: PathBuf,
  pub cmd: String,
  pub port: String,
}

impl Cli {
  pub const USAGE: &str = "Usage: dirwatch -watch <dir> -serve <dir> -run <cmd> -port <port>";

  pub fn parse() -> Result<Self, Error> {
    Ok(Self {
      dir_watch: find_arg("-watch").unwrap_or_else(|| ".".to_string()).into(),
      dir_serve: find_arg("-serve").unwrap_or_else(|| ".".to_string()).into(),
      port: find_arg("-port").unwrap_or_else(|| "8080".to_string()),
      cmd: find_arg("-run").unwrap_or_else(|| "".to_string()),
    })
  }
}

pub fn find_flag(name: &str) -> bool {
  env::args().any(|a| a == name)
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
  pub fn new(cmd: &str) -> Self {
    let mut cmd_iter = cmd.split_whitespace();
    let Some(exe) = cmd_iter.next()
    else {
      return Self(None);
    };

    let mut cmd = Command::new(exe);
    cmd.args(cmd_iter).stdin(Stdio::piped());
    Self(Some(cmd))
  }

  pub fn run_wait(&mut self, data: &[u8]) -> Result<ExitStatus, Error> {
    if let Some(ref mut cmd) = self.0 {
      let mut p = cmd.stdout(Stdio::null()).spawn()?;
      if let Some(stdin) = p.stdin.as_mut() {
        stdin.write_all(data)?;
      }
      return Ok(p.wait()?);
    }

    Ok(ExitStatus::default())
  }
}
