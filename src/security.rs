use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use anyhow::Result;
use log::info;

/// Handle SECURITY - SEGUE O PADRÃO EXATO DO BSPROXY:
/// 1. SEMPRE envia 101 primeiro
/// 2. SEMPRE lê do cliente
/// 3. SEMPRE envia 200
/// 4. Depois detecta SSH vs VPN pelo payload
pub async fn handle_security(mut socket: TcpStream, status: &str) -> Result<()> {
    info!("🔐 SECURITY handshake (padrão BSProxy)...");

    // PASSO 1: SEMPRE envia 101 Switching Protocols primeiro
    let response_101 = format!("HTTP/1.1 101 {}\r\n\r\n", status);
    socket.write_all(response_101.as_bytes()).await?;
    socket.flush().await?;
    info!("📤 Enviado: 101 {}", status);

    // PASSO 2: SEMPRE lê do cliente (payload do Injector)
    let mut buf = [0u8; 256];
    let n = socket.read(&mut buf).await?;
    let payload = String::from_utf8_lossy(&buf[..n]);
    info!("📩 SECURITY payload: {}", payload.trim());

    // PASSO 3: SEMPRE envia 200 OK
    let response_200 = format!("HTTP/1.1 200 {}\r\n\r\n", status);
    socket.write_all(response_200.as_bytes()).await?;
    socket.flush().await?;
    info!("📤 Enviado: 200 {}", status);

    // PASSO 4: Detecta backend pelo payload - SSH vs VPN
    let addr_proxy = if payload.contains("SSH") || payload.is_empty() {
        "127.0.0.1:22"
    } else {
        "127.0.0.1:1194"
    };

    info!("🔗 Conectando ao backend: {}", addr_proxy);

    let server_connect = TcpStream::connect(addr_proxy).await;
    if server_connect.is_err() {
        let alt = if addr_proxy == "127.0.0.1:22" { "127.0.0.1:1194" } else { "127.0.0.1:22" };
        info!("⚠️ Falha em {}, tentando {}", addr_proxy, alt);
        match TcpStream::connect(alt).await {
            Ok(s) => {
                info!("✅ SECURITY túnel iniciado para {}", alt);
                let (cr, cw) = socket.into_split();
                let (sr, sw) = s.into_split();
                let cr = Arc::new(Mutex::new(cr));
                let cw = Arc::new(Mutex::new(cw));
                let sr = Arc::new(Mutex::new(sr));
                let sw = Arc::new(Mutex::new(sw));
                tokio::try_join!(transfer_data(cr, sw), transfer_data(sr, cw))?;
                info!("🔚 SECURITY túnel finalizado.");
                Ok(())
            }
            Err(e) => {
                info!("❌ Ambos backends falharam: {}", e);
                Ok(())
            }
        }
    } else {
        let server_stream = server_connect?;
        info!("✅ SECURITY túnel iniciado para {}", addr_proxy);
        let (cr, cw) = socket.into_split();
        let (sr, sw) = server_stream.into_split();
        let cr = Arc::new(Mutex::new(cr));
        let cw = Arc::new(Mutex::new(cw));
        let sr = Arc::new(Mutex::new(sr));
        let sw = Arc::new(Mutex::new(sw));
        tokio::try_join!(transfer_data(cr, sw), transfer_data(sr, cw))?;
        info!("🔚 SECURITY túnel finalizado.");
        Ok(())
    }
}

async fn transfer_data(
    read_stream: Arc<Mutex<tokio::net::tcp::OwnedReadHalf>>,
    write_stream: Arc<Mutex<tokio::net::tcp::OwnedWriteHalf>>,
) -> Result<()> {
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
