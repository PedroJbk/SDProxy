use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use anyhow::Result;
use log::info;

/// Lê e descarta os headers HTTP até encontrar \r\n\r\n ou \n\n (comum em payloads do HTTP Injector)
async fn consume_http_headers(socket: &mut TcpStream) -> std::io::Result<()> {
    let mut buf: Vec<u8> = Vec::new();
    let mut tmp = [0u8; 1];

    loop {
        socket.read_exact(&mut tmp).await?;
        buf.push(tmp[0]);

        // Detectar \r\n\r\n
        if buf.len() >= 4 && &buf[buf.len() - 4..] == b"\r\n\r\n" {
            break;
        }
        // Detectar \n\n (payloads customizados)
        if buf.len() >= 2 && &buf[buf.len() - 2..] == b"\n\n" {
            break;
        }
        if buf.len() > 8192 {
            break;
        }
    }
    Ok(())
}

pub async fn handle_websocket(mut socket: TcpStream) -> Result<()> {
    info!("🌐 WebSocket/HTTP handshake...");
    
    // Consumir headers HTTP
    consume_http_headers(&mut socket).await?;
    
    // Resposta solicitada: 200 OK seguido de 101 Switching Protocols
    // Alguns injectores esperam o 200 OK antes do upgrade
    let response_200 = "HTTP/1.1 200 OK\r\n\r\n";
    socket.write_all(response_200.as_bytes()).await?;

    // Resposta de upgrade WebSocket (101 Switching Protocols)
    let response_101 = "HTTP/1.1 101 Switching Protocols\r\n\
                        Upgrade: websocket\r\n\
                        Connection: Upgrade\r\n\
                        Sec-WebSocket-Accept: dGhlIHNhbXBsZSBub25jZQ==\r\n\
                        \r\n";
    
    socket.write_all(response_101.as_bytes()).await?;
    info!("🌐 WebSocket handshake complete!");
    
    // Encaminhar para SSH local
    let target = "127.0.0.1:22";
    
    match TcpStream::connect(target).await {
        Ok(remote) => {
            info!("✅ Conectado ao SSH na porta 22");
            let (mut client_reader, mut client_writer) = socket.into_split();
            let (mut remote_reader, mut remote_writer) = remote.into_split();
            
            tokio::try_join!(
                tokio::io::copy(&mut client_reader, &mut remote_writer),
                tokio::io::copy(&mut remote_reader, &mut client_writer)
            )?;
            
            info!("🔚 Conexão WebSocket->SSH encerrada");
            Ok(())
        }
        Err(e) => {
            info!("❌ Falha ao conectar ao SSH: {}", e);
            // Tentar VPN como fallback
            match TcpStream::connect("127.0.0.1:1194").await {
                Ok(remote) => {
                    info!("✅ WebSocket -> VPN conectado");
                    let (mut client_reader, mut client_writer) = socket.into_split();
                    let (mut remote_reader, mut remote_writer) = remote.into_split();
                    
                    tokio::try_join!(
                        tokio::io::copy(&mut client_reader, &mut remote_writer),
                        tokio::io::copy(&mut remote_reader, &mut client_writer)
                    )?;
                    
                    Ok(())
                }
                Err(e2) => {
                    anyhow::bail!("WebSocket connection failed: SSH={}, VPN={}", e, e2)
                }
            }
        }
    }
}
