use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use anyhow::Result;
use log::info;

/// Handler para protocolo xHTTP/Proto.
/// xHTTP é um transport que encapsula dados em requisições HTTP GET/POST,
/// simulando tráfego web normal. O proxy detecta o header xHTTP ou X-
/// e encaminha o payload para o backend SSH ou VPN.
pub async fn handle_xhttp(mut socket: TcpStream, status: &str, ssh_only: bool) -> Result<()> {
    info!("🔗 xHTTP/Proto handshake detectado");

    // Ler o request xHTTP do cliente (GET/POST com headers xHTTP)
    let mut buf = [0u8; 8192];
    let n = socket.read(&mut buf).await?;
    let payload_str = String::from_utf8_lossy(&buf[..n]).to_string();
    info!("xHTTP payload recebido ({} bytes): {:?}", n, &payload_str[..n.min(200)]);

    // Extrair headers xHTTP relevantes
    let _method = if payload_str.starts_with("GET") { "GET" } else { "POST" };
    let _host = extract_header_value(&payload_str, "Host");
    let _content_type = extract_header_value(&payload_str, "Content-Type");

    // Determinar backend: SSH ou VPN
    let addr_proxy = if ssh_only {
        "127.0.0.1:22"
    } else if payload_str.contains("SSH") || payload_str.contains("ssh") {
        "127.0.0.1:22"
    } else {
        "127.0.0.1:1194"
    };

    info!("xHTTP -> Backend: {}", addr_proxy);

    // Conectar ao backend com fallback
    let server_stream = connect_with_fallback(addr_proxy, ssh_only).await?;

    // Enviar resposta HTTP 101 (switching protocols)
    let response_101 = format!(
        "HTTP/1.1 101 Switching Protocols\r\n\
         Server: SDProxy\r\n\
         Connection: Upgrade\r\n\
         Upgrade: {}\r\n\
         X-Protocol: xHTTP\r\n\
         X-Status: {}\r\n\
         \r\n",
        if payload_str.contains("SSH") || payload_str.contains("ssh") { "ssh" } else { "vpn" },
        status
    );
    socket.write_all(response_101.as_bytes()).await?;
    socket.flush().await?;
    info!("xHTTP 101 enviado");

    // Enviar 200 OK (compatibilidade com alguns clientes)
    let response_200 = format!(
        "HTTP/1.1 200 OK\r\n\
         Server: SDProxy\r\n\
         Connection: keep-alive\r\n\
         Content-Type: application/octet-stream\r\n\
         X-Protocol: xHTTP\r\n\
         X-Status: {}\r\n\
         \r\n",
        status
    );
    socket.write_all(response_200.as_bytes()).await?;
    socket.flush().await?;
    info!("xHTTP 200 enviado");

    // Tunnel bidirecional
    let (client_r, client_w) = socket.into_split();
    let (server_r, server_w) = server_stream.into_split();
    let client_r = Arc::new(Mutex::new(client_r));
    let client_w = Arc::new(Mutex::new(client_w));
    let server_r = Arc::new(Mutex::new(server_r));
    let server_w = Arc::new(Mutex::new(server_w));

    info!("xHTTP tunnel bidirecional iniciado");
    tokio::try_join!(
        transfer_data(client_r, server_w.clone()),
        transfer_data(server_r, client_w.clone()),
    )?;
    info!("xHTTP tunnel finalizado");
    Ok(())
}

/// Handler para protocolo Proto (dados binários/diretos via TCP)
/// Proto encaminha dados brutos sem handshake HTTP, ideal para conexões
/// que não precisam de encapsulamento HTTP.
pub async fn handle_proto(mut socket: TcpStream, ssh_only: bool) -> Result<()> {
    info!("📦 Proto (TCP raw) detectado");

    let addr_proxy = if ssh_only {
        "127.0.0.1:22"
    } else {
        "127.0.0.1:22" // Proto geralmente vai para SSH
    };

    info!("Proto -> Backend: {}", addr_proxy);

    // Conectar ao backend com fallback
    let mut server_stream = connect_with_fallback(addr_proxy, ssh_only).await?;

    // Tunnel bidirecional direto (sem headers HTTP)
    tokio::io::copy_bidirectional(&mut socket, &mut server_stream).await?;
    info!("Proto tunnel finalizado");
    Ok(())
}

/// Extrai valor de um header HTTP pelo nome
fn extract_header_value<'a>(payload: &'a str, header_name: &str) -> Option<&'a str> {
    for line in payload.lines() {
        if line.to_lowercase().starts_with(&format!("{}:", header_name.to_lowercase())) {
            let value = line.split(':').nth(1)?.trim();
            return Some(value);
        }
    }
    None
}

/// Tenta conectar ao backend primário, com fallback para o alternativo
async fn connect_with_fallback(primary: &str, ssh_only: bool) -> Result<TcpStream> {
    match TcpStream::connect(primary).await {
        Ok(stream) => {
            info!("✅ Conectado ao backend: {}", primary);
            Ok(stream)
        }
        Err(e) => {
            info!("⚠️ Falha em {}: {}. Tentando fallback...", primary, e);
            if ssh_only {
                return Err(e.into());
            }
            let alt = if primary.contains(":22") {
                "127.0.0.1:1194"
            } else {
                "127.0.0.1:22"
            };
            match TcpStream::connect(alt).await {
                Ok(stream) => {
                    info!("✅ Fallback OK: {}", alt);
                    Ok(stream)
                }
                Err(e2) => {
                    info!("❌ Ambos falharam: {}, {}", e, e2);
                    Err(e2.into())
                }
            }
        }
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
