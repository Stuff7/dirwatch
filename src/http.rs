use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt::Display;
use std::fs::File;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::ops::{Deref, DerefMut};
use std::path::Path;
use std::str;

use crate::error::Error;

#[derive(Debug)]
pub struct HttpRequest {
  pub method: HttpMethod,
  pub path: Box<str>,
  pub peer_addr: SocketAddr,
  pub headers: HttpHeaders,
}

impl TryFrom<&mut TcpStream> for HttpRequest {
  type Error = Error;
  fn try_from(stream: &mut TcpStream) -> Result<Self, Self::Error> {
    let peer_addr = stream.peer_addr().unwrap_or_else(|_| "Unknown".parse().unwrap());
    let buffer = read_request_headers(stream)?;
    let request = str::from_utf8(&buffer)?;

    let mut lines = request.lines();
    let mut method = lines.next().ok_or(Error::EmptyRequest)?.split_whitespace();
    let (method, path) = (method.next().unwrap_or_default(), method.next().unwrap_or("???"));

    let mut request = Self {
      method: method.into(),
      path: path.into(),
      peer_addr,
      headers: HttpHeaders(HashMap::new()),
    };

    for line in lines {
      let Some((k, v)) = line.split_once(':')
      else {
        break;
      };

      request.headers.set(k.to_ascii_lowercase(), v[1..].to_string());
    }

    Ok(request)
  }
}

fn read_request_headers(stream: &mut TcpStream) -> Result<Vec<u8>, Error> {
  let mut buffer = Vec::new();
  let mut chunk = [0; 512];

  loop {
    match stream.read(&mut chunk) {
      Ok(0) => break,
      Ok(n) => {
        buffer.extend_from_slice(&chunk[..n]);

        if buffer.windows(4).any(|window| window == b"\r\n\r\n") {
          break;
        }
      }
      Err(e) => {
        return Err(Error::Io(e));
      }
    }
  }

  Ok(buffer)
}

pub struct HttpResponse {
  pub status: usize,
  pub headers: HttpHeaders,
  pub contents: Vec<u8>,
}

impl HttpResponse {
  pub fn new() -> Self {
    Self {
      status: 200,
      headers: HttpHeaders(HashMap::new()),
      contents: Vec::new(),
    }
  }

  pub fn set_404(&mut self) -> &mut Self {
    self
      .set_status(404)
      .set_content(b"404 Not Found")
      .set_header("content-type", "text/plain");
    self
  }

  pub fn set_status(&mut self, status: usize) -> &mut Self {
    self.status = status;
    self
  }

  pub fn set_content(&mut self, content: &[u8]) -> &mut Self {
    self.contents.extend_from_slice(content);
    self
  }

  pub fn set_header<T: Into<Cow<'static, str>>>(&mut self, k: T, v: T) -> &mut Self {
    self.headers.set(k, v);
    self
  }

  pub fn set_file<P: AsRef<Path>>(&mut self, path: P) -> Result<(), Error> {
    self.contents.clear();
    let Ok(mut file) = File::open(&path)
    else {
      self.set_404();
      return Ok(());
    };

    file.read_to_end(&mut self.contents)?;

    let mime_type = match path.as_ref().extension().and_then(|ext| ext.to_str()) {
      Some("html") => "text/html",
      Some("css") => "text/css",
      Some("js") => "application/javascript",
      Some("png") => "image/png",
      Some("ico") => "image/x-icon",
      _ => "application/octet-stream",
    };

    self.set_header("content-type", mime_type);

    Ok(())
  }

  pub fn write_to(&mut self, stream: &mut TcpStream) -> Result<(), Error> {
    stream.write_all(self.to_string().as_bytes())?;
    if !self.contents.is_empty() {
      stream.write_all(&self.contents)?;
    }
    stream.flush()?;

    Ok(())
  }

  fn status_text(&self) -> &str {
    match self.status {
      200 => "OK",
      404 => "NOT FOUND",
      s => todo!("Http status text for {s}"),
    }
  }
}

impl Display for HttpResponse {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "HTTP/1.1 {} {}\r\n{}", self.status, self.status_text(), self.headers)?;
    if !self.contents.is_empty() {
      write!(f, "content-length: {}\r\n", self.contents.len())?;
    }
    write!(f, "\r\n")
  }
}

#[derive(Debug)]
pub enum HttpMethod {
  Get,
  Post,
  Put,
  Delete,
  Patch,
  Options,
  Head,
  Trace,
  Connect,
  Unknown,
}

impl From<&str> for HttpMethod {
  fn from(value: &str) -> Self {
    match value.to_uppercase().as_str() {
      "GET" => Self::Get,
      "POST" => Self::Post,
      "PUT" => Self::Put,
      "DELETE" => Self::Delete,
      "PATCH" => Self::Patch,
      "OPTIONS" => Self::Options,
      "HEAD" => Self::Head,
      "TRACE" => Self::Trace,
      "CONNECT" => Self::Connect,
      _ => Self::Unknown,
    }
  }
}

#[derive(Debug)]
pub struct HttpHeaders(HashMap<Cow<'static, str>, Cow<'static, str>>);

impl HttpHeaders {
  pub fn set<T: Into<Cow<'static, str>>>(&mut self, k: T, v: T) -> &mut Self {
    self.insert(k.into(), v.into());
    self
  }
}

impl Deref for HttpHeaders {
  type Target = HashMap<Cow<'static, str>, Cow<'static, str>>;
  fn deref(&self) -> &Self::Target {
    &self.0
  }
}

impl DerefMut for HttpHeaders {
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.0
  }
}

impl Display for HttpHeaders {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    for (k, v) in &self.0 {
      write!(f, "{k}: {v}\r\n")?;
    }
    Ok(())
  }
}
