use std::collections::HashMap;
use std::io::Error;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use lazy_static::lazy_static;
use log::info;

lazy_static! {
    static ref SESSIONS: Arc<Mutex<HashMap<String, Arc<Mutex<XHttpSession>>>>> =
        Arc::new(Mutex::new(HashMap::new()));
}

// ─────────────────────────────────────────────────────────────────────────────
// Sessão xHTTP: mantém o backend TCP + buffers de reassembly
// ─────────────────────────────────────────────────────────────────────────────

struct XHttpSession {
    backend: Option<TcpStream>,
    downlink_buffer: Vec<u8>,
    active: bool,
    created: std::time::Instant,
}

impl XHttpSession {
    fn new() -> Self {
        XHttpSession {
            backend: None,
            downlink_buffer: Vec::new(),
            active: true,
            created: std::time::Instant::now(),
        }
    }

    async fn connect_backend(&mut self, ssh_only: bool) -> Result<(), Error> {
        if self.backend.is_some() {
            return Ok(());
        }
        let primary = if ssh_only {
            "127.0.0.1:22"
        } else {
            "127.0.0.1:22"
        };
        match TcpStream::connect(primary).await {
            Ok(s) => {
                info!("xHTTP session: backend SSH conectado");
                self.backend = Some(s);
                Ok(())
            }
            Err(e) => {
                if !ssh_only {
                    info!("xHTTP SSH falhou ({}), tentando VPN...", e);
                    match TcpStream::connect("127.0.0.1:1194").await {
                        Ok(s) => {
                            info!("xHTTP session: backend VPN conectado");
                            self.backend = Some(s);
                            Ok(())
                        }
                        Err(e2) => Err(Error::new(
                            std::io::ErrorKind::ConnectionRefused,
                            format!("SSH: {}, VPN: {}", e, e2),
                        )),
                    }
                } else {
                    Err(e)
                }
            }
        }
    }

    async fn read_backend(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        if !self.downlink_buffer.is_empty() {
            let len = buf.len().min(self.downlink_buffer.len());
            buf[..len].copy_from_slice(&self.downlink_buffer[..len]);
            self.downlink_buffer.drain(..len);
            return Ok(len);
        }
        match self.backend {
            Some(ref mut b) => {
                match tokio::time::timeout(
                    std::time::Duration::from_millis(500),
                    b.read(buf),
                ).await {
                    Ok(Ok(0)) => Err(Error::new(
                        std::io::ErrorKind::ConnectionReset, "Backend fechou")),
                    Ok(Ok(n)) => Ok(n),
                    Ok(Err(e)) => Err(e),
                    Err(_) => Ok(0),
                }
            }
            None => Err(Error::new(
                std::io::ErrorKind::NotConnected, "Sem backend")),
        }
    }

    async fn write_backend(&mut self, data: &[u8]) -> Result<(), Error> {
        match self.backend {
            Some(ref mut b) => {
                b.write_all(data).await?;
                Ok(())
            }
            None => Err(Error::new(
                std::io::ErrorKind::NotConnected, "Sem backend")),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// HTTP Request parser simples
// ─────────────────────────────────────────────────────────────────────────────

struct HttpRequest {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    content_length: usize,
}

impl HttpRequest {
    fn parse(data: &[u8]) -> Option<HttpRequest> {
        let text = String::from_utf8_lossy(data);
        let header_end = find_crlf_crlf(data)?;
        let header_section = text[..header_end].to_string();
        let lines: Vec<&str> = header_section.split("\r\n").collect();
        if lines.is_empty() { return None; }

        let parts: Vec<&str> = lines[0].splitn(3, ' ').collect();
        if parts.len() < 2 { return None; }

        let mut headers = HashMap::new();
        let mut content_length: usize = 0;
        for line in &lines[1..] {
            if let Some(pos) = line.find(':') {
                let key = line[..pos].trim().to_lowercase();
                let val = line[pos + 1..].trim().to_string();
                if key == "content-length" {
                    content_length = val.parse().unwrap_or(0);
                }
                headers.insert(key, val);
            }
        }

        Some(HttpRequest {
            method: parts[0].to_string(),
            path: parts[1].to_string(),
            headers,
            content_length,
        })
    }
}

fn find_crlf_crlf(data: &[u8]) -> Option<usize> {
    for i in 0..data.len().saturating_sub(3) {
        if data[i] == b'\r' && data[i + 1] == b'\n'
            && data[i + 2] == b'\r' && data[i + 3] == b'\n' {
            return Some(i);
        }
    }
    None
}

// ─────────────────────────────────────────────────────────────────────────────
// Handler principal xHTTP (SplitHTTP)
// ─────────────────────────────────────────────────────────────────────────────

/// Compatível com SocksRevive-XHTTP-DEMO:
///   GET  /path/session-id       → streaming downlink (server→client)
///   POST /path/session-id/seq   → uplink sequenciado (client→server)
pub async fn handle_xhttp(
    client_stream: TcpStream,
    status: &str,
    ssh_only: bool,
) -> Result<(), Error> {
    let mut client = client_stream;

    // Ler request
    let mut buf = vec![0u8; 65536];
    let n = client.read(&mut buf).await?;
    if n == 0 {
        return Err(Error::new(
            std::io::ErrorKind::ConnectionReset, "Sem dados"));
    }

    let request = HttpRequest::parse(&buf[..n]).ok_or_else(|| {
        Error::new(std::io::ErrorKind::InvalidData, "HTTP inválido")
    })?;

    info!("xHTTP {} {} (content-length: {})", request.method, request.path, request.content_length);

    match request.method.as_str() {
        "GET" => {
            // Se tem body (xHTTP com dados inline), tratar como POST
            if request.content_length > 0 && n > find_crlf_crlf(&buf[..n]).unwrap_or(0) + 4 {
                let body_start = find_crlf_crlf(&buf[..n]).unwrap_or(0) + 4;
                let body = &buf[body_start..n];
                handle_xhttp_post(&mut client, &request.path, body, ssh_only).await
            } else {
                handle_xhttp_get(&mut client, &request.path, status, ssh_only).await
            }
        }
        "POST" => {
            // Extrair body após headers
            let body_start = find_crlf_crlf(&buf[..n]).unwrap_or(0) + 4;
            let body = if body_start < n {
                &buf[body_start..n]
            } else {
                &[]
            };
            // Se body recebido é menor que Content-Length, ler o resto
            let mut full_body = body.to_vec();
            while full_body.len() < request.content_length {
                let remaining = request.content_length - full_body.len();
                let mut tmp = vec![0u8; remaining];
                let r = tokio::time::timeout(
                    std::time::Duration::from_secs(5),
                    client.read(&mut tmp),
                ).await;
                match r {
                    Ok(Ok(rn)) if rn > 0 => {
                        full_body.extend_from_slice(&tmp[..rn]);
                    }
                    _ => break,
                }
            }
            handle_xhttp_post(&mut client, &request.path, &full_body, ssh_only).await
        }
        _ => {
            let resp = "HTTP/1.1 405 Method Not Allowed\r\nContent-Length: 0\r\n\r\n";
            client.write_all(resp.as_bytes()).await?;
            Ok(())
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// GET handler: streaming downlink
// ─────────────────────────────────────────────────────────────────────────────

async fn handle_xhttp_get(
    client: &mut TcpStream,
    path: &str,
    status: &str,
    ssh_only: bool,
) -> Result<(), Error> {
    // /path/session-id  ou  /session-id
    let session_id = extract_session_id(path);
    info!("xHTTP GET session={}", session_id);

    let session = get_or_create_session(&session_id, ssh_only).await?;

    // Responder 200 OK com streaming
    let response = format!(
        "HTTP/1.1 200 {}\r\n\
         Server: SDProxy\r\n\
         Content-Type: application/octet-stream\r\n\
         Transfer-Encoding: chunked\r\n\
         Connection: keep-alive\r\n\
         X-Session: {}\r\n\
         X-Protocol: xHTTP\r\n\
         \r\n",
        status, session_id
    );
    client.write_all(response.as_bytes()).await?;
    client.flush().await?;

    // Loop de streaming: lê do backend e envia como chunks
    let mut chunk_buf = vec![0u8; 8192];
    let mut empty_chunks = 0;
    let max_empty = 120; // ~60s de idle antes de fechar

    loop {
        let mut sess = session.lock().await;
        if !sess.active {
            break;
        }

        match sess.read_backend(&mut chunk_buf).await {
            Ok(n) if n > 0 => {
                empty_chunks = 0;
                // Enviar chunk HTTP: tamanho_hex\r\ndados\r\n
                let hex = format!("{:x}\r\n", n);
                client.write_all(hex.as_bytes()).await?;
                client.write_all(&chunk_buf[..n]).await?;
                client.write_all(b"\r\n").await?;
            }
            Ok(_) => {
                // Timeout sem dados - enviar keep-alive
                empty_chunks += 1;
                if empty_chunks > max_empty {
                    // Enviar chunk vazio final (encerrar stream)
                    client.write_all(b"0\r\n\r\n").await?;
                    client.flush().await?;
                    break;
                }
            }
            Err(_) => {
                // Backend fechado
                client.write_all(b"0\r\n\r\n").await?;
                client.flush().await?;
                break;
            }
        }
        client.flush().await?;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    info!("xHTTP GET stream encerrado (session={})", session_id);
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// POST handler: uplink sequenciado
// ─────────────────────────────────────────────────────────────────────────────

async fn handle_xhttp_post(
    client: &mut TcpStream,
    path: &str,
    body: &[u8],
    _ssh_only: bool,
) -> Result<(), Error> {
    let clean_path = path.trim_start_matches('/');
    let parts: Vec<&str> = clean_path.split('/').collect();

    let session_id = if parts.len() >= 2 {
        parts[1].to_string()
    } else if !parts.is_empty() {
        parts[0].to_string()
    } else {
        return Err(Error::new(std::io::ErrorKind::InvalidInput, "Path vazio"));
    };

    let seq: u64 = if parts.len() > 2 {
        parts[2].parse().unwrap_or(0)
    } else {
        0
    };

    info!("xHTTP POST session={} seq={} body={}B", session_id, seq, body.len());

    // Obter sessão
    let sessions = SESSIONS.lock().await;
    let session = sessions.get(&session_id).cloned();
    drop(sessions);

    let session = match session {
        Some(s) => s,
        None => {
            // Cliente enviou POST antes do GET, retornar 404
            let resp = format!(
                "HTTP/1.1 404 Not Found\r\n\
                 Content-Length: 27\r\n\
                 Connection: close\r\n\r\nSession {} not found",
                session_id
            );
            client.write_all(resp.as_bytes()).await?;
            return Ok(());
        }
    };

    // Encaminhar dados ao backend
    {
        let mut sess = session.lock().await;
        if let Err(e) = sess.write_backend(body).await {
            info!("xHTTP POST write_backend error: {}", e);
        }
    }

    // Responder 200 OK
    let resp = format!(
        "HTTP/1.1 200 OK\r\n\
         Server: SDProxy\r\n\
         Content-Type: application/octet-stream\r\n\
         Content-Length: 2\r\n\
         Connection: keep-alive\r\n\
         X-Session: {}\r\n\
         X-Seq: {}\r\n\r\nOK",
        session_id, seq
    );
    client.write_all(resp.as_bytes()).await?;
    client.flush().await?;

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Proto handler (TCP raw sem HTTP)
// ─────────────────────────────────────────────────────────────────────────────

pub async fn handle_proto(
    mut socket: TcpStream,
    ssh_only: bool,
) -> Result<(), Error> {
    info!("Proto: conexão raw");

    let addr = if ssh_only {
        "127.0.0.1:22"
    } else {
        "127.0.0.1:22"
    };

    let mut backend = match TcpStream::connect(addr).await {
        Ok(s) => s,
        Err(e) => {
            if !ssh_only {
                match TcpStream::connect("127.0.0.1:1194").await {
                    Ok(s) => s,
                    Err(e2) => return Err(Error::new(
                        std::io::ErrorKind::ConnectionRefused,
                        format!("SSH: {}, VPN: {}", e, e2))),
                }
            } else {
                return Err(e);
            }
        }
    };

    info!("Proto: backend conectado");
    tokio::io::copy_bidirectional(&mut socket, &mut backend).await?;
    info!("Proto: tunnel finalizado");
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn extract_session_id(path: &str) -> String {
    let clean = path.trim_start_matches('/');
    let parts: Vec<&str> = clean.split('/').collect();
    if parts.len() >= 2 {
        parts[1].to_string()
    } else if !parts.is_empty() {
        parts[0].to_string()
    } else {
        String::new()
    }
}

async fn get_or_create_session(
    id: &str,
    ssh_only: bool,
) -> Result<Arc<Mutex<XHttpSession>>, Error> {
    let mut sessions = SESSIONS.lock().await;
    let session = sessions.entry(id.to_string())
        .or_insert_with(|| Arc::new(Mutex::new(XHttpSession::new())))
        .clone();
    drop(sessions);

    // Conectar backend se necessário
    {
        let mut sess = session.lock().await;
        sess.connect_backend(ssh_only).await?;
    }

    Ok(session)
}
