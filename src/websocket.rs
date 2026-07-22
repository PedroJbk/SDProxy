//! WebSocket Handler
//! Detecta handshake WebSocket e faz proxy para SSH

use std::io::Error;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::time::{timeout, Duration};

pub async fn handle_websocket(
    mut stream: TcpStream,
    status: &str,
) -> Result<(), Error> {
    println!("[WebSocket] Nova conexão...");

    // Ler handshake WebSocket
    let mut buf = vec![0u8; 8192];
    let n = match timeout(Duration::from_secs(5), stream.read(&mut buf)).await {
        Ok(Ok(n)) => n,
        _ => return Ok(()),
    };

    let request = String::from_utf8_lossy(&buf[..n]);

    // Responder com 101 Switching Protocols
    let response = format!(
        "HTTP/1.1 101 Switching Protocols\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Accept: dummy\r\n\
         X-Status: {}\r\n\r\n",
        status
    );

    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.flush().await;

    // Tunnel para SSH backend
    let addr = "127.0.0.1:22";
    match TcpStream::connect(addr).await {
        Ok(backend) => {
            let (cr, cw) = stream.into_split();
            let (sr, sw) = backend.into_split();
            let cr = Arc::new(Mutex::new(cr));
            let cw = Arc::new(Mutex::new(cw));
            let sr = Arc::new(Mutex::new(sr));
            let sw = Arc::new(Mutex::new(sw));
            let _ = tokio::try_join!(
                transfer(cr, sw),
                transfer(sr, cw),
            );
        }
        Err(e) => println!("[WebSocket] Erro backend: {}", e),
    }

    Ok(())
}

async fn transfer(
    read_stream: Arc<Mutex<tokio::net::tcp::OwnedReadHalf>>,
    write_stream: Arc<Mutex<tokio::net::tcp::OwnedWriteHalf>>,
) -> Result<(), Error> {
    let mut buffer = [0; 8192];
    loop {
        let bytes_read = {
            let mut read_guard = read_stream.lock().await;
            read_guard.read(&mut buffer).await?
        };
        if bytes_read == 0 { break; }
        let mut write_guard = write_stream.lock().await;
        write_guard.write_all(&buffer[..bytes_read]).await?;
    }
    Ok(())
}
