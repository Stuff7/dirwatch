use crate::channels::{self, Receiver, Sender};
use crate::cli::Cmd;
use crate::http::{read_request_headers, HttpMethod};
use crate::{
  dirwatch,
  error::Error,
  http::{HttpRequest, HttpResponse},
  Cli,
};
use readln::{read_key, Key};
use std::io::Write;
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::thread;

#[derive(Debug, Clone, Copy)]
pub enum Event {
  Start,
  FileChange,
  CmdFinished,
  HttpRequest(SocketAddr),
  StreamClosed(SocketAddr),
  Quit,
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
  let stream_ip = stream.peer_addr()?;

  let is_sse = thread::scope(|s| -> Result<bool, Error> {
    use std::sync::{Arc, Mutex};

    let req = Arc::new(Mutex::new(HttpRequest::from_ip(stream_ip)));

    {
      let tx = Sender::from(&rx);
      let mut stream = stream.try_clone()?;
      let ip = stream_ip;
      let req = req.clone();

      s.spawn(move || -> Result<(), Error> {
        loop {
          let headers = read_request_headers(&mut stream)?;
          if !req.lock().unwrap().read_from_buffer(&headers)? {
            tx.send(Event::StreamClosed(ip));
            return Ok(());
          };

          tx.send(Event::HttpRequest(ip));
        }
      });
    }

    let mut is_sse = false;
    loop {
      let event = rx.recv();

      match event {
        Event::CmdFinished if is_sse => {
          println!("[\x1b[93m{}\x1b[0m] \x1b[32mFile Changed\x1b[0m", stream_ip);
          send_sse_message(&mut stream)?;
        }
        Event::HttpRequest(ip) if ip == stream_ip => {
          let req = req.lock().unwrap();
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
                "/sse" => {
                  res
                    .set_header("content-type", "text/event-stream")
                    .set_header("cache-control", "no-cache")
                    .set_header("connection", "keep-alive");

                  println!("[\x1b[93m{}\x1b[0m] \x1b[36mSSE Connected\x1b[0m", stream_ip);
                  is_sse = true;
                }
                _ => res.set_file(dir_serve.join(&req.path[1..]), &req)?,
              }
            }
          }
          else {
            res.set_404();
          }

          res.write_to(&mut stream)?;
        }
        Event::StreamClosed(ip) if ip == stream_ip => break,
        Event::Quit => {
          println!("[\x1b[93m{}\x1b[0m] \x1b[36mQUIT\x1b[0m", stream_ip);
          stream.shutdown(Shutdown::Write)?;
          break;
        }
        _ => (),
      }
    }

    Ok(is_sse)
  })?;

  if is_sse {
    println!("[\x1b[93m{}\x1b[0m] \x1b[33mSSE Disconnected\x1b[0m", stream_ip);
  }
  else {
    println!("[\x1b[93m{}\x1b[0m] \x1b[33mHttp Disconnected\x1b[0m", stream_ip);
  }

  Ok(())
}

fn run_cmd(mut cmd: Cmd, tx: Receiver<Event>) -> Result<(), Error> {
  loop {
    let event = tx.recv();
    match event {
      Event::FileChange => {
        cmd.run_wait()?;
        tx.send(Event::CmdFinished);
      }
      Event::Quit => break,
      _ => (),
    }
  }

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
     \n\x1b[38;5;225mPress Q\x1b[0m to exit\n\
    ",
    cli.port,
    listener.local_addr()?.ip(),
    cli.port,
    cli.dir_watch,
    cli.dir_serve,
  );

  let (tx, rx) = channels::RingBuffer::channel::<32>(Event::Start);

  let dirwatcher = {
    let dir_watch = cli.dir_watch.clone();
    let tx = tx.clone();

    thread::spawn(move || {
      if let Err(e) = dirwatch::watch_dir(&dir_watch, dirwatch::IN_MODIFY, tx) {
        eprintln!("\x1b[38;5;210mError watching directory:\x1b[0m {e}");
      }
    })
  };

  let cmd_runner = {
    let cmd = Cmd::new(&cli.cmd);
    let tx = rx.clone();

    thread::spawn(move || {
      if let Err(e) = run_cmd(cmd, tx) {
        eprintln!("\x1b[38;5;210mCommand execution failed:\x1b[0m {e}");
      }
    })
  };

  const QUIT_MSG: &[u8] = b"QUIT\r\n";
  let key_listener = {
    let tx = tx.clone();
    let addr = listener.local_addr()?;

    thread::spawn(move || -> Result<(), Error> {
      loop {
        match read_key()? {
          Key::Byte(b) if b.eq_ignore_ascii_case(&b'q') => {
            tx.send(Event::Quit);
            let mut stream = TcpStream::connect(addr)?;
            stream.write_all(QUIT_MSG)?;
            break;
          }
          _ => (),
        }
      }

      Ok(())
    })
  };

  let mut stream_peek = [0; 6];
  thread::scope(|s| -> Result<(), Error> {
    for stream in listener.incoming() {
      match stream {
        Ok(stream) => {
          stream.peek(&mut stream_peek)?;
          if stream_peek == QUIT_MSG {
            break;
          }

          let peer_addr = stream.peer_addr()?;
          let dir_serve = cli.dir_serve.clone();
          let rx = rx.clone();

          s.spawn(move || {
            if let Err(e) = handle_http(stream, &dir_serve, rx) {
              eprintln!("[{}] Error handling request: {}", peer_addr, e);
            }
          });
        }
        Err(e) => eprintln!("Connection failed: {}", e),
      }
    }

    Ok(())
  })?;

  cmd_runner.join().unwrap();
  key_listener.join().unwrap()?;
  dirwatcher.join().unwrap();

  println!("Server shutdown");
  Ok(())
}

pub fn send_sse_message(stream: &mut TcpStream) -> Result<(), Error> {
  stream.write_all(b"data: File changed\n\n")?;
  stream.flush()?;
  Ok(())
}
