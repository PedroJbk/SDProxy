use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::time::{timeout, Duration};
use anyhow::Result;
use std::collections::HashMap;

use tokio_rustls::rustls::{self, Certificate, PrivateKey};
use tokio_rustls::TlsAcceptor;

/// Sessão xHTTP ativa
struct XhttpSession {
    ssh_write: Arc<Mutex<tokio::net::tcp::OwnedWriteHalf>>,
    ssh_read: Arc<Mutex<tokio::net::tcp::OwnedReadHalf>>,
    active: bool,
}

static SESSIONS: once_cell::sync::Lazy<Arc<Mutex<HashMap<String, XhttpSession>>>> =
    once_cell::sync::Lazy::new(|| Arc::new(Mutex::new(HashMap::new())));

/// Handler TLS + xHTTP para porta 443
pub async fn handle_tls_with_xhttp(
    mut stream: TcpStream,
    status: &str,
    _ssh_only: &bool,
) -> Result<()> {
    println!("[TLS+xHTTP] Nova conexão TLS na porta 443");

    // Ler o TLS ClientHello completo
    let mut tls_buf = vec![0u8; 16384];
    let n = match timeout(Duration::from_secs(10), stream.read(&mut tls_buf)).await {
        Ok(Ok(n)) => n,
        _ => {
            println!("[TLS+xHTTP] Timeout na leitura TLS");
            return Ok(());
        }
    };

    println!("[TLS+xHTTP] TLS recebido ({} bytes)", n);

    if n < 2 || tls_buf[0] != 0x16 {
        println!("[TLS+xHTTP] Não é TLS válido");
        return Ok(());
    }

    // Tentar TLS termination com cert auto-assinado
    let cert_path = "/opt/sdproxy/cert.pem";
    let key_path = "/opt/sdproxy/key.pem";

    let config = match build_tls_config(cert_path, key_path) {
        Ok(c) => c,
        Err(e) => {
            println!("[TLS+xHTTP] Erro TLS config: {}. Fallback HTTP direto...", e);
            return handle_http_request_from_buf(stream, &tls_buf[..n], status).await;
        }
    };

    let acceptor = TlsAcceptor::from(Arc::new(config));

    let tls_stream = match acceptor.accept(stream).await {
        Ok(s) => s,
        Err(e) => {
            println!("[TLS+xHTTP] TLS handshake falhou: {}", e);
            return Ok(());
        }
    };

    println!("[TLS+xHTTP] TLS handshake OK");

    // Dividir o TLS stream para ler HTTP
    let (mut tls_read, mut tls_write) = tokio::io::split(tls_stream);

    // Ler a requisição HTTP
    let mut http_buf = vec![0u8; 65536];
    let http_n = match timeout(Duration::from_secs(15), tls_read.read(&mut http_buf)).await {
        Ok(Ok(n)) => n,
        _ => {
            println!("[TLS+xHTTP] Timeout lendo HTTP");
            return Ok(());
        }
    };

    println!("[TLS+xHTTP] HTTP recebido ({} bytes)", http_n);

    let http_str = String::from_utf8_lossy(&http_buf[..http_n]).to_string();

    // Parsear method e path
    let (method, path) = match parse_http_request(&http_str) {
        Some(m) => m,
        None => {
            println!("[TLS+xHTTP] Falha ao parsear HTTP");
            let resp = "HTTP/1.1 400 Bad Request\r\n\r\n";
            let _ = tls_write.write_all(resp.as_bytes()).await;
            return Ok(());
        }
    };

    println!("[TLS+xHTTP] Method: {} Path: {}", method, path);

    // Re-juntar read/write
    let tls_combined = tls_read.unsplit(tls_write);

    match method.as_str() {
        "GET" => {
            handle_xhttp_get(tls_combined, &path, status).await
        }
        "POST" => {
            handle_xhttp_post(tls_combined, &http_str, &path, status).await
        }
        _ => {
            let resp = format!("HTTP/1.1 405 Method Not Allowed\r\nX-Status: {}\r\n\r\n", status);
            let mut s = tls_combined;
            s.write_all(resp.as_bytes()).await?;
            Ok(())
        }
    }
}

/// Handle xHTTP GET - Streaming downlink
async fn handle_xhttp_get(
    mut stream: impl AsyncReadExt + AsyncWriteExt + Unpin,
    path: &str,
    status: &str,
) -> Result<()> {
    let session_id = extract_session_id(path);
    println!("[xHTTP GET] session_id: {}", session_id);

    if session_id.is_empty() {
        let resp = "HTTP/1.1 404 Not Found\r\n\r\n";
        stream.write_all(resp.as_bytes()).await?;
        return Ok(());
    }

    // Conectar ao SSH backend
    let ssh_stream = match TcpStream::connect("127.0.0.1:22").await {
        Ok(s) => s,
        Err(e) => {
            println!("[xHTTP GET] SSH falhou: {}", e);
            let resp = format!("HTTP/1.1 502 Bad Gateway\r\nX-Status: {}\r\n\r\n", status);
            stream.write_all(resp.as_bytes()).await?;
            return Ok(());
        }
    };

    let (ssh_r, ssh_w) = ssh_stream.into_split();
    let ssh_r = Arc::new(Mutex::new(ssh_r));
    let ssh_w = Arc::new(Mutex::new(ssh_w));

    // Salvar sessão
    {
        let mut sessions = SESSIONS.lock().await;
        sessions.insert(session_id.clone(), XhttpSession {
            ssh_write: ssh_w.clone(),
            ssh_read: ssh_r.clone(),
            active: true,
        });
    }

    // Enviar HTTP response com streaming chunked
    let response = format!(
        "HTTP/1.1 200 OK\r\n\
         Content-Type: application/octet-stream\r\n\
         Transfer-Encoding: chunked\r\n\
         Cache-Control: no-cache\r\n\
         Connection: keep-alive\r\n\
         X-Session: {}\r\n\
         X-Status: {}\r\n\r\n",
        session_id, status
    );

    stream.write_all(response.as_bytes()).await?;
    stream.flush().await?;
    println!("[xHTTP GET] Response enviada para session {}", session_id);

    // Ler dados do SSH e enviar como chunks
    loop {
        let data = {
            let mut read_guard = ssh_r.lock().await;
            let mut buf = [0u8; 4096];
            match timeout(Duration::from_secs(60), read_guard.read(&mut buf)).await {
                Ok(Ok(0)) => break,
                Ok(Ok(n)) => Some(buf[..n].to_vec()),
                Ok(Err(e)) => {
                    println!("[xHTTP GET] Erro lendo SSH: {}", e);
                    break;
                }
                Err(_) => None, // timeout - keepalive
            }
        };

        match data {
            Some(chunk) => {
                let chunk_header = format!("{:x}\r\n", chunk.len());
                stream.write_all(chunk_header.as_bytes()).await?;
                stream.write_all(&chunk).await?;
                stream.write_all(b"\r\n").await?;
                stream.flush().await?;
            }
            None => {
                // Keepalive
                let _ = stream.write_all(b"0\r\n\r\n").await;
                let _ = stream.flush().await;
            }
        }
    }

    // Limpar sessão
    {
        let mut sessions = SESSIONS.lock().await;
        sessions.remove(&session_id);
    }

    println!("[xHTTP GET] Streaming encerrado para session {}", session_id);
    Ok(())
}

/// Handle xHTTP POST - Uplink
async fn handle_xhttp_post(
    mut stream: impl AsyncReadExt + AsyncWriteExt + Unpin,
    full_request: &str,
    path: &str,
    status: &str,
) -> Result<()> {
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    let session_id = if parts.len() >= 2 { parts[1].to_string() } else { String::new() };

    println!("[xHTTP POST] session_id: {}", session_id);

    let content_length = extract_content_length(full_request).unwrap_or(0);

    if content_length == 0 {
        let resp = format!("HTTP/1.1 200 OK\r\nX-Status: {}\r\nContent-Length: 0\r\n\r\n", status);
        stream.write_all(resp.as_bytes()).await?;
        return Ok(());
    }

    // Ler o body do POST
    let mut body_buf = vec![0u8; content_length];
    match timeout(Duration::from_secs(30), stream.read_exact(&mut body_buf)).await {
        Ok(Ok(_)) => {
            println!("[xHTTP POST] Body recebido: {} bytes", body_buf.len());

            let mut sessions = SESSIONS.lock().await;
            if let Some(session) = sessions.get(&session_id) {
                if session.active {
                    let mut write_guard = session.ssh_write.lock().await;
                    match write_guard.write_all(&body_buf).await {
                        Ok(_) => {
                            let resp = format!("HTTP/1.1 200 OK\r\nX-Status: {}\r\nContent-Length: 0\r\n\r\n", status);
                            stream.write_all(resp.as_bytes()).await?;
                        }
                        Err(e) => {
                            println!("[xHTTP POST] Erro enviando ao SSH: {}", e);
                            let resp = format!("HTTP/1.1 500 Internal Server Error\r\nX-Status: {}\r\n\r\n", status);
                            stream.write_all(resp.as_bytes()).await?;
                        }
                    }
                } else {
                    let resp = format!("HTTP/1.1 410 Gone\r\nX-Status: {}\r\n\r\n", status);
                    stream.write_all(resp.as_bytes()).await?;
                }
            } else {
                println!("[xHTTP POST] Sessão {} não encontrada", session_id);
                let resp = format!("HTTP/1.1 404 Not Found\r\nX-Status: {}\r\n\r\n", status);
                stream.write_all(resp.as_bytes()).await?;
            }
        }
        _ => {
            println!("[xHTTP POST] Timeout ou erro lendo body");
            let resp = format!("HTTP/1.1 408 Request Timeout\r\nX-Status: {}\r\n\r\n", status);
            stream.write_all(resp.as_bytes()).await?;
        }
    }

    Ok(())
}

/// Fallback: tratar buffer como HTTP direto (quando TLS falha)
async fn handle_http_request_from_buf(
    mut stream: impl AsyncReadExt + AsyncWriteExt + Unpin,
    buf: &[u8],
    status: &str,
) -> Result<()> {
    let http_str = String::from_utf8_lossy(buf);
    let (method, path) = match parse_http_request(&http_str) {
        Some(m) => m,
        None => return Ok(()),
    };

    match method.as_str() {
        "GET" => handle_xhttp_get(stream, &path, status).await,
        "POST" => handle_xhttp_post(stream, &http_str, &path, status).await,
        _ => Ok(()),
    }
}

// === Helper functions ===

fn parse_http_request(data: &str) -> Option<(String, String)> {
    let first_line = data.lines().next()?;
    let parts: Vec<&str> = first_line.split_whitespace().collect();
    if parts.len() >= 2 {
        Some((parts[0].to_string(), parts[1].to_string()))
    } else {
        None
    }
}

fn extract_session_id(path: &str) -> String {
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    if parts.len() >= 2 {
        parts[1].to_string()
    } else if parts.len() == 1 && !parts[0].is_empty() {
        parts[0].to_string()
    } else {
        String::new()
    }
}

fn extract_content_length(data: &str) -> Option<usize> {
    for line in data.lines() {
        let lower = line.to_lowercase();
        if lower.starts_with("content-length:") {
            return line.split(':').nth(1)?.trim().parse().ok();
        }
    }
    None
}

fn build_tls_config(cert_path: &str, key_path: &str) -> Result<rustls::ServerConfig> {
    let cert_file = std::fs::File::open(cert_path)?;
    let key_file = std::fs::File::open(key_path)?;
    let mut cert_file = std::io::BufReader::new(cert_file);
    let mut key_file = std::io::BufReader::new(key_file);

    let certs: Vec<Certificate> = rustls_pemfile::certs(&mut cert_file)?
        .into_iter()
        .map(Certificate)
        .collect();

    let keys: Vec<PrivateKey> = rustls_pemfile::pkcs8_private_keys(&mut key_file)?
        .into_iter()
        .map(PrivateKey)
        .collect();

    if certs.is_empty() || keys.is_empty() {
        return Err(anyhow::anyhow!("Certs ou keys vazios"));
    }

    let config = rustls::ServerConfig::builder()
        .with_safe_defaults()
        .with_no_client_auth()
        .with_single_cert(certs, keys.into_iter().next().unwrap())?;

    Ok(config)
}
