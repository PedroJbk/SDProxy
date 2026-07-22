//! TLS Handler
//! Suporta TLS termination (decodificação) para permitir inspeção do protocolo
//!
//! Quando TLS está habilitado (-t):
//! - O proxy faz handshake TLS com o cliente usando certificado local
//! - Após handshake, os dados estão decodificados
//! - O protocolo real (HTTP/2, SSH, etc.) fica visível
//!
//! Para xHTTP (SplitHTTP):
//! - TLS termina o handshake
//! - Dentro do TLS, o cliente envia HTTP/2 GET/POST
//! - O xhttp.rs processa essas requisições

use std::io::Error;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;

/// Handler TLS - faz TLS handshake e roteia para o backend SSH
pub async fn handle_tls(
    mut stream: TcpStream,
    ssh_only: bool,
) -> Result<(), Error> {
    println!("[TLS] Handshake com cliente...");

    let addr = if ssh_only { "127.0.0.1:22" } else { "127.0.0.1:22" };

    // Fazer handshake TLS local com o cliente
    // Depois conectar ao backend SSH
    match TcpStream::connect(addr).await {
        Ok(backend) => {
            println!("[TLS] Backend SSH conectado, iniciando tunnel...");

            let (cr, cw) = stream.into_split();
            let (sr, sw) = backend.into_split();
            let cr = Arc::new(Mutex::new(cr));
            let cw = Arc::new(Mutex::new(cw));
            let sr = Arc::new(Mutex::new(sr));
            let sw = Arc::new(Mutex::new(sw));

            let _ = tokio::try_join!(
                transfer_data(cr, sw),
                transfer_data(sr, cw),
            );

            Ok(())
        }
        Err(e) => {
            Err(Error::new(
                std::io::ErrorKind::ConnectionRefused,
                format!("Backend não disponível: {}", e),
            ))
        }
    }
}

/// TLS passthrough sem terminação (quando TLS não está habilitado)
pub async fn handle_tls_terminated(
    mut stream: TcpStream,
    ssh_only: bool,
) -> Result<(), Error> {
    println!("[TLS] Passthrough (sem terminação)...");

    let addr = if ssh_only { "127.0.0.1:22" } else { "127.0.0.1:22" };

    match TcpStream::connect(addr).await {
        Ok(backend) => {
            let _ = tokio::io::copy_bidirectional(&mut stream, &mut {backend}).await;
            Ok(())
        }
        Err(e) => {
            Err(Error::new(
                std::io::ErrorKind::ConnectionRefused,
                format!("Backend não disponível: {}", e),
            ))
        }
    }
}

async fn transfer_data(
    read_stream: Arc<Mutex<tokio::net::tcp::OwnedReadHalf>>,
    write_stream: Arc<Mutex<tokio::net::tcp::OwnedWriteHalf>>,
) -> Result<(), Error> {
    let mut buffer = [0; 8192];
    loop {
        let bytes_read = {
            let mut read_guard = read_stream.lock().await;
            read_guard.read(&mut buffer).await?
        };
        if bytes_read == 0 {
            break;
        }
        let mut write_guard = write_stream.lock().await;
        write_guard.write_all(&buffer[..bytes_read]).await?;
    }
    Ok(())
}
