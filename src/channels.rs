use std::{
  fmt::Debug,
  sync::{Arc, Condvar, Mutex, MutexGuard},
  thread,
  time::{Duration, Instant},
};

/// # Usage
/// ```rust
/// use std::thread;
/// let (tx, rx) = broadcast((0, 0));
///
/// let senders: [thread::JoinHandle<()>; 3] = std::array::from_fn(|n| {
///   let mut tx = tx.clone();
///   thread::spawn(move || {
///     for i in 0..5 {
///       tx.send((n, i));
///     }
///   })
/// });
///
/// let receivers: [thread::JoinHandle<()>; 3] = std::array::from_fn(|i| {
///   let rx = rx.clone();
///   let timeout = Duration::from_millis(1);
///   thread::spawn(move || {
///     let mut n = rx.lock();
///     let mut waiting = false;
///     let mut received = 0;
///     loop {
///       if !waiting {
///         received += 1;
///         println!("Recv #{i}: n={n:?}");
///       }
///
///       if received == 15 {
///         return;
///       }
///
///       (n, waiting) = rx.recv_with_timeout(n, timeout);
///     }
///   })
/// });
/// ```
pub fn broadcast<T>(data: T) -> (BroadcastSender<T>, BroadcastReceiver<T>) {
  let tx = BroadcastSender {
    pair: Arc::new((Mutex::new(data), Condvar::new())),
    timeout: Arc::new(Mutex::new(Instant::now() - SEND_COOLDOWN)),
  };

  let rx = BroadcastReceiver { pair: tx.pair.clone() };

  (tx, rx)
}

pub struct BroadcastSender<T> {
  pair: Arc<(Mutex<T>, Condvar)>,
  timeout: Arc<Mutex<Instant>>,
}

const SEND_COOLDOWN: Duration = Duration::from_millis(5);
impl<T: Debug> BroadcastSender<T> {
  pub fn send(&mut self, data: T) {
    let (lock, cvar) = &*self.pair;

    loop {
      let mut timeout = self.timeout.lock().unwrap();
      if timeout.elapsed() < SEND_COOLDOWN {
        thread::sleep(SEND_COOLDOWN - timeout.elapsed());
        continue;
      }

      *timeout = Instant::now();
      drop(timeout);

      *lock.lock().unwrap() = data;

      cvar.notify_all();
      return;
    }
  }
}

impl<T> Clone for BroadcastSender<T> {
  fn clone(&self) -> Self {
    Self {
      pair: self.pair.clone(),
      timeout: self.timeout.clone(),
    }
  }
}

pub struct BroadcastReceiver<T> {
  pair: Arc<(Mutex<T>, Condvar)>,
}

impl<T> BroadcastReceiver<T> {
  pub fn lock(&self) -> MutexGuard<'_, T> {
    let (g, _) = &*self.pair;
    g.lock().unwrap()
  }

  pub fn recv<'a>(&'a self, guard: MutexGuard<'a, T>) -> (MutexGuard<'a, T>, bool) {
    let (_, cvar) = &*self.pair;
    let t = cvar.wait_timeout(guard, Duration::ZERO).unwrap();
    (t.0, t.1.timed_out())
  }

  pub fn recv_with_timeout<'a>(&'a self, guard: MutexGuard<'a, T>, timeout: Duration) -> (MutexGuard<'a, T>, bool) {
    let (_, cvar) = &*self.pair;
    let t = cvar.wait_timeout(guard, timeout).unwrap();
    (t.0, t.1.timed_out())
  }
}

impl<T> Clone for BroadcastReceiver<T> {
  fn clone(&self) -> Self {
    Self { pair: self.pair.clone() }
  }
}
