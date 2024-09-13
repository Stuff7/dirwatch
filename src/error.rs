use std::{
  ffi::NulError,
  fmt::{Debug, Display},
  io,
  str::Utf8Error,
};

pub enum Error {
  Io(io::Error),
  InotifyInit(io::Error),
  InotifyWatch(io::Error),
  InotifyRead(io::Error),
  Utf8(Utf8Error),
  NonUtf8,
  Nul(NulError),
}

impl From<io::Error> for Error {
  fn from(value: io::Error) -> Self {
    Self::Io(value)
  }
}

impl From<Utf8Error> for Error {
  fn from(value: Utf8Error) -> Self {
    Self::Utf8(value)
  }
}

impl From<NulError> for Error {
  fn from(value: NulError) -> Self {
    Self::Nul(value)
  }
}

impl Display for Error {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::Io(err) => write!(f, "{err}"),
      Self::InotifyInit(err) => write!(f, "Failed to initialize inotify: {err}"),
      Self::InotifyWatch(err) => write!(f, "Failed to add inotify watch: {err}"),
      Self::InotifyRead(err) => write!(f, "Failed to read inotify event: {err}"),
      Self::Utf8(err) => write!(f, "{err}"),
      Self::NonUtf8 => write!(f, "Only utf8 file names are supported"),
      Self::Nul(err) => write!(f, "{err}"),
    }
  }
}

impl Debug for Error {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "{self}")
  }
}
