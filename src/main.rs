use anyhow::Result;

mod request;
mod response;
mod headers;
mod server;
mod handlers;

const PORT: usize = 42069;

use crate::server::HttpServer;

#[tokio::main]
async fn main() -> Result<()> {
    let (mut server, cancel_ch) = HttpServer::serve(PORT).await?;
    println!("Server started on port {}...", PORT);

    let handle = tokio::spawn(async move {
        server.listen().await
    });

    tokio::signal::ctrl_c().await?;
    cancel_ch.send(()).ok();

    match handle.await {
        Ok(Ok(())) => println!("Server shut down gracefully"),
        Ok(Err(e)) => eprintln!("Server error: {}", e),
        Err(e) => eprintln!("Task panicked: {}", e),
    }

    Ok(())
}
