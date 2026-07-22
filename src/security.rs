use tokio::io::{AsyncReadExt, AsyncWriteExt, copy_bidirectional};
use tokio::net::TcpStream;
use anyhow::Result;
use log::info;

pub async fn handle_security(mut socket: TcpStream) -> Result<()> {
    info!("🔐 SECURITY handshake...");
    
    let mut buf = [0u8; 256];
    let n = socket.read(&mut buf).await?;
    let data = String::from_utf8_lossy(&buf[..n]);
    
    info!("📩 SECURITY: {}", data);
    
    let response = "HTTP/1.1 200 OK\r\n\
                    Connection: Upgrade\r\n\
                    Upgrade: security\r\n\
                    \r\n";
    
    socket.write_all(response.as_bytes()).await?;
    info!("🔐 SECURITY handshake complete!");
    
    // Encaminhar para SSH após handshake SECURITY
    match TcpStream::connect("127.0.0.1:22").await {
        Ok(mut remote) => {
            info!("✅ SECURITY -> SSH conectado");
            let _ = copy_bidirectional(&mut socket, &mut remote).await;
            info!("🔚 Conexão SECURITY->SSH encerrada");
            Ok(())
        }
        Err(e) => {
            info!("❌ Falha ao conectar ao SSH: {}", e);
            Err(e.into())
        }
    }
}
