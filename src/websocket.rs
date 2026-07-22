use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use anyhow::Result;
use log::info;

/// Handle WebSocket/HTTP - SEGUE O PADRÃO EXATO DO HTTP INJECTOR com [split]:
/// 1. Recebe parte 1 do payload (antes do [split])
/// 2. Envia 101
/// 3. Recebe parte 2 do payload (depois do [split])
/// 4. Envia 200
/// 5. Conecta ao backend
/// 6. Tunnel bidirecional
pub async fn handle_websocket(mut socket: TcpStream, status: &str) -> Result<()> {
    info!("🌐 WebSocket/HTTP handshake (padrão HTTP Injector [split])...");

    // PASSO 1: Recebe parte 1 do payload (antes do [split])
    let mut buf = [0u8; 4096];
    let n1 = socket.read(&mut buf).await?;
    let part1 = String::from_utf8_lossy(&buf[..n1]);
    info!("📥 Parte 1 recebida ({} bytes): {:?}", n1, &part1[..std::cmp::min(n1, 200)]);

    // PASSO 2: Envia 101 Switching Protocols (resposta à parte 1)
    let response_101 = format!("HTTP/1.1 101 {}\r\n\r\n", status);
    socket.write_all(response_101.as_bytes()).await?;
    socket.flush().await?;
    info!("📤 Enviado: 101 {}", status);

    // PASSO 3: Recebe parte 2 do payload (depois do [split])
    let mut buf2 = [0u8; 4096];
    let n2 = socket.read(&mut buf2).await?;
    let part2 = String::from_utf8_lossy(&buf2[..n2]);
    info!("📥 Parte 2 recebida ({} bytes): {:?}", n2, &part2[..std::cmp::min(n2, 200)]);

    // Detecta backend pelo payload completo
    let full_payload = format!("{}{}", part1, part2);
    let addr_proxy = if full_payload.contains("SSH") {
        "127.0.0.1:22"
    } else {
        "127.0.0.1:1194"
    };

    // PASSO 4: Envia 200 OK (resposta à parte 2)
    let response_200 = format!("HTTP/1.1 200 {}\r\n\r\n", status);
    socket.write_all(response_200.as_bytes()).await?;
    socket.flush().await?;
    info!("📤 Enviado: 200 {}", status);

    // PASSO 5: Conecta ao backend
    info!("🔗 Conectando ao backend: {}", addr_proxy);
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

    // PASSO 6: Tunnel bidirecional
    let (client_r, client_w) = socket.into_split();
    let (server_r, server_w) = server_stream.into_split();

    let client_r = Arc::new(Mutex::new(client_r));
    let client_w = Arc::new(Mutex::new(client_w));
    let server_r = Arc::new(Mutex::new(server_r));
    let server_w = Arc::new(Mutex::new(server_w));

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
