use std::fs::File;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::{str, thread};

use crate::{dirwatch, error::Error};

struct HttpRequest {
  method: Box<str>,
  path: Box<str>,
  peer_addr: SocketAddr,
  user_agent: Box<str>,
}

impl TryFrom<&TcpStream> for HttpRequest {
  type Error = Error;
  fn try_from(stream: &TcpStream) -> Result<Self, Self::Error> {
    let peer_addr = stream.peer_addr().unwrap_or_else(|_| "Unknown".parse().unwrap());
    let mut buffer = [0; 1024];

    stream.peek(&mut buffer)?;
    let request = str::from_utf8(&buffer)?;

    let mut lines = request.lines();
    let mut method = lines.next().unwrap_or("Unknown Method").split_whitespace();
    let (method, path) = (method.next().unwrap_or("UNKNOWN"), method.next().unwrap_or("Unknown Path"));
    let user_agent = lines.find(|line| line.starts_with("User-Agent:")).unwrap_or("User-Agent: Unknown");

    Ok(Self {
      method: method.into(),
      path: path.into(),
      peer_addr,
      user_agent: user_agent.into(),
    })
  }
}

fn handle_sse(mut stream: TcpStream, dir: String) -> Result<(), Error> {
  let req = HttpRequest::try_from(&stream)?;
  println!(
    "[\x1b[93m{}\x1b[0m] \x1b[34m{}\x1b[0m \x1b[33m{}\x1b[0m - \x1b[36m{}\x1b[0m SSE Connected",
    req.peer_addr, req.method, req.path, req.user_agent
  );

  let response = "HTTP/1.1 200 OK\r\n\
                  Content-Type: text/event-stream\r\n\
                  Cache-Control: no-cache\r\n\
                  Connection: keep-alive\r\n\r\n";
  stream.write_all(response.as_bytes())?;
  stream.flush()?;

  dirwatch::watch_dir(&dir, dirwatch::IN_MODIFY, &mut stream)?;
  println!(
    "[\x1b[93m{}\x1b[0m] \x1b[34m{}\x1b[0m \x1b[33m{}\x1b[0m - \x1b[36m{}\x1b[0m SSE Disconnected",
    req.peer_addr, req.method, req.path, req.user_agent
  );
  Ok(())
}

fn handle_file(mut stream: TcpStream, path: &str) -> Result<(), Error> {
  let path = Path::new(path);
  let mut file = File::open(path)?;
  let mut contents = Vec::new();
  file.read_to_end(&mut contents)?;

  let mime_type = match path.extension().and_then(|ext| ext.to_str()) {
    Some("html") => "text/html",
    Some("css") => "text/css",
    Some("js") => "application/javascript",
    _ => "application/octet-stream",
  };

  let response = format!(
    "HTTP/1.1 200 OK\r\n\
     Content-Length: {}\r\n\
     Content-Type: {}\r\n\r\n",
    contents.len(),
    mime_type
  );

  stream.write_all(response.as_bytes())?;
  stream.write_all(&contents)?;
  stream.flush()?;

  Ok(())
}

fn handle_http(mut stream: TcpStream) -> Result<(), Error> {
  let req = HttpRequest::try_from(&stream)?;
  println!(
    "[\x1b[93m{}\x1b[0m] HTTP \x1b[34m{}\x1b[0m \x1b[33m{}\x1b[0m - \x1b[36m{}\x1b[0m",
    req.peer_addr, req.method, req.path, req.user_agent
  );

  if &*req.method == "GET" {
    if &*req.path == "/" {
      let contents = include_str!("index.html");
      let response = format!(
        "HTTP/1.1 200 OK\r\n\
        Content-Length: {}\r\n\
        Content-Type: text/html\r\n\r\n\
        {}",
        contents.len(),
        contents
      );

      stream.write_all(response.as_bytes())?;
      stream.flush()?;
    }
    else {
      handle_file(stream, &req.path)?;
    }
  }
  else {
    let response = "HTTP/1.1 404 NOT FOUND\r\n\r\n";
    stream.write_all(response.as_bytes())?;
    stream.flush()?;
  }
  Ok(())
}

pub fn run_server(cli: &crate::Cli) -> Result<(), Error> {
  let listener = TcpListener::bind(format!("0.0.0.0:{}", cli.port))?;

  println!("\x1b[1m\x1b[38;5;159mhttp://localhost:{}", cli.port);
  println!("\x1b[38;5;158mhttp://{}:{}\n", listener.local_addr()?.ip(), cli.port);
  println!("Watching \x1b[93m{}\x1b[0m", cli.dir_watch);
  println!("Serving  \x1b[93m{}\x1b[0m", cli.dir_serve);
  println!("\n\x1b[38;5;225mCtrl-C\x1b[0m to exit\n");

  for stream in listener.incoming() {
    match stream {
      Ok(stream) => {
        let peer_addr = stream.peer_addr()?;
        let mut buf = [0; 8];
        let is_sse = stream.peek(&mut buf).unwrap_or(0) > 0 && &buf == b"GET /sse";

        if is_sse {
          let dir = cli.dir_watch.to_string();
          thread::spawn(move || {
            if let Err(e) = handle_sse(stream, dir) {
              eprintln!("Error handling SSE client {}: {}", peer_addr, e);
            }
          });
        }
        else {
          thread::spawn(move || {
            if let Err(e) = handle_http(stream) {
              eprintln!("Error handling HTTP client {}: {}", peer_addr, e);
            }
          });
        }
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
        io::ErrorKind::BrokenPipe | io::ErrorKind::ConnectionReset
      )
    }
  }
}
