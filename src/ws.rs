use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::{str, thread};

use crate::encoding;
use crate::{dirwatch, error::Error};

fn generate_accept_key(key: &str) -> String {
  let magic = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
  let mut input = key.as_bytes().to_vec();
  input.extend_from_slice(magic.as_bytes());
  let hash = encoding::Sha1::hash(&input);
  encoding::base64_encode(&hash)
}

fn accept_websocket(stream: &mut TcpStream) -> Result<(), Error> {
  let mut buffer = [0; 1024];
  let _ = stream.read(&mut buffer)?;

  let request = str::from_utf8(&buffer)?;

  let key = request
    .lines()
    .find(|line| line.starts_with("Sec-WebSocket-Key:"))
    .ok_or_else(|| Error::MissingArg("Sec-WebSocket-Key"))?
    .split(':')
    .nth(1)
    .ok_or_else(|| Error::InvalidArg("Sec-WebSocket-Key"))?
    .trim()
    .to_string();

  let accept_key = generate_accept_key(&key);
  let response = format!(
    "HTTP/1.1 101 Switching Protocols\r\n\
     Upgrade: websocket\r\n\
     Connection: Upgrade\r\n\
     Sec-WebSocket-Accept: {}\r\n\r\n",
    accept_key
  );

  stream.write_all(response.as_bytes())?;
  stream.flush()?;
  Ok(())
}

pub fn send_websocket_message(stream: &mut TcpStream, message: &str) -> Result<(), Error> {
  let mut response = vec![0x81];
  let length = message.len();

  if length <= 125 {
    response.push(length as u8);
  }
  else if length <= 65535 {
    response.push(126);
    response.push((length >> 8) as u8);
    response.push(length as u8);
  }
  else {
    response.push(127);
    response.push((length >> 56) as u8);
    response.push((length >> 48) as u8);
    response.push((length >> 40) as u8);
    response.push((length >> 32) as u8);
    response.push((length >> 24) as u8);
    response.push((length >> 16) as u8);
    response.push((length >> 8) as u8);
    response.push(length as u8);
  }

  response.extend_from_slice(message.as_bytes());
  println!("Sending {:?}", String::from_utf8_lossy(&response));
  stream.write_all(&response)?;
  stream.flush()?;
  Ok(())
}

fn handle_websocket(mut stream: TcpStream, dir: String) -> Result<(), Error> {
  println!("WebSocket client connected");

  accept_websocket(&mut stream)?;
  dirwatch::watch_dir(&dir, dirwatch::IN_MODIFY, &mut stream)
}

fn handle_http(mut stream: TcpStream) -> Result<(), Error> {
  let mut buffer = [0; 1024];
  let _ = stream.read(&mut buffer)?;

  let get = b"GET / HTTP/1.1\r\n";
  if buffer.starts_with(get) {
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
    let response = "HTTP/1.1 404 NOT FOUND\r\n\r\n";
    stream.write_all(response.as_bytes())?;
    stream.flush()?;
  }
  Ok(())
}

pub fn run_server(dir: &str) -> Result<(), Error> {
  let listener = TcpListener::bind("0.0.0.0:8080")?;
  println!("Listening on port 8080");

  for stream in listener.incoming() {
    match stream {
      Ok(stream) => {
        let peer_addr = stream.peer_addr()?;
        let mut buf = [0; 7];
        let is_websocket = stream.peek(&mut buf).unwrap_or(0) > 0 && &buf == b"GET /ws";

        if is_websocket {
          let dir = dir.to_string();
          thread::spawn(move || {
            if let Err(e) = handle_websocket(stream, dir) {
              eprintln!("Error handling WebSocket client {}: {}", peer_addr, e);
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
