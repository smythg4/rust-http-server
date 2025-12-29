use core::fmt;
use anyhow::{bail, Result};
use tokio::io::AsyncReadExt;
use crate::headers::Headers;


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParserState {
    Initialized,
    ParsingHeaders,
    ParsingBodyFull,
    ParsingBodyChunked,
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
}

impl TryFrom<&str> for HttpMethod {
    type Error = anyhow::Error;
    fn try_from(value: &str) -> Result<Self> {
        match value.to_uppercase().as_str() {
            "GET" => Ok(HttpMethod::Get),
            "POST" => Ok(HttpMethod::Post),
            _ => bail!("unknown method: {}", value),
        }
    }
}

impl fmt::Display for HttpMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpMethod::Get => write!(f, "GET"),
            HttpMethod::Post => write!(f, "POST"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpVersion {
    HTTP11,
}

impl TryFrom<&str> for HttpVersion {
    type Error = anyhow::Error;
    fn try_from(value: &str) -> Result<Self> {
        match value.to_uppercase().as_str() {
            "HTTP/1.1" => Ok(HttpVersion::HTTP11),
            _ => bail!("unknown HTTP version: {}", value),
        }
    }
}

impl fmt::Display for HttpVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpVersion::HTTP11 => write!(f, "HTTP/1.1"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RequestLine {
    pub method: HttpMethod,
    pub target: String,
    pub version: HttpVersion,
}

impl fmt::Display for RequestLine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f,"{} {} {}", self.method, self.target, self.version)
    }
}

impl RequestLine {
    pub fn parse_request_line(data: &[u8]) -> Result<(Option<Self>, usize)> {
        // read until a CRLF
        let line_str = String::from_utf8_lossy(data).to_string();
        if let Some((line, _rest)) = line_str.split_once("\r\n") {
            let p = line.len() + 2; // +2 accounts for '\r\n'
            // split that on whitespace
            let mut parts = line.split_whitespace();

            let method_raw = parts.next()
                .ok_or_else(|| anyhow::anyhow!("invalid http request line: missing method"))?;
            let target_raw = parts.next()
                .ok_or_else(|| anyhow::anyhow!("invalid http request line: missing target"))?;
            let version_raw = parts.next()
                .ok_or_else(|| anyhow::anyhow!("invalid http request line: missing version"))?;

            let method = HttpMethod::try_from(method_raw)?;
            let target = if target_raw.starts_with("/") {
                target_raw.trim().to_lowercase()
                } else {
                    bail!("target must start with '/'")
                };
            let version = HttpVersion::try_from(version_raw)?;

            Ok((Some(Self {
                method,
                target,
                version,
            }),p))
        } else {
            Ok((None, 0))
        }

    } 
}

#[derive(Debug)]
pub struct HttpRequest {
    parser_state: ParserState,
    pub request_line: Option<RequestLine>,
    pub headers: Headers,
    pub body: Vec<u8>,
}

impl HttpRequest {
    pub fn new() -> Self {
        Self {
            parser_state: ParserState::Initialized,
            request_line: None,
            headers: Headers::new(),
            body: Vec::new(),
        }
    }

    pub fn with_request_line(mut self, rl: RequestLine) -> Self {
        self.request_line = Some(rl);
        self
    }

    pub fn with_header(mut self, key: &str, value: &str) -> Self {
        self.headers.insert(key, value);
        self
    }

    pub fn with_body(mut self, body: Vec<u8>) -> Self {
        self.body = body;
        self
    }

    pub async fn parse_from<R: AsyncReadExt + Unpin>(conn: &mut R) -> Result<Self> {
        let mut request = HttpRequest::new();
        let mut buffer = [0u8; 1024];
        let mut buffer_len = 0;
        println!("Starting parse subroutine...");
        loop {

            // Only read if we need more data
            if buffer_len == 0 && request.parser_state != ParserState::Done {
                println!("Buffer Length: {}. Parser Status: {:?}", buffer_len, request.parser_state);
                println!("Reading from connection...");
                let n = conn.read(&mut buffer[buffer_len..]).await?;
                println!("Read {} bytes...", n);
                buffer_len += n;
            }

            match request.parser_state {
                ParserState::Initialized => {
                    println!("Parsing request line...");
                    let (request_line, consumed) = RequestLine::parse_request_line(&buffer[..buffer_len])?;
                    request.request_line = request_line;
                    buffer.copy_within(consumed.., 0);
                    buffer_len -= consumed;

                    println!("Consumed {} bytes, RL: {:?}", consumed, request.request_line);

                    if request.request_line.is_some() {
                        println!("Request line parsing complete. Moving on...");
                        request.parser_state = ParserState::ParsingHeaders;
                    }
                },
                ParserState::ParsingHeaders => {
                    println!("Parsing headers...");

                    if let (Some((field_name, field_value)), consumed) = Headers::parse_headers(&buffer[..buffer_len])? {
                        println!("Consumed {} bytes, {}: {}", consumed, field_name, field_value);

                        // digging into headers' inner to expose entry. Probably not the best way to do this...
                        let e: &mut String = request.headers.0.entry(field_name.trim().to_lowercase()).or_default();
                        if e.is_empty() {
                            *e = field_value.trim().to_string();
                        } else {
                            e.push_str(", ");
                            e.push_str(field_value.trim());
                        }
                        buffer.copy_within(consumed.., 0);
                        buffer_len -= consumed;
                    }

                    if buffer[..buffer_len].starts_with(b"\r\n") {
                        buffer.copy_within(2.., 0);
                        buffer_len -= 2;
                        println!("Headers parsing complete. Moving on...");

                        request.parser_state = if request.headers.get("transfer-encoding").map(|s| s.as_str()) == Some("chunked") {
                            ParserState::ParsingBodyChunked
                        } else if request.headers.get("content-length").is_none() {
                            println!("No body to parse. Calling it a day...");
                            ParserState::Done
                        } else {
                            ParserState::ParsingBodyFull
                        };
                    }
                },
                ParserState::ParsingBodyFull => {
                    println!("Parsing the full body...");
                    if let Some(content_length) = request.headers.get("content-length") {
                        let content_length = content_length.parse::<usize>()?;
                        
                        // copy from buffer to body
                        let to_copy = buffer_len.min(content_length - request.body.len());
                        request.body.extend_from_slice(&buffer[..to_copy]);
                        buffer.copy_within(to_copy.., 0);
                        buffer_len -= to_copy;

                        println!("Copied {} bytes to body, total: {}/{}", to_copy, request.body.len(), content_length);

                        if request.body.len() == content_length {
                            println!("Body parsing complete. Moving on...");
                            request.parser_state = ParserState::Done;
                        }
                    } else {
                        // no content-length -> no body
                        println!("No body to parse. Moving on...");
                        request.parser_state = ParserState::Done;
                    }
                    
                },
                ParserState::ParsingBodyChunked => {
                    println!("Parsing chunked body...");
                    let size_end= buffer[..buffer_len]
                        .windows(2)
                        .position(|two_bytes| two_bytes == b"\r\n")
                        .ok_or_else(|| anyhow::anyhow!("no chunk size found in data buffer"))?;

                    let size_str = String::from_utf8_lossy(&buffer[..size_end]);
                    let chunk_size = usize::from_str_radix(&size_str, 16)?;

                    if chunk_size == 0 {
                        // final chunk: "0\r\n\r\n"
                        buffer.copy_within(size_end+4.., 0);
                        buffer_len -= size_end + 4;
                        request.parser_state = ParserState::Done;
                        continue;
                    }

                    // check if we have the complete chunk: <size>\r\n<data>\r\n
                    let chunk_total = size_end + 2 + chunk_size + 2;
                    if buffer_len < chunk_total {
                        // don't have full chunk, need to read more data
                        break;
                    }

                    // extract chunk data (skip size line + \r\n)
                    let data_start = size_end + 2;
                    let data_end = data_start + chunk_size;
                    request.body.extend_from_slice(&buffer[data_start..data_end]);

                    // shift buffer (skip chunk + \r\n)
                    buffer.copy_within(data_end+2.., 0);
                    buffer_len -= data_end+2;
                },
                ParserState::Done => break,
            }
        }
        
        Ok(request)
    } 

    async fn read_chunk<R: AsyncReadExt + Unpin>(conn: &mut R) -> Result<Vec<u8>> {
        // read first two chars
        let mut hex_bytes = [0u8; 2];
        conn.read_exact(&mut hex_bytes).await?;
        let hex_str = String::from_utf8_lossy(&hex_bytes).to_string();
        let len = usize::from_str_radix(&hex_str, 16)?;

        // read the rest of the chunk
        let mut result = vec![0u8; len];
        conn.read_exact(&mut result).await?;

        Ok(result)
    }
}

impl fmt::Display for HttpRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // unwrap on request line is a little janky, but should never fail at this point
        write!(f,"{}\r\n{}\r\n{}", self.request_line.as_ref().unwrap(), self.headers, String::from_utf8_lossy(&self.body))
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
            (HttpMethod::Get, "/".to_string(), HttpVersion::HTTP11),
            (HttpMethod::Get, "/coffee".to_string(), HttpVersion::HTTP11),
            (HttpMethod::Get, "/".to_string(), HttpVersion::HTTP11), // err placeholder
            (HttpMethod::Get, "/".to_string(), HttpVersion::HTTP11), // err placeholder
            (HttpMethod::Post, "/coffee".to_string(), HttpVersion::HTTP11),
            (HttpMethod::Get, "/".to_string(), HttpVersion::HTTP11), // err placeholder
        ];

        for (i, test_line) in test_data.iter().enumerate() {
            let result = RequestLine::parse_request_line(&test_line);
            if [2,3, 5].contains(&i) {
                assert!(result.is_err());
                println!("{}: Error {:?}", i+1, result.err());
            } else {
                let (result, _p) = result.unwrap();
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
            (HttpMethod::Get, "/".to_string(), ("Host", "localhost:42069")),
            (HttpMethod::Get, "/".to_string(), ("","")), // err placeholder
            (HttpMethod::Get, "/coffee".to_string(), ("","")),
            (HttpMethod::Post, "/prime/agen".to_string(), ("Content-Type","text/plain, application/json")),
        ];
        for (i, test_line) in test_data.iter().enumerate() {
            let mut reader = std::io::Cursor::new(test_line);
            let result = HttpRequest::parse_from(&mut reader).await;
            if [1].contains(&i) {
                assert!(result.is_err());
                println!("{}: Error {:?}", i+1, result.err());
            } else {
                let request = result.unwrap();
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

    #[tokio::test]
    async fn parse_chunked_body() {
        let test_data = [
            // Simple chunked body: "Hello World"
            b"POST /upload HTTP/1.1\r\nHost: localhost:42069\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nHello\r\n6\r\n World\r\n0\r\n\r\n".to_vec(),

            // Single chunk: "Test"
            b"POST /data HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nTest\r\n0\r\n\r\n".to_vec(),

            // Empty chunked body (just terminator)
            b"POST /empty HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n".to_vec(),

            // Three chunks: "abc" + "def" + "ghi" = "abcdefghi"
            b"POST /multi HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n3\r\ndef\r\n3\r\nghi\r\n0\r\n\r\n".to_vec(),

            // Larger hex chunk size: "10" = 16 bytes in hex
            b"POST /hex HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n10\r\nSixteenBytesHere\r\n0\r\n\r\n".to_vec(),
        ];

        let expected = [
            (HttpMethod::Post, "/upload".to_string(), "Hello World"),
            (HttpMethod::Post, "/data".to_string(), "Test"),
            (HttpMethod::Post, "/empty".to_string(), ""),
            (HttpMethod::Post, "/multi".to_string(), "abcdefghi"),
            (HttpMethod::Post, "/hex".to_string(), "SixteenBytesHere"),
        ];

        for (i, test_line) in test_data.iter().enumerate() {
            let mut reader = std::io::Cursor::new(test_line);
            let result = HttpRequest::parse_from(&mut reader).await;

            assert!(result.is_ok(), "Test case {} failed to parse", i);
            let request = result.unwrap();

            let rl = request.request_line.unwrap();
            assert_eq!(expected[i].0, rl.method, "Test case {} method mismatch", i);
            assert_eq!(expected[i].1, rl.target, "Test case {} target mismatch", i);

            let body_str = String::from_utf8_lossy(&request.body);
            assert_eq!(expected[i].2, body_str, "Test case {} body mismatch", i);

            // Verify Transfer-Encoding header was present
            if i != 2 || !request.body.is_empty() {
                assert_eq!(
                    request.headers.get("transfer-encoding"),
                    Some(&"chunked".to_string()),
                    "Test case {} missing chunked header",
                    i
                );
            }

            println!("✓ Test case {}: {} bytes decoded correctly", i, request.body.len());
        }
    }
}