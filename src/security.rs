use tokio::io::{AsyncReadExt, AsyncWriteExt, copy_bidirectional};
use tokio::net::TcpStream;
use anyhow::Result;
use log::{info, debug};
use tokio::time::{timeout, Duration};

pub async fn handle_security(mut socket: TcpStream, status: &str) -> Result<()> {
    info!("🔐 SECURITY handshake (Tripla Resposta)...");
    
    // 1. Primeira Resposta: 101
    socket.write_all(format!("HTTP/1.1 101 {}\r\n\r\n", status).as_bytes()).await?;
    debug!("📤 Sent Response 1: 101 {}", status);

    // 2. Leitura do payload (headers do injetor)
    let mut buffer = [0u8; 1024];
    let _ = socket.read(&mut buffer).await?;
    
    // 3. Segunda Resposta: 200 com headers de Upgrade (baseado no snippet do usuário)
    let response2 = "HTTP/1.1 200 OK\r\n\
                    Connection: Upgrade\r\n\
                    Upgrade: security\r\n\
                    \r\n";
    socket.write_all(response2.as_bytes()).await?;
    debug!("📤 Sent Response 2: 200 OK (Upgrade)");

    // 4. Terceira Resposta: 200 Status (baseado no código funcional)
    socket.write_all(format!("HTTP/1.1 200 {}\r\n\r\n", status).as_bytes()).await?;
    debug!("📤 Sent Response 3: 200 {}", status);

    info!("🔐 SECURITY handshake complete!");
    
    // Detecção de backend (SSH vs VPN)
    let mut peek_buffer = [0u8; 1024];
    let addr_proxy = match timeout(Duration::from_millis(500), socket.peek(&mut peek_buffer)).await {
        Ok(Ok(n)) if n > 0 => {
            let data = String::from_utf8_lossy(&peek_buffer[..n]);
            if data.contains("SSH") || data.is_empty() { "127.0.0.1:22" } else { "127.0.0.1:1194" }
        },
        _ => "127.0.0.1:22",
    };

    info!("🔗 Conectando ao backend: {}", addr_proxy);
    let mut remote = match TcpStream::connect(addr_proxy).await {
        Ok(s) => s,
        Err(_) => TcpStream::connect("127.0.0.1:22").await?,
    };

    info!("✅ SECURITY Túnel iniciado.");
    let _ = copy_bidirectional(&mut socket, &mut remote).await;
    
    Ok(())
}
