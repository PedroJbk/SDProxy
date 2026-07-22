//! SSH Handler
//! Faz tunnel direto para o SSH local (127.0.0.1:22)

use std::io::Error;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;

pub async fn handle_ssh_tunnel(
    mut stream: TcpStream,
    backend_addr: &str,
) -> Result<(), Error> {
    println!("[SSH] Nova conexão → {}", backend_addr);

    match TcpStream::connect(backend_addr).await {
        Ok(backend) => {
            println!("[SSH] Tunnel estabelecido com {}", backend_addr);
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
            Ok(())
        }
        Err(e) => {
            Err(Error::new(
                std::io::ErrorKind::ConnectionRefused,
                format!("SSH backend não disponível: {}", e),
            ))
        }
    }
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
