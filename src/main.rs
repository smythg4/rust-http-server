use anyhow::{Result, bail};
use tokio::net::TcpListener;

use crate::request::Request;

mod request;
mod headers;

#[tokio::main]
async fn main() -> Result<()> {
    let listener = TcpListener::bind("0.0.0.0:42069").await?;
    println!("Server listening on {}...", listener.local_addr()?);
    loop {
        let (conn, addr) = listener.accept().await?;
        println!("Accepted connection from: {}", addr);
        
        let mut request = Request::from(conn);
        request.parse().await?;
        println!("{}", request.request_line.unwrap());
        println!("{}", request.headers);
        println!("Terminating connection from: {}", addr);
    }

    Ok(())
}
