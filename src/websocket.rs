use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use anyhow::Result;
use log::info;

/// Handle WebSocket/HTTP - SEGUE O PADRÃO EXATO DO BSPROXY:
/// 1. SEMPRE envia 101 primeiro
/// 2. SEMPRE lê do cliente
/// 3. SEMPRE envia 200
/// 4. Conecta ao backend
/// 5. ENCAMINHA O PAYLOAD ao backend
/// 6. Faz tunnel bidirecional
pub async fn handle_websocket(mut socket: TcpStream, status: &str) -> Result<()> {
    info!("🌐 WebSocket/HTTP handshake (padrão BSProxy)...");

    // PASSO 1: SEMPRE envia 101 Switching Protocols primeiro
    let response_101 = format!("HTTP/1.1 101 {}\r\n\r\n", status);
    socket.write_all(response_101.as_bytes()).await?;
    socket.flush().await?;
    info!("📤 Enviado: 101 {}", status);

    // PASSO 2: SEMPRE lê do cliente (payload do Injector)
    let mut buf = [0u8; 4096];
    let n = socket.read(&mut buf).await?;
    let payload = String::from_utf8_lossy(&buf[..n]);
    info!("📩 Payload recebido ({} bytes): {}", n, payload.trim());

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

    // PASSO 5: Conecta ao backend
    let server_stream = match TcpStream::connect(addr_proxy).await {
        Ok(s) => s,
        Err(e) => {
            let alt = if addr_proxy == "127.0.0.1:22" { "127.0.0.1:1194" } else { "127.0.0.1:22" };
            info!("⚠️ Falha em {}: {}. Tentando {}", addr_proxy, e, alt);
            match TcpStream::connect(alt).await {
                Ok(s) => {
                    info!("✅ Conectado ao fallback: {}", alt);
                    s
                }
                Err(e2) => {
                    info!("❌ Ambos backends falharam: {}, {}", e, e2);
                    return Ok(());
                }
            }
        }
    };

    info!("✅ Conectado ao backend: {}", addr_proxy);

    // PASSO 5b: ENCAMINHAR O PAYLOAD ao backend (CRUCIAL!)
    let (mut client_r, mut client_w) = socket.into_split();
    let (mut server_r, mut server_w) = server_stream.into_split();

    // Envia o payload já lido ao backend
    server_w.write_all(&buf[..n]).await?;
    server_w.flush().await?;
    info!("📤 Payload encaminhado ao backend ({} bytes)", n);

    let client_r = Arc::new(Mutex::new(client_r));
    let client_w = Arc::new(Mutex::new(client_w));
    let server_r = Arc::new(Mutex::new(server_r));
    let server_w = Arc::new(Mutex::new(server_w));

    // PASSO 6: Tunnel bidirecional
    info!("🔗 Túnel bidirecional iniciado");
    tokio::try_join!(
        transfer_data(client_r, server_w.clone()),
        transfer_data(server_r, client_w.clone()),
    )?;

    info!("🔚 Túnel finalizado.");
    Ok(())
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
