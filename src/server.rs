use crate::channels::{self, Receiver, Sender, Transceiver};
use crate::cli::Cmd;
use crate::http::HttpMethod;
use crate::{
  dirwatch,
  error::Error,
  http::{HttpRequest, HttpResponse},
  Cli,
};
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::thread;

#[derive(Debug)]
pub enum Event {
  Start,
  FileChange,
  CmdFinished,
}

fn inject_hr(req: &HttpRequest, res: &mut HttpResponse, path: &Path) -> Result<(), Error> {
  res.set_file(path, req)?;
  const HEAD: &[u8] = b"<head>";

  let Some(idx) = res.contents.windows(HEAD.len()).position(|s| s == HEAD).map(|i| i + HEAD.len())
  else {
    return Ok(());
  };

  res.contents.splice(idx..idx, include_str!("hot_reload.html").as_bytes().iter().copied());

  Ok(())
}

fn handle_http(mut stream: TcpStream, dir_serve: &Path, rx: Receiver<Event>) -> Result<(), Error> {
  let mut addr;
  loop {
    let req = HttpRequest::try_from(&mut stream)?;
    addr = req.peer_addr;

    println!(
      "[\x1b[93m{}\x1b[0m] \x1b[34m{:?}\x1b[0m \x1b[33m{}\x1b[0m - \x1b[36m{}\x1b[0m | {}",
      req.peer_addr,
      req.method,
      req.path,
      req.headers.get("user-agent").unwrap_or(&"No user agent".into()),
      req.headers.get("connection").unwrap_or(&"".into()),
    );

    let mut res = HttpResponse::new();

    if matches!(req.method, HttpMethod::Get) {
      let path = dir_serve.join(&req.path[1..]);

      if path.is_dir() {
        if let Err(e) = inject_hr(&req, &mut res, &path.join("index.html")) {
          eprintln!("{e}");
          res.set_404();
        }
      }
      else {
        match &*req.path {
          "/sse" => return handle_sse(stream, req, rx),
          _ => res.set_file(dir_serve.join(&req.path[1..]), &req)?,
        }
      }
    }
    else {
      res.set_404();
    }

    res.write_to(&mut stream)?;

    if !req.headers.get("connection").is_some_and(|v| v == "keep-alive") {
      break;
    }
  }

  println!("[\x1b[93m{}\x1b[0m] \x1b[33mHttp Disconnected\x1b[0m", addr);

  Ok(())
}

fn handle_sse(mut stream: TcpStream, req: HttpRequest, rx: Receiver<Event>) -> Result<(), Error> {
  println!("[\x1b[93m{}\x1b[0m] \x1b[36mSSE Connected\x1b[0m", req.peer_addr);

  let mut response = HttpResponse::new();
  response
    .set_header("content-type", "text/event-stream")
    .set_header("cache-control", "no-cache")
    .set_header("connection", "keep-alive");

  response.write_to(&mut stream)?;

  let mut guard = rx.lock();
  stream.set_nonblocking(true)?;

  loop {
    if is_stream_closed(&mut stream) {
      break;
    }

    guard = rx.recv(guard);
    if matches!(&*guard, Event::CmdFinished) {
      println!("[\x1b[93m{}\x1b[0m] \x1b[32mFile Changed\x1b[0m", req.peer_addr);
      send_sse_message(&mut stream)?;
    }
  }

  println!("[\x1b[93m{}\x1b[0m] \x1b[33mSSE Disconnected\x1b[0m", req.peer_addr);
  Ok(())
}

fn run_cmd(mut cmd: Cmd, tx: Transceiver<Event>) -> Result<(), Error> {
  let mut event = tx.lock();

  loop {
    event = tx.recv(event);
    if matches!(&*event, Event::FileChange) {
      cmd.run_wait()?;
      event = tx.send(event, Event::CmdFinished);
    }
  }
}

pub fn run_server(cli: &Cli) -> Result<(), Error> {
  let listener = TcpListener::bind(format!("0.0.0.0:{}", cli.port))?;

  println!(
    "\x1b[1m\x1b[38;5;159mhttp://localhost:{}\n\
     \x1b[38;5;158mhttp://{}:{}\n\
     \n\
     Watching \x1b[93m{:?}\x1b[0m\n\
     Serving  \x1b[93m{:?}\x1b[0m\n\
     \n\x1b[38;5;225mCtrl-C\x1b[0m to exit\n\
    ",
    cli.port,
    listener.local_addr()?.ip(),
    cli.port,
    cli.dir_watch,
    cli.dir_serve,
  );

  let (tx, rx) = channels::broadcast(Event::Start);

  {
    let dir_watch = cli.dir_watch.clone();
    let tx = tx.clone();

    thread::spawn(move || {
      if let Err(e) = dirwatch::watch_dir(&dir_watch, dirwatch::IN_MODIFY, tx) {
        eprintln!("\x1b[38;5;210mError watching directory:\x1b[0m {e}");
      }
    });
  }

  {
    let cmd = Cmd::new(&cli.cmd);
    let tx = tx.transceiver();

    thread::spawn(move || {
      if let Err(e) = run_cmd(cmd, tx) {
        eprintln!("\x1b[38;5;210mCommand execution failed:\x1b[0m {e}");
      }
    });
  }

  for stream in listener.incoming() {
    match stream {
      Ok(stream) => {
        let peer_addr = stream.peer_addr()?;
        let dir_serve = cli.dir_serve.clone();
        let rx = rx.clone();

        thread::spawn(move || {
          if let Err(e) = handle_http(stream, &dir_serve, rx) {
            eprintln!("[{}] Error handling request: {}", peer_addr, e);
          }
        });
      }
      Err(e) => eprintln!("Connection failed: {}", e),
    }
  }

  Ok(())
}

pub fn send_sse_message(stream: &mut TcpStream) -> Result<(), Error> {
  stream.write_all(b"data: File changed\n\n")?;
  stream.flush()?;
  Ok(())
}

pub fn is_stream_closed(stream: &mut TcpStream) -> bool {
  let mut buffer = [0u8; 1];
  match stream.read(&mut buffer) {
    Ok(0) => true,
    Ok(_) => false,
    Err(ref e) => {
      if matches!(e.kind(), io::ErrorKind::WouldBlock) {
        return false;
      }

      matches!(
        io::Error::last_os_error().kind(),
        io::ErrorKind::BrokenPipe | io::ErrorKind::ConnectionReset | io::ErrorKind::ConnectionAborted | io::ErrorKind::UnexpectedEof
      )
    }
  }
}
