use tokio::io::{AsyncReadExt, AsyncWriteExt, copy_bidirectional};
use tokio::net::TcpStream;
use anyhow::Result;
use log::info;

pub async fn handle_security(mut socket: TcpStream, status: &str) -> Result<()> {
    info!("🔐 SECURITY handshake (Tripla Resposta)...");
    
    // Consumir os headers da requisição inicial
    let mut buf = [0u8; 1024];
    let _ = socket.read(&mut buf).await?;
    
    // Conforme o print do usuário:
    // 1. Status: 101 (STATUS) Informational
    socket.write_all(format!("HTTP/1.1 101 {}\r\n\r\n", status).as_bytes()).await?;

    // 2. Enviando 200 HTTP status - HTTP/1.1 200 OK
    socket.write_all(b"HTTP/1.1 200 OK\r\n\r\n").await?;

    // 3. HTTP/1.1 200 (STATUS)
    socket.write_all(format!("HTTP/1.1 200 {}\r\n\r\n", status).as_bytes()).await?;

    info!("🔐 SECURITY handshake complete!");
    
    // Conectar ao backend (SSH por padrão para Security)
    let mut remote = match TcpStream::connect("127.0.0.1:22").await {
        Ok(s) => s,
        Err(_) => {
            TcpStream::connect("127.0.0.1:1194").await?
        }
    };

    info!("✅ SECURITY Túnel iniciado.");
    let _ = copy_bidirectional(&mut socket, &mut remote).await;
    
    Ok(())
}
