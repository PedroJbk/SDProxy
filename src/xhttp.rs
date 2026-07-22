use std::collections::HashMap;
use std::io::Error;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tokio::time::{timeout, Duration};

use tokio_rustls::rustls::{self, Certificate, PrivateKey};
use tokio_rustls::TlsAcceptor;

/// Sessão xHTTP ativa
struct XhttpSession {
    ssh_write: Arc<Mutex<tokio::net::tcp::OwnedWriteHalf>>,
    ssh_read: Arc<Mutex<tokio::net::tcp::OwnedReadHalf>>,
}

static SESSIONS: once_cell::sync::Lazy<Arc<Mutex<HashMap<String, XhttpSession>>>> =
    once_cell::sync::Lazy::new(|| Arc::new(Mutex::new(HashMap::new())));

/// Iniciar listener xHTTP na porta 443
pub async fn start_xhttp_listener(port: u16, status: String) -> Result<(), Error> {
    let listener = TcpListener::bind(format!("[::]:{}", port)).await?;
    println!("[xHTTP] Servico xHTTP SplitHTTP rodando na porta: {}", port);
    println!("[xHTTP] Status: {}", status);
    println!("[xHTTP] Aguardando conexões...");

    let status_arc = Arc::new(status);

    loop {
        match listener.accept().await {
            Ok((client_stream, addr)) => {
                let status = status_arc.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_xhttp_client(client_stream, &status).await {
                        println!("[xHTTP] Erro cliente {}: {}", addr, e);
                    }
                });
            }
            Err(e) => {
                println!("[xHTTP] Erro aceitar conexao: {}", e);
            }
        }
    }
}

/// Handler de conexão xHTTP com TLS termination
async fn handle_xhttp_client(
    mut stream: TcpStream,
    status: &str,
) -> Result<(), Error> {
    // Ler primeiros bytes para detectar TLS
    let mut buf = [0u8; 1];
    let n = match timeout(Duration::from_secs(10), stream.read(&mut buf)).await {
        Ok(Ok(n)) => n,
        _ => return Ok(()),
    };

    if n == 0 {
        return Ok(());
    }

    let first_byte = buf[0];
    let is_tls = first_byte == 0x16;

    println!("[xHTTP] Conexão recebida: TLS={}", is_tls);

    if is_tls {
        handle_tls_xhttp(stream, status).await
    } else {
        // Fallback: tratar como HTTP direto
        handle_http_xhttp(stream, &buf[..1], status).await
    }
}

/// Handle TLS + xHTTP
async fn handle_tls_xhttp(
    stream: TcpStream,
    status: &str,
) -> Result<(), Error> {
    println!("[xHTTP+TLS] Nova conexão TLS");

    let cert_path = "/opt/sdproxy/cert.pem";
    let key_path = "/opt/sdproxy/key.pem";

    let config = match build_tls_config(cert_path, key_path) {
        Ok(c) => c,
        Err(e) => {
            println!("[xHTTP+TLS] Erro TLS config: {}. Verifique certificados em /opt/sdproxy/", e);
            return Ok(());
        }
    };

    let acceptor = TlsAcceptor::from(Arc::new(config));
    let tls_stream = match acceptor.accept(stream).await {
        Ok(s) => s,
        Err(e) => {
            println!("[xHTTP+TLS] TLS handshake falhou: {}", e);
            return Ok(());
        }
    };

    println!("[xHTTP+TLS] TLS handshake OK");

    // Ler o request HTTP completo
    let (mut tls_read, tls_write) = tokio::io::split(tls_stream);

    // Ler linha por linha até encontrar \r\n\r\n (fim do header HTTP)
    let mut http_headers = Vec::new();
    let mut http_buf = vec![0u8; 4096];
    let mut total_read = 0;
    let mut end_of_headers = false;

    while !end_of_headers && total_read < 16384 {
        match timeout(Duration::from_secs(15), tls_read.read(&mut http_buf)).await {
            Ok(Ok(n)) if n > 0 => {
                total_read += n;
                http_headers.extend_from_slice(&http_buf[..n]);

                // Procurar \r\n\r\n que indica fim dos headers HTTP
                let pos = http_headers.windows(4).position(|w| w == b"\r\n\r\n");
                if let Some(p) = pos {
                    end_of_headers = true;
                    // Se há body (Content-Length), ler o body
                    let header_part = String::from_utf8_lossy(&http_headers[..p]);
                    let content_length = extract_content_length_from_str(&header_part).unwrap_or(0);
                    let header_end = p + 4;
                    let body_already = total_read - header_end;

                    if content_length > 0 && body_already < content_length {
                        let remaining = content_length - body_already;
                        let mut body_buf = vec![0u8; remaining];
                        let mut body_read = 0;
                        while body_read < remaining {
                            match timeout(Duration::from_secs(15), tls_read.read(&mut body_buf[body_read..])).await {
                                Ok(Ok(n)) if n > 0 => {
                                    body_read += n;
                                    http_headers.extend_from_slice(&body_buf[0..n]);
                                }
                                _ => break,
                            }
                        }
                        println!("[xHTTP+TLS] Body lido: {} bytes", body_read);
                    }
                }
            }
            _ => {
                println!("[xHTTP+TLS] Timeout ou erro lendo headers");
                return Ok(());
            }
        }
    }

    let http_str = String::from_utf8_lossy(&http_headers);
    let (method, path) = match parse_http_request(&http_str) {
        Some(m) => m,
        None => {
            println!("[xHTTP+TLS] Falha parsear HTTP request");
            println!("[xHTTP+TLS] Dados recebidos (primeiros 200 chars): {}", &http_str[..http_str.len().min(200)]);
            return Ok(());
        }
    };

    println!("[xHTTP+TLS] {} {}", method, path);

    // Reunir o stream TLS para usar
    let tls_combined = tls_read.unsplit(tls_write);

    match method.as_str() {
        "GET" => handle_xhttp_get(tls_combined, &path, status).await,
        "POST" => handle_xhttp_post(tls_combined, &http_str, &path, status).await,
        other => {
            println!("[xHTTP+TLS] Método não suportado: {}", other);
            Ok(())
        }
    }
}

/// Handle HTTP xHTTP direto (sem TLS)
async fn handle_http_xhttp(
    mut stream: impl AsyncReadExt + AsyncWriteExt + Unpin,
    first_bytes: &[u8],
    status: &str,
) -> Result<(), Error> {
    let mut buf = first_bytes.to_vec();
    let mut rest = vec![0u8; 16384];
    let rest_n = match timeout(Duration::from_secs(5), stream.read(&mut rest)).await {
        Ok(Ok(n)) => n,
        _ => 0,
    };
    buf.extend_from_slice(&rest[..rest_n]);

    let http_str = String::from_utf8_lossy(&buf);
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

/// xHTTP GET - Criar sessão SSH + Streaming downlink (dados SSH → cliente)
/// O cliente envia: GET /ssh/{sessionId}
/// Resposta: 200 com body streaming chunked dos dados SSH
async fn handle_xhttp_get(
    mut stream: impl AsyncReadExt + AsyncWriteExt + Unpin,
    path: &str,
    status: &str,
) -> Result<(), Error> {
    let session_id = extract_session_id(path);
    println!("[xHTTP GET] Path: {}", path);
    println!("[xHTTP GET] Session: {}", session_id);

    if session_id.is_empty() {
        println!("[xHTTP GET] Session ID vazio, retornando 404");
        let resp = format!("HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
        stream.write_all(resp.as_bytes()).await?;
        return Ok(());
    }

    // Conectar ao SSH backend (127.0.0.1:22)
    println!("[xHTTP GET] Conectando ao SSH backend 127.0.0.1:22...");
    let ssh_stream = match TcpStream::connect("127.0.0.1:22").await {
        Ok(s) => s,
        Err(e) => {
            println!("[xHTTP GET] SSH falhou: {}", e);
            let resp = format!("HTTP/1.1 502 Bad Gateway\r\nX-Status: {}\r\nContent-Length: 0\r\n\r\n", status);
            stream.write_all(resp.as_bytes()).await?;
            return Ok(());
        }
    };
    println!("[xHTTP GET] SSH conectado!");

    let (ssh_r, ssh_w) = ssh_stream.into_split();
    let ssh_r = Arc::new(Mutex::new(ssh_r));
    let ssh_w = Arc::new(Mutex::new(ssh_w));

    // Salvar sessão para que os POSTs possam enviar dados
    {
        let mut sessions = SESSIONS.lock().await;
        sessions.insert(session_id.clone(), XhttpSession {
            ssh_write: ssh_w,
            ssh_read: ssh_r.clone(),
        });
        println!("[xHTTP GET] Sessão {} registrada", session_id);
    }

    // Enviar response HTTP 200 com Transfer-Encoding: chunked
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

    println!("[xHTTP GET] Enviando response 200 chunked");
    stream.write_all(response.as_bytes()).await?;
    stream.flush().await?;
    println!("[xHTTP GET] Response enviado, iniciando streaming SSH → cliente");

    // Loop: ler dados do SSH e enviar como chunks HTTP
    loop {
        let data = {
            let mut read_guard = ssh_r.lock().await;
            let mut buf = [0u8; 4096];
            match timeout(Duration::from_secs(120), read_guard.read(&mut buf)).await {
                Ok(Ok(0)) => {
                    println!("[xHTTP GET] SSH fechou conexão (EOF)");
                    break;
                }
                Ok(Ok(n)) => {
                    println!("[xHTTP GET] SSH → {} bytes", n);
                    Some(buf[..n].to_vec())
                }
                Ok(Err(e)) => {
                    println!("[xHTTP GET] Erro lendo SSH: {}", e);
                    break;
                }
                Err(_) => {
                    // Timeout - enviar chunk vazio para manter conexão viva
                    println!("[xHTTP GET] Timeout SSH, keepalive...");
                    None
                }
            }
        };

        match data {
            Some(chunk) => {
                // Enviar como chunked encoding
                let chunk_header = format!("{:x}\r\n", chunk.len());
                if stream.write_all(chunk_header.as_bytes()).await.is_err() {
                    println!("[xHTTP GET] Erro escrevendo chunk header, cliente fechou");
                    break;
                }
                if stream.write_all(&chunk).await.is_err() {
                    println!("[xHTTP GET] Erro escrevendo chunk data");
                    break;
                }
                if stream.write_all(b"\r\n").await.is_err() {
                    break;
                }
                if stream.flush().await.is_err() {
                    break;
                }
            }
            None => {
                // Keepalive - esperar mais
            }
        }
    }

    // Limpar sessão
    {
        let mut sessions = SESSIONS.lock().await;
        sessions.remove(&session_id);
        println!("[xHTTP GET] Sessão {} removida", session_id);
    }

    println!("[xHTTP GET] Fim session {}", session_id);
    Ok(())
}

/// xHTTP POST - Uplink (dados cliente → SSH)
/// O cliente envia: POST /ssh/{sessionId}/{sequence}
/// Content-Type: application/octet-stream
/// Body: bytes SSH (até 900KiB)
async fn handle_xhttp_post(
    mut stream: impl AsyncReadExt + AsyncWriteExt + Unpin,
    full_request: &str,
    path: &str,
    status: &str,
) -> Result<(), Error> {
    // Path: /ssh/{sessionId}/{sequence}
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    let session_id = if parts.len() >= 2 { parts[1].to_string() } else { String::new() };
    let sequence = if parts.len() >= 3 { parts[2].to_string() } else { "0".to_string() };

    println!("[xHTTP POST] Path: {}", path);
    println!("[xHTTP POST] Session: {} Seq: {}", session_id, sequence);

    // Pegar Content-Length dos headers
    let content_length = extract_content_length_from_str(full_request).unwrap_or(0);
    println!("[xHTTP POST] Content-Length: {}", content_length);

    if content_length == 0 {
        // POST sem body - apenas acknowledge
        let resp = format!("HTTP/1.1 200 OK\r\nContent-Length: 0\r\nX-Status: {}\r\n\r\n", status);
        stream.write_all(resp.as_bytes()).await?;
        return Ok(());
    }

    // Ler o body do POST (dados SSH)
    let mut body_buf = vec![0u8; content_length];
    let mut total_read = 0;
    while total_read < content_length {
        match timeout(Duration::from_secs(30), stream.read(&mut body_buf[total_read..])).await {
            Ok(Ok(0)) => {
                println!("[xHTTP POST] Stream fechado antes de ler body completo");
                break;
            }
            Ok(Ok(n)) => {
                total_read += n;
            }
            Ok(Err(e)) => {
                println!("[xHTTP POST] Erro lendo body: {}", e);
                break;
            }
            Err(_) => {
                println!("[xHTTP POST] Timeout lendo body");
                break;
            }
        }
    }

    println!("[xHTTP POST] Body lido: {}/{} bytes", total_read, content_length);

    // Enviar dados ao SSH backend via sessão
    let mut sessions = SESSIONS.lock().await;
    if let Some(session) = sessions.get(&session_id) {
        let mut write_guard = session.ssh_write.lock().await;
        if write_guard.write_all(&body_buf[..total_read]).await.is_err() {
            println!("[xHTTP POST] Erro escrevendo no SSH");
        } else {
            println!("[xHTTP POST] {} bytes enviados ao SSH", total_read);
        }
    } else {
        println!("[xHTTP POST] Sessão {} não encontrada!", session_id);
    }

    // Responder 200 (sucesso = body aceito para entrega)
    let resp = format!("HTTP/1.1 200 OK\r\nContent-Length: 0\r\nX-Status: {}\r\n\r\n", status);
    stream.write_all(resp.as_bytes()).await?;
    stream.flush().await?;

    println!("[xHTTP POST] Response 200 enviado");
    Ok(())
}

// === Helpers ===

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
    // /ssh/{sessionId} ou /ssh/{sessionId}/{seq}
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    if parts.len() >= 2 {
        parts[1].to_string()
    } else if parts.len() == 1 && !parts[0].is_empty() {
        parts[0].to_string()
    } else {
        String::new()
    }
}

fn extract_content_length_from_str(data: &str) -> Option<usize> {
    for line in data.lines() {
        let lower = line.to_lowercase();
        if lower.starts_with("content-length:") {
            return line.split(':').nth(1)?.trim().parse().ok();
        }
    }
    None
}

fn build_tls_config(cert_path: &str, key_path: &str) -> Result<rustls::ServerConfig, Error> {
    let cert_file = std::fs::File::open(cert_path)?;
    let key_file = std::fs::File::open(key_path)?;
    let mut cert_reader = std::io::BufReader::new(cert_file);
    let mut key_reader = std::io::BufReader::new(key_file);

    let certs: Vec<Certificate> = rustls_pemfile::certs(&mut cert_reader)?
        .into_iter()
        .map(Certificate)
        .collect();

    let keys: Vec<PrivateKey> = rustls_pemfile::pkcs8_private_keys(&mut key_reader)?
        .into_iter()
        .map(PrivateKey)
        .collect();

    if certs.is_empty() || keys.is_empty() {
        return Err(Error::new(std::io::ErrorKind::Other, "Certs ou keys vazios"));
    }

    let config = rustls::ServerConfig::builder()
        .with_safe_defaults()
        .with_no_client_auth()
        .with_single_cert(certs, keys.into_iter().next().unwrap())
        .map_err(|e| Error::new(std::io::ErrorKind::Other, e))?;

    Ok(config)
}
