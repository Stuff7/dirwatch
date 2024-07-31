use crate::channels::BroadcastSender;
use crate::error::Error;
use libc::{inotify_add_watch, inotify_event, inotify_init1, read, EAGAIN, EWOULDBLOCK, IN_CLOSE_WRITE};
use std::ffi::{CStr, CString};
use std::path::Path;
use std::{io, thread, time};

pub use libc::{IN_CREATE, IN_DELETE, IN_DELETE_SELF, IN_IGNORED, IN_MODIFY};

const EVENT_SIZE: usize = std::mem::size_of::<inotify_event>();
const BUF_LEN: usize = 1024 * (EVENT_SIZE + 16);

pub fn watch_dir(path: &Path, mask: u32, mut tx: BroadcastSender<bool>) -> Result<(), Error> {
  let fd = unsafe { inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
  if fd < 0 {
    let err = io::Error::last_os_error();
    return Err(io::Error::new(err.kind(), format!("Failed to initialize inotify: {}", err)).into());
  }

  let path_c = CString::new(path.to_str().unwrap().as_bytes())?;
  let wd = unsafe { inotify_add_watch(fd, path_c.as_ptr(), mask) };
  if wd < 0 {
    let err = io::Error::last_os_error();
    return Err(io::Error::new(err.kind(), format!("Failed to add inotify watch: {}", err)).into());
  }

  let mut buffer = [0; BUF_LEN];

  loop {
    let length = unsafe { read(fd, buffer.as_mut_ptr() as *mut libc::c_void, buffer.len()) };
    if length < 0 {
      let err = io::Error::last_os_error();
      if err.raw_os_error() == Some(EAGAIN) || err.raw_os_error() == Some(EWOULDBLOCK) {
        thread::sleep(time::Duration::from_millis(10));
        continue;
      }
      else {
        return Err(io::Error::new(err.kind(), format!("Failed to read inotify events: {}", err)).into());
      }
    }

    let mut i = 0;
    while i < length as usize {
      let event = unsafe { &*(buffer.as_ptr().add(i) as *const inotify_event) };
      tx.send(true);
      log_event(event, &buffer);
      i += EVENT_SIZE + event.len as usize;
    }
  }
}

fn log_event(event: &inotify_event, buffer: &[u8]) {
  let mask = event.mask;
  let wd = event.wd;
  let name_len = event.len as usize;

  let mut mask_str = String::new();
  if mask & IN_MODIFY != 0 {
    mask_str.push_str("IN_MODIFY ");
  }
  if mask & IN_CREATE != 0 {
    mask_str.push_str("IN_CREATE ");
  }
  if mask & IN_DELETE != 0 {
    mask_str.push_str("IN_DELETE ");
  }
  if mask & IN_CLOSE_WRITE != 0 {
    mask_str.push_str("IN_CLOSE_WRITE ");
  }
  if mask & IN_DELETE_SELF != 0 {
    mask_str.push_str("IN_DELETE_SELF ");
  }
  if mask & IN_IGNORED != 0 {
    mask_str.push_str("IN_IGNORED ");
  }

  let name = if name_len > 0 {
    let name_cstr = unsafe { CStr::from_ptr(buffer.as_ptr().add(EVENT_SIZE) as *const _) };
    name_cstr.to_str().unwrap_or("Invalid UTF-8")
  }
  else {
    ""
  };

  println!("WD: {}, Mask: {}, Name: {}", wd, mask_str.trim(), name);
}
