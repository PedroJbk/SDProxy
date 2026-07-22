use tokio::io::{AsyncReadExt, AsyncWriteExt, copy_bidirectional};
use tokio::net::TcpStream;
use anyhow::Result;
use log::info;

/// Consome os headers de forma segura sem travar
async fn consume_http_headers(socket: &mut TcpStream) -> std::io::Result<()> {
    let mut buf = [0u8; 1];
    let mut consecutive_newlines = 0;

    // Lemos byte a byte até encontrar o fim dos headers (\n\n ou \r\n\r\n)
    // Isso é mais lento mas 100% seguro para payloads customizados
    while consecutive_newlines < 2 {
        let n = socket.read(&mut buf).await?;
        if n == 0 { break; }
        
        if buf[0] == b'\n' {
            consecutive_newlines += 1;
        } else if buf[0] != b'\r' {
            consecutive_newlines = 0;
        }
    }
    Ok(())
}

pub async fn handle_websocket(mut socket: TcpStream) -> Result<()> {
    info!("🌐 WebSocket handshake...");
    
    // Consumir os headers da requisição
    let _ = consume_http_headers(&mut socket).await;
    
    // Resposta padrão 101. Removi o 200 OK extra para evitar travamento em alguns apps.
    // O 101 já implica sucesso na maioria dos protocolos de upgrade.
    let response = "HTTP/1.1 101 Switching Protocols\r\n\
                    Upgrade: websocket\r\n\
                    Connection: Upgrade\r\n\r\n";
    
    socket.write_all(response.as_bytes()).await?;
    
    // Conectar ao SSH local
    let mut remote = match TcpStream::connect("127.0.0.1:22").await {
        Ok(s) => s,
        Err(_) => {
            // Fallback para VPN se SSH falhar
            TcpStream::connect("127.0.0.1:1194").await?
        }
    };

    info!("✅ Túnel iniciado.");
    
    // O segredo para não travar: copy_bidirectional direto sem split complexo
    let _ = copy_bidirectional(&mut socket, &mut remote).await;
    
    Ok(())
}
