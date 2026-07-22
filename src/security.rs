use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use anyhow::Result;
use log::info;

/// Handle SECURITY - PADRAO BSProxy SIMPLIFICADO
pub async fn handle_security(mut socket: TcpStream, status: &str) -> Result<()> {
    info!("SECURITY handshake...");

    // Ler payload do Injector
    let mut buf = [0u8; 8192];
    let n = socket.read(&mut buf).await?;
    info!("Payload recebido ({} bytes)", n);

    // Detectar backend
    let payload_str = String::from_utf8_lossy(&buf[..n]);
    let addr_proxy = if payload_str.contains("SSH") || payload_str.contains("ssh") {
        "127.0.0.1:22"
    } else {
        "127.0.0.1:1194"
    };

    // Conectar ao backend PRIMEIRO
    info!("Conectando ao backend: {}", addr_proxy);
    let server_stream = match TcpStream::connect(addr_proxy).await {
        Ok(s) => s,
        Err(e) => {
            let alt = if addr_proxy == "127.0.0.1:22" { "127.0.0.1:1194" } else { "127.0.0.1:22" };
            info!("Falha em {}: {}. Tentando {}", addr_proxy, e, alt);
            match TcpStream::connect(alt).await {
                Ok(s) => s,
                Err(e2) => {
                    info!("Ambos falharam: {}, {}", e, e2);
                    return Ok(());
                }
            }
        }
    };

    // Enviar 101 ao Injector
    let response_101 = format!("HTTP/1.1 101 {}\r\n\r\n", status);
    socket.write_all(response_101.as_bytes()).await?;
    socket.flush().await?;
    info!("Enviado: 101 {}", status);

    // Enviar 200 ao Injector
    let response_200 = format!("HTTP/1.1 200 {}\r\n\r\n", status);
    socket.write_all(response_200.as_bytes()).await?;
    socket.flush().await?;
    info!("Enviado: 200 {}", status);

    // Tunnel bidirecional
    let (client_r, client_w) = socket.into_split();
    let (server_r, server_w) = server_stream.into_split();

    let client_r = Arc::new(Mutex::new(client_r));
    let client_w = Arc::new(Mutex::new(client_w));
    let server_r = Arc::new(Mutex::new(server_r));
    let server_w = Arc::new(Mutex::new(server_w));

    info!("Tunnel bidirecional iniciado");
    tokio::try_join!(
        transfer_data(client_r, server_w.clone()),
        transfer_data(server_r, client_w.clone()),
    )?;

    info!("Tunnel finalizado.");
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
