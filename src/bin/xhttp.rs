use std::collections::HashMap;
use std::io::Error;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio::time::{timeout, Duration};

use tokio_rustls::rustls::{self, Certificate, PrivateKey};
use tokio_rustls::TlsAcceptor;

/// Sessão xHTTP ativa com canais para comunicação GET<->POST<->SSH
struct XhttpSession {
    /// Canal para enviar dados do POST (uplink) para a task do SSH
    post_tx: mpsc::Sender<Vec<u8>>,
    /// Canal para enviar dados do SSH (downlink) para a task do GET
    get_tx: mpsc::Sender<Vec<u8>>,
    /// Indicador se a sessão está ativa
    active: Arc<RwLock<bool>>,
}

static SESSIONS: once_cell::sync::Lazy<Arc<Mutex<HashMap<String, XhttpSession>>>> =
    once_cell::sync::Lazy::new(|| Arc::new(Mutex::new(HashMap::new())));

#[tokio::main]
async fn main() -> Result<(), Error> {
    let port = get_port();
    let status = get_status();
    let ssh_port = get_ssh_port();

    println!("[xHTTP] Servico xHTTP SplitHTTP rodando na porta: {}", port);
    println!("[xHTTP] SSH backend: 127.0.0.1:{}", ssh_port);
    println!("[xHTTP] Status: {}", status);
    println!("[xHTTP] Certs: /opt/sdproxy/cert.pem + key.pem");
    println!("[xHTTP] Aguardando conexões...");

    let listener = TcpListener::bind(format!("[::]:{}", port)).await?;
    let status_arc = Arc::new(status);

    loop {
        match listener.accept().await {
            Ok((client_stream, addr)) => {
                let status = status_arc.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_xhttp_client(client_stream, &status, ssh_port).await {
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

async fn handle_xhttp_client(
    stream: TcpStream,
    status: &str,
    ssh_port: u16,
) -> Result<(), Error> {
    // Usar PEEK para detectar TLS sem consumir o byte
    let mut peek_buf = [0u8; 3];
    let peek_result = timeout(Duration::from_secs(10), stream.peek(&mut peek_buf)).await;
    let bytes_peeked = match peek_result {
        Ok(Ok(n)) => n,
        _ => return Ok(()),
    };

    let first_byte = peek_buf[0];
    println!("[xHTTP] Conexão: first_byte=0x{:02x} bytes={}", first_byte, bytes_peeked);

    // Detecta TLS (0x16 = TLS ClientHello)
    if first_byte == 0x16 {
        println!("[xHTTP] TLS detectado, fazendo handshake...");
        return handle_tls_xhttp(stream, status, ssh_port).await;
    }

    // Detecta HTTP (GET, POST, HEAD)
    if first_byte == 0x47 || first_byte == 0x50 || first_byte == 0x48 {
        println!("[xHTTP] HTTP direto detectado");
        return handle_http_xhttp_raw(stream, status, ssh_port).await;
    }

    // Dados raw TCP - tenta tratar como HTTP puro (sem TLS)
    println!("[xHTTP] Dados raw TCP (0x{:02x}), tentando HTTP puro...", first_byte);
    handle_http_xhttp_raw(stream, status, ssh_port).await
}

async fn handle_tls_xhttp(
    stream: TcpStream,
    status: &str,
    ssh_port: u16,
) -> Result<(), Error> {
    let cert_path = "/opt/sdproxy/cert.pem";
    let key_path = "/opt/sdproxy/key.pem";

    let config = match build_tls_config(cert_path, key_path) {
        Ok(c) => c,
        Err(e) => {
            println!("[xHTTP] Erro TLS config: {}. Verifique certs.", e);
            return Ok(());
        }
    };

    let acceptor = TlsAcceptor::from(Arc::new(config));
    let mut tls_stream = match acceptor.accept(stream).await {
        Ok(s) => s,
        Err(e) => {
            println!("[xHTTP] TLS handshake falhou: {}", e);
            return Ok(());
        }
    };

    println!("[xHTTP] TLS handshake OK");

    // Ler o request HTTP completo do stream TLS
    let mut tls_read_buf = Vec::new();
    let mut chunk = vec![0u8; 8192];
    let mut end_of_headers = false;
    let mut total_read = 0usize;

    while !end_of_headers && total_read < 65536 {
        match timeout(Duration::from_secs(15), tls_stream.read(&mut chunk)).await {
            Ok(Ok(n)) if n > 0 => {
                total_read += n;
                tls_read_buf.extend_from_slice(&chunk[..n]);
                if let Some(_) = tls_read_buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    end_of_headers = true;
                }
            }
            _ => {
                println!("[xHTTP] Timeout lendo HTTP request TLS");
                return Ok(());
            }
        }
    }

    // Extrair method e path antes de ler body
    let header_end_pos = tls_read_buf.windows(4).position(|w| w == b"\r\n\r\n").unwrap_or(0);
    let http_str: String = String::from_utf8_lossy(&tls_read_buf[..header_end_pos]).to_string();
    let content_length = extract_content_length_from_bytes(&tls_read_buf[..header_end_pos + 4]).unwrap_or(0);
    let body_already = total_read - (header_end_pos + 4);

    // Se há body (POST), ler o body completo
    if content_length > 0 && body_already < content_length {
        let remaining = content_length - body_already;
        let mut body_buf = vec![0u8; remaining];
        let mut body_read = 0;
        while body_read < remaining {
            match timeout(Duration::from_secs(30), tls_stream.read(&mut body_buf[body_read..])).await {
                Ok(Ok(n)) if n > 0 => {
                    body_read += n;
                }
                _ => break,
            }
        }
        tls_read_buf.extend_from_slice(&body_buf[..body_read]);
        println!("[xHTTP] POST TLS body: {} bytes", body_read);
    }

    let (method, path) = match parse_http_request(&http_str) {
        Some(m) => m,
        None => {
            println!("[xHTTP] Falha parsear HTTP TLS: {:?}", &http_str[..http_str.len().min(200)]);
            return Ok(());
        }
    };

    println!("[xHTTP TLS] {} {}", method, path);

    // Precisa enviar a resposta pelo TLS stream
    let mut tls_stream = tls_stream;

    match method.as_str() {
        "GET" => handle_xhttp_get_tls(&mut tls_stream, &path, status, ssh_port).await,
        "POST" => handle_xhttp_post_tls(&mut tls_stream, &tls_read_buf, &path, status).await,
        other => {
            println!("[xHTTP] Método não suportado: {}", other);
            Ok(())
        }
    }
}

/// Handle dados raw TCP como HTTP puro (sem TLS)
async fn handle_http_xhttp_raw(
    mut stream: TcpStream,
    status: &str,
    ssh_port: u16,
) -> Result<(), Error> {
    // Ler request HTTP completo
    let mut buf = vec![0u8; 65536];
    let n = match timeout(Duration::from_secs(10), stream.read(&mut buf)).await {
        Ok(Ok(n)) if n > 0 => n,
        _ => return Ok(()),
    };

    let http_str = String::from_utf8_lossy(&buf[..n]);
    println!("[xHTTP RAW] Dados recebidos: {} bytes", n);

    // Extrair headers até \r\n\r\n
    let header_end = http_str.find("\r\n\r\n").unwrap_or(0);
    let header_str = if header_end > 0 {
        &http_str[..header_end]
    } else {
        &http_str
    };

    let (method, path) = match parse_http_request(header_str) {
        Some(m) => m,
        None => {
            println!("[xHTTP RAW] Nao eh HTTP valido");
            return Ok(());
        }
    };

    println!("[xHTTP RAW] {} {}", method, path);

    match method.as_str() {
        "GET" => handle_xhttp_get_raw(&mut stream, &path, status, ssh_port).await,
        "POST" => handle_xhttp_post_raw(&mut stream, &buf[..n], &path, status).await,
        _ => Ok(()),
    }
}

/// xHTTP GET via TLS - Abre SSH e mantém stream aberta para downlink
async fn handle_xhttp_get_tls(
    tls_stream: &mut tokio_rustls::server::TlsStream<TcpStream>,
    path: &str,
    status: &str,
    ssh_port: u16,
) -> Result<(), Error> {
    let mut session_id = extract_session_id(path);
    
    if session_id.is_empty() {
        session_id = generate_session_id();
        println!("[xHTTP GET TLS] Path: {} Session: {} (gerado)", path, session_id);
    } else {
        println!("[xHTTP GET TLS] Path: {} Session: {}", path, session_id);
    }

    // Conectar ao SSH backend
    println!("[xHTTP GET TLS] Conectando SSH 127.0.0.1:{}...", ssh_port);
    let ssh_stream = match TcpStream::connect(format!("127.0.0.1:{}", ssh_port)).await {
        Ok(s) => s,
        Err(e) => {
            println!("[xHTTP GET TLS] SSH falhou: {}", e);
            let resp = format!("HTTP/1.1 502 Bad Gateway\r\nX-Status: {}\r\nContent-Length: 0\r\n\r\n", status);
            tls_stream.write_all(resp.as_bytes()).await?;
            return Ok(());
        }
    };
    println!("[xHTTP GET TLS] SSH conectado!");

    // Dividir SSH stream
    let (mut ssh_read, mut ssh_write) = ssh_stream.into_split();

    // Criar canais
    let (post_tx, mut post_rx) = mpsc::channel::<Vec<u8>>(256);
    let (get_tx, mut get_rx) = mpsc::channel::<Vec<u8>>(256);
    let active = Arc::new(RwLock::new(true));

    // Registrar sessão
    {
        let mut sessions = SESSIONS.lock().await;
        sessions.insert(session_id.clone(), XhttpSession {
            post_tx,
            get_tx: get_tx.clone(), // clona para a sessão
            active: active.clone(),
        });
        println!("[xHTTP GET TLS] Sessão {} registrada", session_id);
    }

    // Task 1: Ler dados do POST e escrever no SSH
    let active_write = active.clone();
    let _ssh_write_task = tokio::spawn(async move {
        while let Some(data) = post_rx.recv().await {
            if !*active_write.read().await {
                break;
            }
            if ssh_write.write_all(&data).await.is_err() {
                println!("[xHTTP SSH-Write] Erro escrevendo para SSH");
                break;
            }
            println!("[xHTTP SSH-Write] {} bytes → SSH", data.len());
        }
    });

    // Task 2: Ler dados do SSH e enviar pelo canal GET
    let active_read = active.clone();
    let get_tx_for_read = get_tx.clone();
    let _ssh_read_task = tokio::spawn(async move {
        let mut buf = vec![0u8; 16384];
        loop {
            if !*active_read.read().await {
                break;
            }
            match timeout(Duration::from_secs(60), ssh_read.read(&mut buf)).await {
                Ok(Ok(0)) => {
                    println!("[xHTTP SSH-Read] SSH EOF");
                    break;
                }
                Ok(Ok(n)) => {
                    let data = buf[..n].to_vec();
                    println!("[xHTTP SSH-Read] SSH → GET channel: {} bytes", n);
                    if get_tx_for_read.send(data).await.is_err() {
                        println!("[xHTTP SSH-Read] GET channel fechado");
                        break;
                    }
                }
                Ok(Err(e)) => {
                    println!("[xHTTP SSH-Read] Erro SSH: {}", e);
                    break;
                }
                Err(_) => {
                    // Timeout sem dados - continua esperando
                }
            }
        }
    });

    // Enviar response 200 OK - streaming infinito
    let response = format!(
        "HTTP/1.1 200 OK\r\n\
         Content-Type: application/octet-stream\r\n\
         Cache-Control: no-cache, no-store, must-revalidate\r\n\
         Pragma: no-cache\r\n\
         Expires: 0\r\n\
         Connection: keep-alive\r\n\
         X-Session-ID: {}\r\n\
         X-Status: {}\r\n\r\n",
        session_id, status
    );

    tls_stream.write_all(response.as_bytes()).await?;
    tls_stream.flush().await?;
    println!("[xHTTP GET TLS] Headers de streaming enviados");

    // Stream: ler do canal GET e enviar para o cliente TLS
    loop {
        match timeout(Duration::from_secs(60), get_rx.recv()).await {
            Ok(Some(data)) => {
                if tls_stream.write_all(&data).await.is_err() {
                    println!("[xHTTP GET TLS] Erro escrevendo para cliente TLS");
                    break;
                }
                if tls_stream.flush().await.is_err() {
                    break;
                }
                println!("[xHTTP GET TLS] → cliente: {} bytes", data.len());
            }
            Ok(None) => {
                println!("[xHTTP GET TLS] Canal GET fechado");
                break;
            }
            Err(_) => {
                // Timeout - continua esperando
            }
        }
    }

    // Desativar sessão
    *active.write().await = false;
    {
        let mut sessions = SESSIONS.lock().await;
        sessions.remove(&session_id);
        println!("[xHTTP GET TLS] Sessão {} removida", session_id);
    }

    Ok(())
}

/// xHTTP GET raw (sem TLS)
async fn handle_xhttp_get_raw(
    mut stream: impl AsyncReadExt + AsyncWriteExt + Unpin,
    path: &str,
    status: &str,
    ssh_port: u16,
) -> Result<(), Error> {
    let mut session_id = extract_session_id(path);
    
    if session_id.is_empty() {
        session_id = generate_session_id();
    }

    println!("[xHTTP GET RAW] Session: {}", session_id);

    let ssh_stream = match TcpStream::connect(format!("127.0.0.1:{}", ssh_port)).await {
        Ok(s) => s,
        Err(e) => {
            println!("[xHTTP GET RAW] SSH falhou: {}", e);
            let resp = format!("HTTP/1.1 502 Bad Gateway\r\nX-Status: {}\r\nContent-Length: 0\r\n\r\n", status);
            stream.write_all(resp.as_bytes()).await?;
            return Ok(());
        }
    };

    let (mut ssh_read, mut ssh_write) = ssh_stream.into_split();

    let (post_tx, mut post_rx) = mpsc::channel::<Vec<u8>>(256);
    let (get_tx, mut get_rx) = mpsc::channel::<Vec<u8>>(256);
    let active = Arc::new(RwLock::new(true));

    {
        let mut sessions = SESSIONS.lock().await;
        sessions.insert(session_id.clone(), XhttpSession {
            post_tx,
            get_tx: get_tx.clone(),
            active: active.clone(),
        });
    }

    // Task SSH write
    tokio::spawn(async move {
        while let Some(data) = post_rx.recv().await {
            if ssh_write.write_all(&data).await.is_err() {
                break;
            }
        }
    });

    // Task SSH read
    let get_tx_for_read = get_tx.clone();
    tokio::spawn(async move {
        let mut buf = vec![0u8; 16384];
        loop {
            match timeout(Duration::from_secs(60), ssh_read.read(&mut buf)).await {
                Ok(Ok(0)) => break,
                Ok(Ok(n)) => {
                    let data = buf[..n].to_vec();
                    if get_tx_for_read.send(data).await.is_err() {
                        break;
                    }
                }
                _ => break,
            }
        }
    });

    // Enviar headers
    let response = format!(
        "HTTP/1.1 200 OK\r\n\
         Content-Type: application/octet-stream\r\n\
         Cache-Control: no-cache, no-store, must-revalidate\r\n\
         Pragma: no-cache\r\n\
         Expires: 0\r\n\
         Connection: keep-alive\r\n\
         X-Session-ID: {}\r\n\
         X-Status: {}\r\n\r\n",
        session_id, status
    );

    stream.write_all(response.as_bytes()).await?;
    stream.flush().await?;

    // Stream dados do SSH para o cliente
    loop {
        match timeout(Duration::from_secs(60), get_rx.recv()).await {
            Ok(Some(data)) => {
                if stream.write_all(&data).await.is_err() {
                    break;
                }
                if stream.flush().await.is_err() {
                    break;
                }
            }
            _ => {
                break;
            }
        }
    }

    *active.write().await = false;
    {
        let mut sessions = SESSIONS.lock().await;
        sessions.remove(&session_id);
    }

    Ok(())
}

/// xHTTP POST via TLS - Escreve dados no SSH via canal
async fn handle_xhttp_post_tls(
    tls_stream: &mut tokio_rustls::server::TlsStream<TcpStream>,
    full_request: &[u8],
    path: &str,
    status: &str,
) -> Result<(), Error> {
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    let session_id = if parts.len() >= 2 { parts[1].to_string() } else { String::new() };
    let sequence = if parts.len() >= 3 { parts[2] } else { "0" };

    println!("[xHTTP POST TLS] Session: {} Seq: {}", session_id, sequence);

    let content_length = extract_content_length_from_bytes(full_request).unwrap_or(0);

    if content_length == 0 {
        let resp = format!("HTTP/1.1 200 OK\r\nContent-Length: 0\r\nX-Status: {}\r\n\r\n", status);
        tls_stream.write_all(resp.as_bytes()).await?;
        return Ok(());
    }

    // Extrair body do request
    let header_end = full_request.windows(4).position(|w| w == b"\r\n\r\n").unwrap_or(0) + 4;
    let body_in_request = full_request.len() - header_end;

    let sessions = SESSIONS.lock().await;
    if let Some(session) = sessions.get(&session_id) {
        let mut body = if body_in_request >= content_length {
            full_request[header_end..header_end + content_length].to_vec()
        } else {
            let mut body = full_request[header_end..].to_vec();
            // Ler restante do TLS stream
            let remaining = content_length - body_in_request;
            let mut buf = vec![0u8; remaining];
            let mut read = 0;
            while read < remaining {
                match timeout(Duration::from_secs(30), tls_stream.read(&mut buf[read..])).await {
                    Ok(Ok(n)) if n > 0 => {
                        read += n;
                    }
                    _ => break,
                }
            }
            body.extend_from_slice(&buf[..read]);
            body[..content_length].to_vec()
        };

        body.truncate(content_length);
        println!("[xHTTP POST TLS] {} bytes → canal POST (Seq: {})", body.len(), sequence);

        // Enviar dados pelo canal POST para a task do SSH
        if session.post_tx.send(body).await.is_err() {
            println!("[xHTTP POST TLS] Canal POST fechado para sessao {}", session_id);
        }
    } else {
        println!("[xHTTP POST TLS] Sessão {} não encontrada!", session_id);
        let resp = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n";
        tls_stream.write_all(resp.as_bytes()).await?;
        return Ok(());
    }

    // Responder 200 OK
    let resp = "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: keep-alive\r\n\r\n";
    tls_stream.write_all(resp.as_bytes()).await?;
    tls_stream.flush().await?;
    println!("[xHTTP POST TLS] 200 OK enviado");

    Ok(())
}

/// xHTTP POST raw (sem TLS)
async fn handle_xhttp_post_raw<S: AsyncReadExt + AsyncWriteExt + Unpin>(
    stream: &mut S,
    full_request: &[u8],
    path: &str,
    status: &str,
) -> Result<(), Error> {
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    let session_id = if parts.len() >= 2 { parts[1].to_string() } else { String::new() };

    println!("[xHTTP POST RAW] Session: {}", session_id);

    let content_length = extract_content_length_from_bytes(full_request).unwrap_or(0);

    if content_length == 0 {
        let resp = format!("HTTP/1.1 200 OK\r\nContent-Length: 0\r\nX-Status: {}\r\n\r\n", status);
        stream.write_all(resp.as_bytes()).await?;
        return Ok(());
    }

    let header_end = full_request.windows(4).position(|w| w == b"\r\n\r\n").unwrap_or(0) + 4;
    let body_in_request = full_request.len() - header_end;

    let sessions = SESSIONS.lock().await;
    if let Some(session) = sessions.get(&session_id) {
        let body = if body_in_request >= content_length {
            full_request[header_end..header_end + content_length].to_vec()
        } else {
            let mut body = full_request[header_end..].to_vec();
            let remaining = content_length - body_in_request;
            let mut buf = vec![0u8; remaining];
            let mut read = 0;
            while read < remaining {
                match timeout(Duration::from_secs(30), stream.read(&mut buf[read..])).await {
                    Ok(Ok(n)) if n > 0 => read += n,
                    _ => break,
                }
            }
            body.extend_from_slice(&buf[..read]);
            body[..content_length].to_vec()
        };

        if session.post_tx.send(body).await.is_err() {
            println!("[xHTTP POST RAW] Canal POST fechado");
        }
    } else {
        println!("[xHTTP POST RAW] Sessão {} não encontrada!", session_id);
        let resp = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n";
        stream.write_all(resp.as_bytes()).await?;
        return Ok(());
    }

    let resp = "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: keep-alive\r\n\r\n";
    stream.write_all(resp.as_bytes()).await?;
    stream.flush().await?;

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
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    if parts.len() >= 2 {
        parts[1].to_string()
    } else {
        String::new()
    }
}

fn generate_session_id() -> String {
    use std::time::SystemTime;
    let t = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap();
    format!("{:x}{:x}", t.as_secs(), t.subsec_nanos())
}

fn extract_content_length_from_bytes(data: &[u8]) -> Option<usize> {
    let s = String::from_utf8_lossy(data);
    for line in s.lines() {
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

fn get_port() -> u16 {
    let args: Vec<String> = std::env::args().collect();
    let mut port = 443;
    for i in 1..args.len() {
        if args[i] == "--port" || args[i] == "-p" {
            if i + 1 < args.len() {
                port = args[i + 1].parse().unwrap_or(443);
            }
        }
    }
    port
}

fn get_status() -> String {
    let args: Vec<String> = std::env::args().collect();
    let mut status = String::from("@SDProxy");
    for i in 1..args.len() {
        if args[i] == "--status" || args[i] == "-s" {
            if i + 1 < args.len() {
                status = args[i + 1].clone();
            }
        }
    }
    status
}

fn get_ssh_port() -> u16 {
    let args: Vec<String> = std::env::args().collect();
    let mut port = 22;
    for i in 1..args.len() {
        if args[i] == "--ssh-port" {
            if i + 1 < args.len() {
                port = args[i + 1].parse().unwrap_or(22);
            }
        }
    }
    port
}
