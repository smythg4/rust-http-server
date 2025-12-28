use core::fmt;
use std::collections::HashMap;

use anyhow::{bail, Result};
use tokio::io::AsyncReadExt;

use crate::headers::Headers;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParserState {
    Initialized,
    ParsingHeaders,
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HTTPMethod {
    GET,
    POST,
}

impl TryFrom<&str> for HTTPMethod {
    type Error = anyhow::Error;
    fn try_from(value: &str) -> Result<Self> {
        match value.to_uppercase().as_str() {
            "GET" => Ok(HTTPMethod::GET),
            "POST" => Ok(HTTPMethod::POST),
            _ => bail!("unknown method: {}", value),
        }
    }
}

impl fmt::Display for HTTPMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HTTPMethod::GET => write!(f, "GET"),
            HTTPMethod::POST => write!(f, "POST"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HTTPVersion {
    HTTP11,
}

impl TryFrom<&str> for HTTPVersion {
    type Error = anyhow::Error;
    fn try_from(value: &str) -> Result<Self> {
        match value.to_uppercase().as_str() {
            "HTTP/1.1" => Ok(HTTPVersion::HTTP11),
            _ => bail!("unknown HTTP version: {}", value),
        }
    }
}

impl fmt::Display for HTTPVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HTTPVersion::HTTP11 => write!(f, "HTTP/1.1"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RequestLine {
    method: HTTPMethod,
    target: String,
    version: HTTPVersion,
}

impl fmt::Display for RequestLine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f,"Request line:\n- Method: {}\n- Target: {}\n- Version: {}", self.method, self.target, self.version)
    }
}

impl RequestLine {
    pub async fn parse_request_line<R: AsyncReadExt + Unpin>(reader: &mut R) -> Result<Self> {
        // read until a CRLF
        let mut byte_buf = [0u8; 1];
        let mut line_bytes = Vec::new();
        while !line_bytes.ends_with(b"\r\n") {
            let _ = reader.read_exact(&mut byte_buf).await?;
            line_bytes.extend_from_slice(&byte_buf);
        }
        // now convert it to a string
        let line = String::from_utf8_lossy(&line_bytes).to_string();
        // split that on whitespace
        let mut parts = line.split_whitespace();

        let method_raw = parts.next()
            .ok_or_else(|| anyhow::anyhow!("invalid http request line: missing method"))?;
        let target_raw = parts.next()
            .ok_or_else(|| anyhow::anyhow!("invalid http request line: missing target"))?;
        let version_raw = parts.next()
            .ok_or_else(|| anyhow::anyhow!("invalid http request line: missing version"))?;

        let method = HTTPMethod::try_from(method_raw)?;
        let target = if target_raw.starts_with("/") {
            target_raw.trim().to_lowercase()
            } else {
                bail!("target must start with '/'")
            };
        let version = HTTPVersion::try_from(version_raw)?;

        Ok(Self {
            method,
            target,
            version,
        })
    } 
}

#[derive(Debug)]
pub struct Request<R: AsyncReadExt + Unpin> {
    conn: R,
    parser_state: ParserState,
    pub request_line: Option<RequestLine>,
    pub headers: Headers,
    body: Vec<u8>,
}

impl<R: AsyncReadExt + Unpin> Request<R> {
    pub fn from(r: R) -> Self {
        Self {
            conn: r,
            parser_state: ParserState::Initialized,
            request_line: None,
            headers: Headers::new(),
            body: Vec::new(),
        }
    }
    pub async fn parse(&mut self) -> Result<()> {
        loop {
            match self.parser_state {
                    ParserState::Initialized => {
                    let request_line = RequestLine::parse_request_line(&mut self.conn).await?;
                    self.request_line = Some(request_line);
                    self.parser_state = ParserState::ParsingHeaders;
                },
                ParserState::ParsingHeaders => {
                    let headers = Headers::parse_headers(&mut self.conn).await?;
                    self.headers = headers;
                    self.parser_state = ParserState::Done;
                },
                ParserState::Done => break,
            }
        }
        
        Ok(())
    } 
}

#[cfg(test)]
mod test {
    use super::*;

    #[tokio::test]
    async fn request_line_basics() {
        let test_data = [
            b"GET / HTTP/1.1\r\nHost: localhost:42069\r\nUser-Agent: curl/7.81.0\r\nAccept: */*\r\n\r\n".to_vec(),
            b"GET /coffee HTTP/1.1\r\nHost: localhost:42069\r\nUser-Agent: curl/7.81.0\r\nAccept: */*\r\n\r\n".to_vec(),
            b"/coffee HTTP/1.1\r\nHost: localhost:42069\r\nUser-Agent: curl/7.81.0\r\nAccept: */*\r\n\r\n".to_vec(), // no method
            b"GET POST /coffee HTTP/1.1\r\nHost: localhost:42069\r\nUser-Agent: curl/7.81.0\r\nAccept: */*\r\n\r\n".to_vec(), // too many elements
            b"POST /coffee HTTP/1.1\r\nHost: localhost:42069\r\nUser-Agent: curl/7.81.0\r\nAccept: */*\r\n\r\n".to_vec(),
            b"/ GET HTTP/1.1\r\nHost: localhost:42069\r\nUser-Agent: curl/7.81.0\r\nAccept: */*\r\n\r\n".to_vec(), // out of order
        ];
        let expected = [
            (HTTPMethod::GET, "/".to_string(), HTTPVersion::HTTP11),
            (HTTPMethod::GET, "/coffee".to_string(), HTTPVersion::HTTP11),
            (HTTPMethod::GET, "/".to_string(), HTTPVersion::HTTP11), // err placeholder
            (HTTPMethod::GET, "/".to_string(), HTTPVersion::HTTP11), // err placeholder
            (HTTPMethod::POST, "/coffee".to_string(), HTTPVersion::HTTP11),
            (HTTPMethod::GET, "/".to_string(), HTTPVersion::HTTP11), // err placeholder
        ];

        for (i, test_line) in test_data.iter().enumerate() {
            let mut reader = std::io::Cursor::new(test_line);
            let result = RequestLine::parse_request_line(&mut reader).await;
            if [2,3, 5].contains(&i) {
                assert!(result.is_err());
                println!("{}: Error {:?}", i+1, result.err());
            } else {
                let result = result.unwrap();
                println!("{}: {:?}", i+1, result);
                assert_eq!(expected[i].0, result.method);
                assert_eq!(expected[i].1, result.target);
                assert_eq!(expected[i].2, result.version);
            }
        }
    }

    #[tokio::test]
    async fn parse_with_headers() {
        let test_data = [
            b"GET / HTTP/1.1\r\nHost: localhost:42069\r\nUser-Agent: curl/7.81.0\r\nAccept: */*\r\n\r\n".to_vec(),
            b"GET / HTTP/1.1\r\nHost localhost:42069\r\n\r\n".to_vec(), // invalid headers (missing ':')
            b"GET /coffee HTTP/1.1\r\n\r\n\r\n".to_vec(), // empty headers
            b"POST /prime/agen HTTP/1.1\r\nHost: localhost:42069\r\nUser-Agent: curl/7.81.0\r\nAccept: */*\r\nContent-Type: text/plain\r\nContent-Type: application/json\r\n\r\n".to_vec(),
        ];
        let expected = [
            (HTTPMethod::GET, "/".to_string(), ("Host", "localhost:42069")),
            (HTTPMethod::GET, "/".to_string(), ("","")), // err placeholder
            (HTTPMethod::GET, "/coffee".to_string(), ("","")),
            (HTTPMethod::POST, "/prime/agen".to_string(), ("Content-Type","text/plain, application/json")),
        ];
        for (i, test_line) in test_data.iter().enumerate() {
            let reader = std::io::Cursor::new(test_line);
            let mut request = Request::from(reader);
            let result = request.parse().await;
            if [1].contains(&i) {
                assert!(result.is_err());
                println!("{}: Error {:?}", i+1, result.err());
            } else {
                let rl = request.request_line.unwrap();
                assert_eq!(expected[i].0, rl.method);
                assert_eq!(expected[i].1, rl.target);
                if i == 0 {
                    let (k,v) = (expected[i].2.0, expected[i].2.1);
                    assert_eq!(request.headers.get(k).map(|s| s.as_str()), Some(v));
                } if i == 2 {
                    assert_eq!(request.headers.len(), 0);
                }

            }
        }
    }
}