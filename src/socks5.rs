//! SOCKS5 Handler
//! Aceita conexões SOCKS5 e faz proxy para SSH

use std::io::Error;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::time::{timeout, Duration};

pub async fn handle_socks5(mut stream: TcpStream) -> Result<(), Error> {
    println!("[SOCKS5] Nova conexão...");

    // Ler handshake SOCKS5
    let mut buf = [0u8; 256];
    let n = match timeout(Duration::from_secs(5), stream.read(&mut buf)).await {
        Ok(Ok(n)) => n,
        _ => return Ok(()),
    };

    if n < 2 || buf[0] != 0x05 {
        println!("[SOCKS5] Handshake inválido");
        return Ok(());
    }

    // Responder: version 5, no auth
    stream.write_all(&[0x05, 0x00]).await?;

    // Ler CONNECT request
    let n2 = match timeout(Duration::from_secs(5), stream.read(&mut buf)).await {
        Ok(Ok(n)) => n,
        _ => return Ok(()),
    };

    if n2 < 7 {
        println!("[SOCKS5] CONNECT request inválido");
        return Ok(());
    }

    // Responder: sucesso, bind address 0.0.0.0:0
    let mut response = vec![0x05, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
    stream.write_all(&response).await?;
    stream.flush().await?;

    // Tunnel para SSH
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
        Err(e) => println!("[SOCKS5] Erro backend: {}", e),
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
