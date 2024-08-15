use std::cell::Cell;
use std::ops::Deref;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use std::{array, thread};

#[derive(Debug)]
struct Slot<T> {
  version: AtomicUsize,
  message: RwLock<T>,
}

#[derive(Debug)]
pub struct RingBuffer<T> {
  buffer: Arc<[Slot<T>]>,
  write_index: Arc<AtomicUsize>,
  version: Arc<AtomicUsize>,
}

impl<T> Clone for RingBuffer<T> {
  fn clone(&self) -> Self {
    Self {
      buffer: self.buffer.clone(),
      write_index: self.write_index.clone(),
      version: self.version.clone(),
    }
  }
}

impl<T: Copy> RingBuffer<T> {
  pub fn new<const BUF_SIZE: usize>(value: T) -> Self {
    Self {
      buffer: Arc::new(array::from_fn::<_, BUF_SIZE, _>(|_| Slot {
        version: AtomicUsize::new(0),
        message: RwLock::new(value),
      })),
      write_index: Arc::new(AtomicUsize::new(0)),
      version: Arc::new(AtomicUsize::new(1)),
    }
  }

  pub fn channel<const BUF_SIZE: usize>(value: T) -> (Sender<T>, Receiver<T>) {
    let rx = Receiver {
      state: RingBuffer::new::<BUF_SIZE>(value),
      last_version: Cell::new(0),
    };

    (Sender(rx.state.clone()), rx)
  }

  pub fn send(&self, new_message: T) {
    let index = self.write_index.fetch_add(1, Ordering::AcqRel) % self.buffer.len();
    let version = self.version.fetch_add(1, Ordering::AcqRel);

    let slot = &self.buffer[index];
    *slot.message.write().unwrap() = new_message;
    slot.version.store(version, Ordering::Release);
  }
}

#[derive(Debug)]
pub struct Receiver<T> {
  state: RingBuffer<T>,
  last_version: Cell<usize>,
}

impl<T> Clone for Receiver<T> {
  fn clone(&self) -> Self {
    Self {
      state: self.state.clone(),
      last_version: Cell::new(0),
    }
  }
}

impl<T> From<&Sender<T>> for Receiver<T> {
  fn from(value: &Sender<T>) -> Self {
    Self {
      state: value.0.clone(),
      last_version: Cell::new(0),
    }
  }
}

impl<T: Copy> Receiver<T> {
  pub fn recv_some(&self) -> Option<T> {
    let current_version = self.last_version.get() + 1;
    for slot in self.state.buffer.iter() {
      if slot.version.load(Ordering::Acquire) == current_version {
        self.last_version.replace(current_version);
        return Some(*slot.message.read().unwrap());
      }
    }

    let next_closest = self
      .state
      .buffer
      .iter()
      .filter(|slot| slot.version.load(Ordering::Acquire) > current_version)
      .min_by_key(|slot| slot.version.load(Ordering::Acquire));

    if let Some(slot) = next_closest {
      self.last_version.replace(slot.version.load(Ordering::Acquire));
      return Some(*slot.message.read().unwrap());
    }

    None
  }

  pub fn recv(&self) -> T {
    loop {
      if let Some(message) = self.recv_some() {
        return message;
      }
      thread::sleep(Duration::from_millis(1));
    }
  }
}

impl<T> Deref for Receiver<T> {
  type Target = RingBuffer<T>;

  fn deref(&self) -> &Self::Target {
    &self.state
  }
}

#[derive(Debug)]
pub struct Sender<T>(RingBuffer<T>);

impl<T> Clone for Sender<T> {
  fn clone(&self) -> Self {
    Self(self.0.clone())
  }
}

impl<T> Deref for Sender<T> {
  type Target = RingBuffer<T>;

  fn deref(&self) -> &Self::Target {
    &self.0
  }
}

impl<T> From<&Receiver<T>> for Sender<T> {
  fn from(value: &Receiver<T>) -> Self {
    Self(value.state.clone())
  }
}
