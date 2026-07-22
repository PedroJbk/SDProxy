//! Security Handler - responde com status HTTP
//! Usado para verificações de status e headless browser checks

use std::io::Error;
use tokio::io::{AsyncWriteExt, AsyncReadExt};
use tokio::net::TcpStream;
use tokio::time::{timeout, Duration};

pub async fn handle_security(
    mut stream: TcpStream,
    status: &str,
) -> Result<(), Error> {
    println!("[Security] Nova conexão...");

    let _ = stream.write_all(
        format!("HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nX-Status: {}\r\nServer: SDProxy\r\nContent-Length: 0\r\n\r\n", status).as_bytes()
    ).await;

    Ok(())
}
