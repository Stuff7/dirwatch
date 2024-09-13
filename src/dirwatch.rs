use crate::channels::{Receiver, Sender};
use crate::error::Error;
use crate::server::Event;
use libc::{inotify_add_watch, inotify_event, inotify_init1, read, EAGAIN, EWOULDBLOCK, IN_CLOSE_WRITE};
use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::fs;
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

  let mut wd_to_path = HashMap::new();
  fn add_watch_recursive(fd: i32, path: &Path, wd_to_path: &mut HashMap<i32, PascalString>, mut mask: u32) -> Result<(), Error> {
    mask |= IN_CREATE;
    let path_c = CString::new(path.to_str().unwrap().as_bytes())?;
    let wd = unsafe { inotify_add_watch(fd, path_c.as_ptr(), mask) };
    if wd < 0 {
      return Err(Error::InotifyWatch(io::Error::last_os_error()));
    }

    wd_to_path.insert(wd, PascalString::new(path.to_str().ok_or(Error::NonUtf8)?.as_bytes()));

    for entry in fs::read_dir(path)? {
      let entry = entry?;
      let path = entry.path();
      if path.is_dir() {
        add_watch_recursive(fd, &path, wd_to_path, mask)?;
      }
    }

    Ok(())
  }

  add_watch_recursive(fd, path, &mut wd_to_path, mask)?;

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

      let event_name = extract_event_name(event, &buffer)?;
      if event.mask & IN_CREATE != 0 {
        let new_path = path.join(event_name);

        if new_path.is_dir() {
          add_watch_recursive(fd, &new_path, &mut wd_to_path, mask)?;
        }
      }

      if event.mask & mask != 0 {
        let mut dir = *wd_to_path.get(&event.wd).expect("event wd not mapped");
        dir.extend(b"/").extend(event_name.as_bytes());
        log_event(event, dir.as_str());
        tx.send(Event::FileChange(dir));
      }

      i += EVENT_SIZE + event.len as usize;
    }
  }

  server_events.join().unwrap();

  Ok(())
}

#[derive(Debug, Clone, Copy)]
pub struct PascalString {
  len: u8,
  pub buf: [u8; 128],
}

impl PascalString {
  fn new(data: &[u8]) -> Self {
    let mut buf = [0; 128];
    buf[..data.len()].copy_from_slice(data);
    Self { len: data.len() as u8, buf }
  }

  fn extend(&mut self, data: &[u8]) -> &mut Self {
    let len = self.len as usize;
    self.len += data.len() as u8;
    self.buf[len..self.len as usize].copy_from_slice(data);
    self
  }

  pub fn as_bytes(&self) -> &[u8] {
    &self.buf[..self.len as usize]
  }

  fn as_str(&self) -> &str {
    unsafe { std::str::from_utf8_unchecked(&self.buf) }
  }
}

fn log_event(event: &inotify_event, name: &str) {
  let mask = event.mask;
  let wd = event.wd;

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

  println!("\x1b[38;5;123mFile Change:\x1b[0m WD: {}, Mask: {}, Name: {}", wd, mask_str.trim(), name);
}

fn extract_event_name<'a>(event: &inotify_event, buffer: &'a [u8]) -> Result<&'a str, Error> {
  let name_len = event.len as usize;
  if name_len > 0 {
    let name_cstr = unsafe { CStr::from_ptr(buffer.as_ptr().add(EVENT_SIZE) as *const _) };
    Ok(name_cstr.to_str()?)
  }
  else {
    Ok("")
  }
}
