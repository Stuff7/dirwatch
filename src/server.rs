use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::time::Duration;
use std::{str, thread};

use crate::channels::{self, BroadcastReceiver};
use crate::http::HttpMethod;
use crate::{
  dirwatch,
  error::Error,
  http::{HttpRequest, HttpResponse},
  Cli,
};

fn inject_hr(res: &mut HttpResponse, path: &Path) -> Result<(), Error> {
  res.set_file(path)?;
  const HEAD: &[u8] = b"<head>";

  let Some(idx) = res.contents.windows(HEAD.len()).position(|s| s == HEAD).map(|i| i + HEAD.len())
  else {
    return Ok(());
  };

  res.contents.splice(idx..idx, include_str!("hot_reload.html").as_bytes().iter().copied());

  Ok(())
}

fn handle_http(mut stream: TcpStream, dir_serve: &Path, rx: BroadcastReceiver<bool>) -> Result<(), Error> {
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
      match &*req.path {
        "/" => inject_hr(&mut res, &dir_serve.join("index.html"))?,
        "/sse" => return handle_sse(stream, req, rx),
        _ => res.set_file(dir_serve.join(&req.path[1..]))?,
      }
    }
    else {
      res.set_404();
    }

    res.write_to(&mut stream)?;

    if res.status != 200 || !req.headers.get("connection").is_some_and(|v| v == "keep-alive") {
      break;
    }
  }

  println!("[\x1b[93m{}\x1b[0m] \x1b[33mHttp Disconnected\x1b[0m", addr);

  Ok(())
}

fn handle_sse(mut stream: TcpStream, req: HttpRequest, rx: BroadcastReceiver<bool>) -> Result<(), Error> {
  println!("[\x1b[93m{}\x1b[0m] \x1b[36mSSE Connected\x1b[0m", req.peer_addr);

  let mut response = HttpResponse::new();
  response
    .set_header("content-type", "text/event-stream")
    .set_header("cache-control", "no-cache")
    .set_header("connection", "keep-alive");

  response.write_to(&mut stream)?;

  let mut guard = rx.lock();
  let mut waiting;
  stream.set_nonblocking(true)?;

  loop {
    if is_stream_closed(&mut stream) {
      break;
    }

    (guard, waiting) = rx.recv(guard);
    if !waiting {
      println!("[\x1b[93m{}\x1b[0m] \x1b[32mFile Changed\x1b[0m", req.peer_addr);
      send_sse_message(&mut stream, "File changed")?;
    }
  }

  println!("[\x1b[93m{}\x1b[0m] \x1b[33mSSE Disconnected\x1b[0m", req.peer_addr);
  Ok(())
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

  let (tx, rx) = channels::broadcast(false, Duration::from_millis(0));

  {
    let dir_watch = cli.dir_watch.clone();
    let tx = tx.clone();
    thread::spawn(move || {
      if let Err(e) = dirwatch::watch_dir(&dir_watch, dirwatch::IN_MODIFY, tx) {
        eprintln!("Error watching directory: {e}");
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

pub fn send_sse_message(stream: &mut TcpStream, message: &str) -> Result<(), Error> {
  let response = format!("data: {}\n\n", message);
  stream.write_all(response.as_bytes())?;
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
