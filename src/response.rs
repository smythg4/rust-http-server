use std::fmt;
use tokio::net::TcpStream;
use tokio::io::AsyncWriteExt;
use sha2::{Sha256, Digest};

use crate::headers::Headers;

#[derive(Debug, Clone, PartialEq, Eq, Copy)]
pub enum HttpStatus {
    Ok,
    BadRequest,
    InternalServerError,
}

impl fmt::Display for HttpStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpStatus::Ok => write!(f, "HTTP/1.1 200 OK"),
            HttpStatus::BadRequest => write!(f, "HTTP/1.1 400 Bad Request"),
            HttpStatus::InternalServerError => write!(f, "HTTP/1.1 500 Internal Server Error"),
        }
    }
}

#[derive(Debug)]
pub struct HttpResponse {
    pub status: HttpStatus,
    pub headers: Headers,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn new() -> Self {
        HttpResponse {
            status: HttpStatus::Ok,
            headers: Headers::new(),
            body: Vec::new(),
        }
    }

    pub fn with_status(mut self, status: HttpStatus) -> Self {
        self.status = status;
        self
    }

    pub fn with_default_headers(mut self) -> Self {
        let mut headers = Headers::new();
        headers.insert("Content-Length", &self.body.len().to_string());
        headers.insert("Connection", "close");
        headers.insert("Content-Type", "text/plain");
        self.headers = headers;
        self
    }

    pub fn with_header(mut self, name: &str, value: &str) -> Self {
        self.headers.insert(name, value);
        self
    }

    pub fn with_body(mut self, message: &str) -> Self {
        self.body = message.as_bytes().to_vec();
        self
    }

}

impl fmt::Display for HttpResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // unwrap on request line is a little janky, but should never fail at this point
        write!(f,"{}\r\n{}\r\n{}", self.status, self.headers, String::from_utf8_lossy(&self.body))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Copy)]
enum WriterState {
    Initial,
    WritingHeaders,
    WritingBodyFull,
    WritingBodyChunked,
    WritingTrailers,
    Done,
}

#[derive(Debug)]
pub struct ResponseWriter {
    writer: TcpStream,
    state: WriterState,
}

impl ResponseWriter {
    pub fn from(writer: TcpStream) -> Self {
        Self { writer, state: WriterState::Initial }
    }

    pub async fn write_all(&mut self, response: &HttpResponse) -> Result<(), std::io::Error> {
        self.write_status(&response.status).await?;
        self.write_headers(&response.headers).await?;
        self.write_body_full(&response.body).await?;
        Ok(())
    }

    pub async fn write_status(&mut self, status_line: &HttpStatus) -> Result<(), std::io::Error> {
        self.writer.write_all(format!("{}\r\n", status_line).as_bytes()).await?;
        self.state = WriterState::WritingHeaders;
        Ok(())
    }

    pub async fn write_headers(&mut self, headers: &Headers) -> Result<(), std::io::Error> {
        self.writer.write_all(format!("{}\r\n",headers).as_bytes()).await?;
        self.state = match headers.get("Transfer-Encoding").map(|s| s.as_str()) {
            Some("chunked") => WriterState::WritingBodyChunked,
            _ => WriterState::WritingBodyFull,
        };
        Ok(())
    }

    pub async fn write_body_full(&mut self, response_body: &[u8]) -> Result<(), std::io::Error> {
        self.writer.write_all(response_body).await?;
        self.state = WriterState::Done;
        Ok(())
    }

    pub async fn write_chunked_body(&mut self, chunk: &[u8]) -> Result<(), std::io::Error> {
        if chunk.is_empty() {
            return self.write_chunked_body_done().await;
        }
        self.writer.write_all(format!("{:x}\r\n", chunk.len()).as_bytes()).await?;
        self.writer.write_all(chunk).await?;
        self.writer.write_all(b"\r\n").await?;
        Ok(())
    }

    pub async fn write_chunked_body_done(&mut self) -> Result<(), std::io::Error> {
        self.writer.write_all(b"0\r\n").await?;
        self.state = WriterState::WritingTrailers;
        Ok(())
    }

    pub async fn write_trailers(&mut self, body: &[u8]) -> Result<(), std::io::Error> {
        let mut headers = Headers::new();
        let body_hash = Sha256::digest(body);
        headers.insert("X-Content-SHA256", &format!("{:x}", body_hash));
        headers.insert("X-Content-Length", body.len().to_string().as_str());
        self.writer.write_all(headers.to_string().as_bytes()).await?;
        self.writer.write_all(b"\r\n").await?;
        self.state = WriterState::Done;
        Ok(())
    }
}