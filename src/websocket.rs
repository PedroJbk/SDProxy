use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use anyhow::Result;
use log::info;

/// Handler para WebSocket com suporte a xHTTP/Proto.
/// Detecta se é WebSocket puro ou xHTTP upgrade e responde adequadamente.
pub async fn handle_websocket(mut socket: TcpStream, status: &str) -> Result<()> {
    info!("WebSocket/xHTTP handshake...");

    let mut buf = [0u8; 8192];
    let n = socket.read(&mut buf).await?;
    info!("Payload recebido ({} bytes)", n);

    let payload_str = String::from_utf8_lossy(&buf[..n]);

    // Detectar se é xHTTP upgrade ou WebSocket puro
    let is_xhttp = payload_str.contains("xHTTP") || payload_str.contains("XHTTP") ||
                   payload_str.contains("X-Proto") || payload_str.contains("X-Split");
    let is_proto = payload_str.contains("X-Proto") || payload_str.contains("x-proto");

    // Determinar backend
    let addr_proxy = if payload_str.contains("SSH") || payload_str.contains("ssh") {
        "127.0.0.1:22"
    } else {
        "127.0.0.1:1194"
    };

    info!("Conectando ao backend: {}", addr_proxy);
    let server_stream = connect_with_fallback(addr_proxy).await?;

    if is_xhttp || is_proto {
        // Resposta xHTTP/Proto - headers customizados
        let response = format!(
            "HTTP/1.1 101 {}\r\n\
             Server: SDProxy\r\n\
             Connection: Upgrade\r\n\
             Upgrade: {}\r\n\
             X-Protocol: {}\r\n\
             X-Status: {}\r\n\
             X-Backend: {}\r\n\
             \r\n",
            status,
            if is_proto { "proto" } else { "xhttp" },
            if is_proto { "proto" } else { "xhttp" },
            status,
            if addr_proxy.contains(":22") { "ssh" } else { "vpn" }
        );
        socket.write_all(response.as_bytes()).await?;
        socket.flush().await?;
        info!("xHTTP/Proto 101 enviado");
    } else {
        // WebSocket upgrade padrão com suporte a xHTTP
        // Extrair Sec-WebSocket-Key do request
        let ws_key = extract_ws_key(&payload_str);

        let upgrade_headers = format!(
            "HTTP/1.1 101 {}\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Accept: {}\r\n\
             X-Protocol: websocket\r\n\
             X-Status: {}\r\n\
             \r\n",
            status,
            compute_ws_accept(&ws_key),
            status
        );
        socket.write_all(upgrade_headers.as_bytes()).await?;
        socket.flush().await?;
        info!("WebSocket 101 enviado (Accept: {})", compute_ws_accept(&ws_key));
    }

    // Enviar 200 OK adicional (compatibilidade)
    let response_200 = format!("HTTP/1.1 200 {}\r\n\r\n", status);
    socket.write_all(response_200.as_bytes()).await?;
    socket.flush().await?;

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
    info!("Tunnel finalizado");
    Ok(())
}

/// Extrai a chave Sec-WebSocket-Key do request
fn extract_ws_key(payload: &str) -> String {
    for line in payload.lines() {
        if line.to_lowercase().starts_with("sec-websocket-key:") {
            return line.split(':').nth(1).unwrap_or("").trim().to_string();
        }
    }
    "dGhlIHNhbXBsZSBub25jZQ==".to_string() // default key
}

/// Calcula o Sec-WebSocket-Accept a partir da chave
fn compute_ws_accept(key: &str) -> String {
    let mut hasher_input = String::from(key);
    hasher_input.push_str("258EAFA5-E914-47DA-95CA-C5AB0DC85B11");

    // SHA-1 + Base64 encoding
    let hash = sha1_simple(hasher_input.as_bytes());
    base64_encode(&hash)
}

/// SHA-1 simples (implementação minimalista)
fn sha1_simple(data: &[u8]) -> [u8; 20] {
    let mut h0: u32 = 0x67452301;
    let mut h1: u32 = 0xEFCDAB89;
    let mut h2: u32 = 0x98BADCFE;
    let mut h3: u32 = 0x10325476;
    let mut h4: u32 = 0xC3D2E1F0;

    // Pad message
    let orig_len = data.len() as u64;
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0x00);
    }
    msg.extend_from_slice(&(orig_len * 8).to_be_bytes());

    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([chunk[i*4], chunk[i*4+1], chunk[i*4+2], chunk[i*4+3]]);
        }
        for i in 16..80 {
            w[i] = (w[i-3] ^ w[i-8] ^ w[i-14] ^ w[i-16]).rotate_left(1);
        }

        let (mut a, mut b, mut c, mut d, mut e) = (h0, h1, h2, h3, h4);

        for i in 0..80 {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A827999u32),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1u32),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDCu32),
                _ => (b ^ c ^ d, 0xCA62C1D6u32),
            };
            let temp = a.rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(w[i]);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }

        h0 = h0.wrapping_add(a);
        h1 = h1.wrapping_add(b);
        h2 = h2.wrapping_add(c);
        h3 = h3.wrapping_add(d);
        h4 = h4.wrapping_add(e);
    }

    let mut result = [0u8; 20];
    result[0..4].copy_from_slice(&h0.to_be_bytes());
    result[4..8].copy_from_slice(&h1.to_be_bytes());
    result[8..12].copy_from_slice(&h2.to_be_bytes());
    result[12..16].copy_from_slice(&h3.to_be_bytes());
    result[16..20].copy_from_slice(&h4.to_be_bytes());
    result
}

/// Base64 encoding simples
fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;

        result.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            result.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(CHARS[(triple & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

/// Conectar ao backend com fallback
async fn connect_with_fallback(primary: &str) -> Result<TcpStream> {
    match TcpStream::connect(primary).await {
        Ok(s) => {
            info!("✅ Backend conectado: {}", primary);
            Ok(s)
        }
        Err(e) => {
            let alt = if primary.contains(":22") { "127.0.0.1:1194" } else { "127.0.0.1:22" };
            info!("Falha em {}: {}. Tentando {}", primary, e, alt);
            match TcpStream::connect(alt).await {
                Ok(s) => {
                    info!("✅ Fallback OK: {}", alt);
                    Ok(s)
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
