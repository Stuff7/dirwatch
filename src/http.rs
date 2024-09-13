use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt::Display;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::net::{SocketAddr, TcpStream};
use std::ops::{Deref, DerefMut, Range};
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

impl Display for HttpRequest {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(
      f,
      "[\x1b[93m  {}\x1b[0m] \x1b[33m󰋻 {:?} {}\x1b[0m | \x1b[36m󰮤 {}\x1b[0m",
      self.peer_addr,
      self.method,
      self.path,
      self.headers.get("user-agent").unwrap_or(&"No user agent".into()),
    )
  }
}

impl HttpRequest {
  pub fn get_range(&self) -> Option<Range<usize>> {
    self.headers.get("range").and_then(|r| {
      let (kw, r) = r.split_once('=')?;

      if kw != "bytes" {
        return None;
      }

      let (s, e) = r.split_once('-')?;
      let s: usize = s.parse().ok()?;
      let e: usize = e.parse().unwrap_or(0);

      Some(s..e)
    })
  }

  pub fn from_ip(peer_addr: SocketAddr) -> Self {
    Self {
      path: "".into(),
      peer_addr,
      method: HttpMethod::Unknown,
      headers: HttpHeaders(HashMap::new()),
    }
  }

  /// Returns whether the request was changed or not.
  pub fn read_from_buffer(&mut self, buffer: &[u8]) -> Result<bool, Error> {
    let request = str::from_utf8(buffer)?;

    let mut lines = request.lines();
    let Some(mut method) = lines.next().map(|ln| ln.split_whitespace())
    else {
      return Ok(false);
    };
    let (method, path) = (method.next().unwrap_or_default(), method.next().unwrap_or("???"));

    let path = match path.bytes().position(|b| b == b'?' || b == b'#') {
      Some(pos) => &path[..pos],
      None => path,
    };

    self.method = method.into();
    self.path = path.into();
    self.headers.clear();

    for line in lines {
      let Some((k, v)) = line.split_once(':')
      else {
        break;
      };

      self.headers.set(k.to_ascii_lowercase(), v[1..].to_string());
    }

    Ok(true)
  }
}

pub fn read_request_headers(stream: &mut TcpStream) -> Result<Vec<u8>, Error> {
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

  pub fn set_header<K: Into<Cow<'static, str>>, V: Into<Cow<'static, str>>>(&mut self, k: K, v: V) -> &mut Self {
    self.headers.set(k, v);
    self
  }

  pub fn set_file<P: AsRef<Path>>(&mut self, path: P, req: &HttpRequest) -> Result<(), Error> {
    self.contents.clear();

    let Ok(mut file) = File::open(&path)
    else {
      self.set_404();
      return Ok(());
    };

    /// 10 MB
    const MAX_CONTENT_LEN: usize = 10 * 1000 * 1000;

    let meta = file.metadata()?;
    let file_size = meta.len() as usize;
    let content_type = get_mime_type(path.as_ref());
    self.set_header("content-type", content_type);

    let (is_range_in_req, start, end) = req
      .get_range()
      .map(|r| {
        let end = if r.end == 0 { file_size } else { r.end };
        (true, r.start, end)
      })
      .unwrap_or((false, 0, file_size));

    if !content_type.contains("image") && (is_range_in_req || file_size > MAX_CONTENT_LEN) {
      let end = end.min(start + MAX_CONTENT_LEN);

      file.seek(SeekFrom::Start(start as u64))?;
      self.contents.resize(end - start, 0);
      file.read_exact(&mut self.contents)?;

      self
        .set_status(206)
        .set_header("accept-ranges", "bytes")
        .set_header("content-range", format!("bytes {}-{}/{}", start, end - 1, file_size));
    }
    else {
      file.read_to_end(&mut self.contents)?;
    }

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
      200 => "Ok",
      206 => "Partial Content",
      404 => "Not Found",
      s => todo!("Http status text for {s}"),
    }
  }
}

fn get_mime_type(path: &std::path::Path) -> &'static str {
  let Some(ext) = path.extension().and_then(|ext| ext.to_str()).map(|ext| ext.to_ascii_lowercase())
  else {
    return "application/octet-stream";
  };

  match ext.as_str() {
    "html" => "text/html",
    "css" => "text/css",
    "js" => "application/javascript",
    "json" => "application/json",
    "png" => "image/png",
    "jpg" | "jpeg" => "image/jpeg",
    "gif" => "image/gif",
    "svg" => "image/svg+xml",
    "ico" => "image/x-icon",
    "xml" => "application/xml",
    "pdf" => "application/pdf",
    "zip" => "application/zip",
    "mp4" => "video/mp4",
    "mov" => "video/quicktime",
    "mp3" => "audio/mpeg",
    "wav" => "audio/wav",
    "ogg" => "audio/ogg",
    "webp" => "image/webp",
    _ => "application/octet-stream",
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
  pub fn get<K: Into<Cow<'static, str>>>(&self, k: K) -> Option<&Cow<'static, str>> {
    self.0.get(&k.into())
  }

  pub fn set<K: Into<Cow<'static, str>>, V: Into<Cow<'static, str>>>(&mut self, k: K, v: V) -> &mut Self {
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
