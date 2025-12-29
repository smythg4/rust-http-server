use crate::response::{HttpResponse, ResponseWriter, HttpStatus};
use crate::request::HttpRequest;

use tokio::fs::File;
use tokio::io::AsyncReadExt;


static DEFAULT_BODY: &str = "<html>
  <head>
    <title>200 OK</title>
  </head>
  <body>
    <h1>Success!</h1>
    <p>Your request was an absolute banger.</p>
  </body>
</html>
";

static BAD_REQUEST_BODY: &str = "<html>
  <head>
    <title>400 Bad Request</title>
  </head>
  <body>
    <h1>Bad Request</h1>
    <p>Your request honestly kinda sucked.</p>
  </body>
</html>
";

static INTERNAL_ERROR_BODY: &str = "<html>
  <head>
    <title>500 Internal Server Error</title>
  </head>
  <body>
    <h1>Internal Server Error</h1>
    <p>Okay, you know what? This one is on me.</p>
  </body>
</html>
";

// Handler error type
#[derive(Debug)]
pub struct HandlerError {
    pub status_code: HttpStatus,
    pub message: String,
}

impl HandlerError {
    pub fn to_response(&self) -> HttpResponse {
        HttpResponse::new()
            .with_status(self.status_code)
            .with_body(&self.message)
            .with_default_headers()
    }
}

//pub type Handler = fn(&mut ResponseWriter, &HttpRequest) -> Pin<Box<dyn Future<Output = Result<(), HandlerError>> + Send>>;

pub async fn default_handler(writer: &mut ResponseWriter, _req: &HttpRequest) -> Result<(), HandlerError> {
    let response = HttpResponse::new()
        .with_status(HttpStatus::Ok)
        .with_body(DEFAULT_BODY)
        .with_default_headers()
        .with_header("Content-Type", "text/html");
    writer.write_all(&response).await.map_err(|e| HandlerError { status_code: HttpStatus::InternalServerError, message: e.to_string() })?;
    Ok(())
}

pub async fn video_handler(writer: &mut ResponseWriter, _req: &HttpRequest) -> Result<(), HandlerError> {
    let mut f = File::open("assets/vim.mp4").await
        .map_err(|e| HandlerError{ status_code: HttpStatus::InternalServerError, message: e.to_string()})?;

    let final_response = HttpResponse::new()
        .with_status(HttpStatus::Ok)
        .with_header("Transfer-Encoding", "chunked")
        .with_header("Content-Type", "video/mp4")
        .with_header("Connection", "close");

    writer.write_status(&final_response.status).await
        .map_err(|e| HandlerError{ status_code: HttpStatus::InternalServerError, message: e.to_string()})?;
    writer.write_headers(&final_response.headers).await
        .map_err(|e| HandlerError{ status_code: HttpStatus::InternalServerError, message: e.to_string()})?;

    let mut file_buffer = [0u8; 512];
    let mut body_copy = Vec::new();
    while let Ok(n) = f.read(&mut file_buffer).await {
        if n == 0 {
            break;
        }
        println!("Forwarding video chunk of size {}", &file_buffer[..n].len());
        body_copy.extend_from_slice(&file_buffer[..n]);

        writer.write_chunked_body(&file_buffer[..n]).await
            .map_err(|e| HandlerError{ status_code: HttpStatus::InternalServerError, message: e.to_string()})?;
    }
    writer.write_chunked_body_done().await
        .map_err(|e| HandlerError{ status_code: HttpStatus::InternalServerError, message: e.to_string()})?;
    
    writer.write_trailers(&body_copy).await
        .map_err(|e| HandlerError{ status_code: HttpStatus::InternalServerError, message: e.to_string()})?;
    
    Ok(())
}

pub async fn proxy_handler(writer: &mut ResponseWriter, req: &HttpRequest) -> Result<(), HandlerError> {
    let (_trash, end_point) = req.request_line.as_ref().unwrap().target
        .split_once("httpbin/")
        .ok_or_else(|| HandlerError{ status_code: HttpStatus::InternalServerError, message: "invalid endpoint".to_string() })?;


    let dest_url = format!("https://httpbin.org/{}", end_point);
    println!("Forwarding request to: {}...", dest_url);

    let mut dest_response = reqwest::get(dest_url).await
        .map_err(|e| HandlerError{ status_code: HttpStatus::InternalServerError, message: e.to_string()})?;

    let final_response = HttpResponse::new()
        .with_status(HttpStatus::Ok)
        .with_header("Transfer-Encoding", "chunked")
        .with_header("Content-Type", "application/json")
        .with_header("Connection", "close");
        

    writer.write_status(&final_response.status).await
        .map_err(|e| HandlerError{ status_code: HttpStatus::InternalServerError, message: e.to_string()})?;
    writer.write_headers(&final_response.headers).await
        .map_err(|e| HandlerError{ status_code: HttpStatus::InternalServerError, message: e.to_string()})?;

    let mut body_copy = Vec::new();
    while let Some(chunk) = dest_response.chunk().await.map_err(|e| HandlerError{ status_code: HttpStatus::InternalServerError, message: e.to_string()})? {
        println!("Forwarding chunk of size {}", chunk.len());
        body_copy.extend_from_slice(&chunk);
        let chunk_bytes = chunk.to_vec();
        writer.write_chunked_body(&chunk_bytes).await
            .map_err(|e| HandlerError{ status_code: HttpStatus::InternalServerError, message: e.to_string()})?;
    }
    
    writer.write_chunked_body_done().await
        .map_err(|e| HandlerError{ status_code: HttpStatus::InternalServerError, message: e.to_string()})?;
    
    writer.write_trailers(&body_copy).await
        .map_err(|e| HandlerError{ status_code: HttpStatus::InternalServerError, message: e.to_string()})?;
    
    Ok(())
}

pub async fn dispatch_handler(writer: &mut ResponseWriter, req: &HttpRequest) -> Result<(), HandlerError> {
    if let Some(rl) = &req.request_line {
        match rl.target.as_str() {
            s if s.starts_with("/yourproblem") => {
                let resp = HttpResponse::new()
                    .with_status(HttpStatus::BadRequest)
                    .with_body(BAD_REQUEST_BODY)
                    .with_default_headers()
                    .with_header("Content-Type", "text/html");
                    writer.write_all(&resp).await
                        .map_err(|e| HandlerError { status_code: HttpStatus::InternalServerError, message: e.to_string() })
            },
            s if s.starts_with("/myproblem") => {
                let resp = HttpResponse::new()
                    .with_status(HttpStatus::InternalServerError)
                    .with_body(INTERNAL_ERROR_BODY)
                    .with_default_headers()
                    .with_header("Content-Type", "text/html");
                    writer.write_all(&resp).await
                        .map_err(|e| HandlerError { status_code: HttpStatus::InternalServerError, message: e.to_string() })
            },
            s if s.starts_with("/httpbin") => proxy_handler(writer, req).await,
            s if s.starts_with("/video") => video_handler(writer, req).await,
            _ => default_handler(writer, req).await,
        }
    } else {
        Err(HandlerError{
                status_code: HttpStatus::InternalServerError,
                message: "No request line found".to_string(),
            })
    }

    
}