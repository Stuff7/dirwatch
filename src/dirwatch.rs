use crate::channels::{Receiver, Sender};
use crate::error::Error;
use crate::server::Event;
use libc::{inotify_add_watch, inotify_event, inotify_init1, read, EAGAIN, EWOULDBLOCK, IN_CLOSE_WRITE};
use std::ffi::{CStr, CString};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use std::{io, thread};

pub use libc::{IN_CREATE, IN_DELETE, IN_DELETE_SELF, IN_IGNORED, IN_MODIFY};

const EVENT_SIZE: usize = std::mem::size_of::<inotify_event>();
const BUF_LEN: usize = 1024 * (EVENT_SIZE + 16);

pub fn watch_dir(path: &Path, mask: u32, tx: Sender<Event>) -> Result<(), Error> {
  let fd = unsafe { inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
  if fd < 0 {
    return Err(Error::InotifyInit(io::Error::last_os_error()));
  }

  let path_c = CString::new(path.to_str().unwrap().as_bytes())?;
  let wd = unsafe { inotify_add_watch(fd, path_c.as_ptr(), mask) };
  if wd < 0 {
    return Err(Error::InotifyWatch(io::Error::last_os_error()));
  }

  let stop = Arc::new(AtomicBool::new(false));
  let server_events = {
    let rx = Receiver::from(&tx);
    let stop = stop.clone();

    thread::spawn(move || loop {
      let event = rx.recv();
      if matches!(event, Event::Quit) {
        stop.store(true, Ordering::Release);
        break;
      }
    })
  };

  let mut buffer = [0; BUF_LEN];

  loop {
    let length = unsafe { read(fd, buffer.as_mut_ptr() as *mut libc::c_void, buffer.len()) };
    if length < 0 {
      let err = io::Error::last_os_error();
      let err_os = err.raw_os_error();

      if err_os == Some(EAGAIN) || err_os == Some(EWOULDBLOCK) {
        if stop.load(Ordering::Acquire) {
          break;
        }

        thread::sleep(Duration::from_millis(10));
        continue;
      }

      return Err(Error::InotifyRead(io::Error::last_os_error()));
    }

    let mut i = 0;
    while i < length as usize {
      let event = unsafe { &*(buffer.as_ptr().add(i) as *const inotify_event) };
      log_event(event, &buffer);
      tx.send(Event::FileChange);
      i += EVENT_SIZE + event.len as usize;
    }
  }

  server_events.join().unwrap();

  Ok(())
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

  println!("\x1b[38;5;123mFile Change:\x1b[0m WD: {}, Mask: {}, Name: {}", wd, mask_str.trim(), name);
}
