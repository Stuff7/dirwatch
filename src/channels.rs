use std::sync::{
  atomic::{AtomicUsize, Ordering},
  Arc, Condvar, Mutex, MutexGuard,
};

/// # Example
///
/// ```rust
/// #[derive(Debug)]
/// enum Signal {
///   Hello,
///   Number(usize),
///   Goodbye,
/// }
///
/// let (tx, rx) = channels::broadcast(Signal::Hello);
///
/// let rx_handles: [JoinHandle<()>; 3] = std::array::from_fn(|i| {
///   let rx = rx.clone();
///
///   thread::spawn(move || {
///     let mut v = rx.lock();
///
///     loop {
///       v = rx.recv(v);
///
///       if matches!(*v, Signal::Goodbye) {
///         break;
///       }
///     }
///   })
/// });
///
/// let tx_handles: [JoinHandle<()>; 3] = std::array::from_fn(|n| {
///   let tx = tx.clone();
///
///   thread::spawn(move || {
///     let n = n * 5;
///
///     for i in n..n + 5 {
///       // Since we're sending an event on every cpu cycle we need to give time for the receivers to start listening again.
///       thread::sleep(Duration::from_millis(10));
///       tx.send(Signal::Number(i));
///     }
///   })
/// });
///
/// for txh in tx_handles {
///   txh.join().unwrap();
/// }
///
/// tx.send(Signal::Goodbye);
///
/// for rxh in rx_handles {
///   rxh.join().unwrap();
/// }
/// ```
pub fn broadcast<T>(v: T) -> (Sender<T>, Receiver<T>) {
  let rx = Receiver {
    state: State {
      tx_pair: Arc::new((Mutex::new(v), Condvar::new())),
      rx_pair: Arc::new((Mutex::new(0), Condvar::new())),
      receiver_count: Arc::new(AtomicUsize::new(0)),
    },
  };

  (Sender { state: rx.state.clone() }, rx)
}

#[derive(Debug, Default)]
pub struct State<T> {
  tx_pair: Arc<(Mutex<T>, Condvar)>,
  rx_pair: Arc<(Mutex<usize>, Condvar)>,
  receiver_count: Arc<AtomicUsize>,
}

impl<T> Clone for State<T> {
  fn clone(&self) -> Self {
    Self {
      tx_pair: self.tx_pair.clone(),
      rx_pair: self.rx_pair.clone(),
      receiver_count: self.receiver_count.clone(),
    }
  }
}

#[derive(Debug, Default)]
pub struct Receiver<T> {
  state: State<T>,
}

impl<T> Clone for Receiver<T> {
  fn clone(&self) -> Self {
    Self { state: self.state.clone() }
  }
}

impl<T> Receiver<T> {
  pub fn lock(&self) -> MutexGuard<T> {
    let (tx_v, _) = &*self.state.tx_pair;
    tx_v.lock().unwrap()
  }

  pub fn recv<'a>(&'a self, guard: MutexGuard<'a, T>) -> MutexGuard<'a, T> {
    let (_, tx_cv) = &*self.state.tx_pair;
    let (rx_v, rx_cv) = &*self.state.rx_pair;
    self.state.receiver_count.fetch_add(1, Ordering::Relaxed);
    let v = tx_cv.wait(guard).unwrap();
    self.state.receiver_count.fetch_sub(1, Ordering::Relaxed);

    let mut recvrs = rx_v.lock().unwrap();
    if *recvrs > 0 {
      *recvrs -= 1;
    }
    let recvrs = *recvrs;
    if recvrs == 0 {
      rx_cv.notify_one();
    }

    v
  }
}

#[derive(Debug, Default)]
pub struct Sender<T> {
  state: State<T>,
}

impl<T> Clone for Sender<T> {
  fn clone(&self) -> Self {
    Self { state: self.state.clone() }
  }
}

impl<T> Sender<T> {
  pub fn send(&self, v: T) {
    let (tx_v, tx_cv) = &*self.state.tx_pair;
    let (rx_v, rx_cv) = &*self.state.rx_pair;

    let mut recvrs = rx_v.lock().unwrap();
    while *recvrs != 0 {
      recvrs = rx_cv.wait(recvrs).unwrap();
    }
    *recvrs = self.state.receiver_count.load(Ordering::Acquire);
    drop(recvrs);

    *tx_v.lock().unwrap() = v;
    tx_cv.notify_all();
  }

  /// # Transceivers
  ///
  /// Transceivers are useful when you need to both receive and send in the same thread.
  ///
  /// # Example
  ///
  /// ```rust
  ///  let (tx, rx) = channels::broadcast(Event::Start);
  ///
  ///  let rx_handles: [JoinHandle<()>; 3] = std::array::from_fn(|i| {
  ///    let rx = rx.clone();
  ///
  ///    thread::spawn(move || {
  ///      let mut v = rx.lock();
  ///
  ///      loop {
  ///        v = rx.recv(v);
  ///        match *v {
  ///          Event::CmdFinished => println!("[RX][recvr{i:02}]: Command finished {:?}", *v),
  ///          Event::Goodbye => break,
  ///          _ => println!("[RX][recvr{i:02}]: Received {:?}", *v),
  ///        }
  ///      }
  ///    })
  ///  });
  ///
  ///  let runner = {
  ///    let tx = tx.transceiver();
  ///
  ///    thread::spawn(move || {
  ///      let mut event = tx.lock();
  ///
  ///      loop {
  ///        event = tx.recv(event);
  ///
  ///        match *event {
  ///          Event::FileChanged => event = tx.send(event, Event::CmdFinished),
  ///          Event::Goodbye => break,
  ///          _ => println!("[RX][runner ]: Received {:?}", *event),
  ///        }
  ///      }
  ///    })
  ///  };
  ///
  ///  let watcher = {
  ///    let tx = tx.clone();
  ///
  ///    thread::spawn(move || {
  ///      for _ in 0..5 {
  ///        // Since we're sending an event on every cpu cycle we need to give time for the receivers to start listening again.
  ///        thread::sleep(Duration::from_millis(10));
  ///        tx.send(Event::FileChanged);
  ///      }
  ///    })
  ///  };
  ///
  ///  watcher.join().unwrap();
  ///
  ///  thread::sleep(Duration::from_millis(10));
  ///  tx.send(Event::Goodbye);
  ///
  ///  for rxh in rx_handles {
  ///    rxh.join().unwrap();
  ///  }
  ///
  ///  runner.join().unwrap();
  /// ```
  pub fn transceiver(&self) -> Transceiver<T> {
    Transceiver {
      rx: Receiver { state: self.state.clone() },
      tx: self.clone(),
    }
  }
}

#[derive(Debug, Default)]
pub struct Transceiver<T> {
  rx: Receiver<T>,
  tx: Sender<T>,
}

impl<T> Transceiver<T> {
  pub fn lock(&self) -> MutexGuard<T> {
    self.rx.lock()
  }

  pub fn recv<'a>(&'a self, guard: MutexGuard<'a, T>) -> MutexGuard<'a, T> {
    self.rx.recv(guard)
  }

  pub fn send<'a>(&'a self, guard: MutexGuard<'a, T>, v: T) -> MutexGuard<'a, T> {
    drop(guard);
    self.tx.send(v);
    self.lock()
  }
}
