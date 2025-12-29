use anyhow::Result;
use tokio::net::{TcpListener, TcpStream};
use std::net::SocketAddr;
use tokio::sync::oneshot;

use crate::{request::HttpRequest, response::ResponseWriter};
use crate::handlers::dispatch_handler;

pub struct HttpServer {
    listener: TcpListener,
    close_conn_rx: oneshot::Receiver<()>,
}

impl HttpServer {
    pub async fn serve(port: usize) -> Result<(Self, oneshot::Sender<()>)> {
        let listener = TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
        let (tx, rx) = oneshot::channel::<()>();
        Ok((Self {
            listener,
            close_conn_rx: rx,
        }, tx))
    }

    pub async fn listen(&mut self) -> Result<()> {
        loop {
            tokio::select! {
                _ = &mut self.close_conn_rx => break,
                result = self.listener.accept() => {
                    let (conn, addr) = result?;
                    tokio::spawn(async move {
                        if let Err(e) = Self::handle_connection(conn, addr).await {
                            eprintln!("Connection error from {}: {}", addr, e);
                        }
                    });
                }
            };
        }
        println!("Gracefully shutting down server...");
        Ok(())
    }

    pub async fn handle_connection(mut conn: TcpStream, addr: SocketAddr) -> Result<()> {
        println!("Accepted connection from: {}", addr);
        let request = HttpRequest::parse_from(&mut conn).await?;

        let mut writer = ResponseWriter::from(conn);

        // Call handler
        if let Err(e) = dispatch_handler(&mut writer, &request).await {
            writer.write_all(&e.to_response()).await?;
        };

        println!("Terminating connection from: {}", addr);
        Ok(())
    }
}