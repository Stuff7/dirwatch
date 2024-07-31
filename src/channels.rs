use std::{
  sync::{Arc, Condvar, Mutex, MutexGuard, TryLockError},
  time::Duration,
};

/// # Usage
/// ```rust
/// let (tx, rx) = broadcast(0, Duration::from_millis(10));
///
/// let sender = {
///   let mut tx = tx.clone();
///   thread::spawn(move || {
///     for i in 0..5 {
///       tx.send(i);
///       thread::sleep(Duration::from_millis(20));
///     }
///   })
/// };
///
/// let receivers: [thread::JoinHandle<()>; 3] = std::array::from_fn(|i| {
///   let rx = rx.clone();
///   thread::spawn(move || {
///     let mut n = rx.lock();
///     let mut waiting = false;
///     loop {
///       if waiting {
///         println!("Recv #{i}: Waiting...");
///       }
///       else {
///         println!("Recv #{i}: n={n}");
///       }
///       if *n == 4 {
///         return;
///       }
///       (n, waiting) = rx.recv(n);
///     }
///   })
/// });
/// ```
pub fn broadcast<T>(data: T, timeout: Duration) -> (BroadcastSender<T>, BroadcastReceiver<T>) {
  let tx = BroadcastSender {
    pair: Arc::new((Mutex::new(data), Condvar::new())),
  };

  let rx = BroadcastReceiver {
    pair: tx.pair.clone(),
    timeout,
  };

  (tx, rx)
}

pub struct BroadcastSender<T> {
  pair: Arc<(Mutex<T>, Condvar)>,
}

impl<T> BroadcastSender<T> {
  pub fn send(&mut self, data: T) {
    let (lock, cvar) = &*self.pair;

    let mut n = match lock.try_lock() {
      Ok(n) => n,
      Err(e) => {
        if matches!(e, TryLockError::WouldBlock) {
          return;
        }
        panic!("Poisoned lock {e}");
      }
    };

    *n = data;
    cvar.notify_all();
  }
}

impl<T> Clone for BroadcastSender<T> {
  fn clone(&self) -> Self {
    Self { pair: self.pair.clone() }
  }
}

pub struct BroadcastReceiver<T> {
  pair: Arc<(Mutex<T>, Condvar)>,
  timeout: Duration,
}

impl<T> BroadcastReceiver<T> {
  pub fn lock(&self) -> MutexGuard<'_, T> {
    let (g, _) = &*self.pair;
    g.lock().unwrap()
  }

  pub fn recv<'a>(&'a self, guard: MutexGuard<'a, T>) -> (MutexGuard<'a, T>, bool) {
    let (_, cvar) = &*self.pair;
    let t = cvar.wait_timeout(guard, self.timeout).unwrap();
    (t.0, t.1.timed_out())
  }
}

impl<T> Clone for BroadcastReceiver<T> {
  fn clone(&self) -> Self {
    Self {
      pair: self.pair.clone(),
      timeout: self.timeout,
    }
  }
}
