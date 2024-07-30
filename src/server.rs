use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::{str, thread};

use crate::http::HttpMethod;
use crate::{
  dirwatch,
  error::Error,
  http::{HttpRequest, HttpResponse},
  Cli,
};

fn handle_http(mut stream: TcpStream, dir_serve: &Path, dir_watch: &Path) -> Result<(), Error> {
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

    let mut response = HttpResponse::not_found();

    if matches!(req.method, HttpMethod::Get) {
      response.status = 200;
      match &*req.path {
        "/" => response.read_file_to_end(dir_serve.join("index.html"))?,
        "/sse" => return handle_sse(stream, req, dir_watch),
        _ => response.read_file_to_end(dir_serve.join(&req.path[1..]))?,
      }
    }

    response.write_to(&mut stream)?;

    if response.status != 200 || !req.headers.get("connection").is_some_and(|v| v == "keep-alive") {
      break;
    }
  }

  println!("[\x1b[93m{}\x1b[0m] \x1b[36mHttpRequest Done\x1b[0m", addr);

  Ok(())
}

fn handle_sse(mut stream: TcpStream, req: HttpRequest, dir: &Path) -> Result<(), Error> {
  println!("[\x1b[93m{}\x1b[0m] \x1b[36mSSE Connected\x1b[0m", req.peer_addr);

  let mut response = HttpResponse::from_status(200);
  response.headers.insert("content-type".into(), "text/event-stream".into());
  response.headers.insert("cache-control".into(), "no-cache".into());
  response.headers.insert("connection".into(), "keep-alive".into());
  response.write_to(&mut stream)?;

  dirwatch::watch_dir(dir, dirwatch::IN_MODIFY, &mut stream)?;
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

  for stream in listener.incoming() {
    match stream {
      Ok(stream) => {
        let peer_addr = stream.peer_addr()?;
        let dir_serve = cli.dir_serve.clone();
        let dir_watch = cli.dir_watch.clone();

        thread::spawn(move || {
          if let Err(e) = handle_http(stream, &dir_serve, &dir_watch) {
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
