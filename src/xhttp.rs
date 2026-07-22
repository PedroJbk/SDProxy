//! xHTTP (SplitHTTP) Handler
//! Protocolo real usado pelo SocksRevive-XHTTP-DEMO
//!
//! Fluxo:
//! 1. Client → TLS handshake (0x16 0x03)
//! 2. Client → HTTP/2 GET /{basePath}/{sessionId} → streaming downlink
//! 3. Client → HTTP/2 POST /{basePath}/{sessionId}/{seq} → uplink sequenciado
//! 4. Dados SSH viajam dentro dos streams HTTP/2
//!
//! O servidor precisa:
//! - Terminar TLS (decodificar)
//! - Parsear HTTP/2 GET/POST
//! - Manter sessões ativas
//! - Bridge: GET response body ↔ SSH backend
//! - Bridge: POST body → SSH backend

use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::time::{timeout, Duration};

/// Sessão xHTTP ativa
struct XhttpSession {
    ssh_stream: TcpStream,
    uplink_sequence: u64,
    /// Buffer para dados SSH recebidos via POST
    ssh_buffer: Vec<u8>,
    /// Flag: conexão SSH ativa
    active: bool,
}

/// Sessões ativas keyed por session_id
lazy_static::lazy_static! {
    static ref SESSIONS: Arc<Mutex<HashMap<String, XhttpSession>>> = Arc::new(Mutex::new(HashMap::new()));
}

/// Handler principal xHTTP
/// Aceita requisições HTTP GET e POST para streaming SSH over HTTP/2
pub async fn handle_xhttp(
    mut stream: TcpStream,
    status: &str,
    ssh_only: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("[xHTTP] Nova conexão recebida");

    // Ler a requisição HTTP do cliente
    let mut request_buf = vec![0u8; 65536];
    let read_result = timeout(Duration::from_secs(10), stream.read(&mut request_buf)).await;

    let request_data = match read_result {
        Ok(Ok(n)) if n > 0 => {
            String::from_utf8_lossy(&request_buf[..n]).to_string()
        }
        _ => {
            println!("[xHTTP] Timeout ou erro na leitura da requisição");
            return Ok(());
        }
    };

    println!("[xHTTP] Request: {} bytes", request_data.len());
    println!("[xHTTP] Preview: {}", &request_data[..std::cmp::min(request_data.len(), 500)]);

    // Parsear a requisição HTTP
    if let Some((method, path)) = parse_http_request(&request_data) {
        println!("[xHTTP] Method: {} Path: {}", method, path);

        match method.as_str() {
            "GET" => {
                handle_xhttp_get(&mut stream, &path, status, ssh_only).await?;
            }
            "POST" => {
                handle_xhttp_post(&mut stream, &request_data, &path, ssh_only).await?;
            }
            _ => {
                println!("[xHTTP] Método não suportado: {}", method);
                send_404(&mut stream, status).await?;
            }
        }
    } else {
        println!("[xHTTP] Falha ao parsear requisição HTTP");
        send_404(&mut stream, status).await?;
    }

    Ok(())
}

/// Parsear requisição HTTP básica
fn parse_http_request(data: &str) -> Option<(String, String)> {
    let first_line = data.lines().next()?;
    let parts: Vec<&str> = first_line.split_whitespace().collect();

    if parts.len() >= 2 {
        Some((parts[0].to_string(), parts[1].to_string()))
    } else {
        None
    }
}

/// Handler GET - Streaming downlink
/// Cliente envia GET /{basePath}/{sessionId}
/// Servidor responde com HTTP/2 streaming + chunked encoding
async fn handle_xhttp_get(
    stream: &mut TcpStream,
    path: &str,
    status: &str,
    ssh_only: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Extrair session_id do path
    // Formato: /ssh/{sessionId} ou /{sessionId}
    let session_id = extract_session_id(path);

    if session_id.is_empty() {
        println!("[xHTTP] GET sem session_id válido");
        send_404(stream, status).await?;
        return Ok(());
    }

    println!("[xHTTP] GET session_id: {}", session_id);

    // Conectar ao backend SSH
    let addr = if ssh_only { "127.0.0.1:22" } else { "127.0.0.1:22" };

    match TcpStream::connect(addr).await {
        Ok(ssh_stream) => {
            println!("[xHTTP] SSH backend conectado para session {}", session_id);

            // Criar sessão
            {
                let mut sessions = SESSIONS.lock().await;
                sessions.insert(session_id.clone(), XhttpSession {
                    ssh_stream,
                    uplink_sequence: 0,
                    ssh_buffer: Vec::new(),
                    active: true,
                });
            }

            // Enviar HTTP response com streaming
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

            println!("[xHTTP] GET response enviada, iniciando streaming para session {}", session_id);

            // Manter conexão aberta e fazer tunnel SSH
            // O SSH backend vai enviar dados que precisamos repassar ao cliente
            let mut sessions = SESSIONS.lock().await;
            if let Some(session) = sessions.get_mut(&session_id) {
                // Ler dados do SSH backend e enviar como chunks HTTP
                let mut buffer = [0u8; 4096];
                loop {
                    match timeout(Duration::from_secs(60), session.ssh_stream.read(&mut buffer)).await {
                        Ok(Ok(0)) => {
                            println!("[xHTTP] SSH stream fechado (EOF)");
                            break;
                        }
                        Ok(Ok(n)) => {
                            // Enviar como chunk HTTP
                            let chunk_header = format!("{:x}\r\n", n);
                            if let Err(e) = stream.write_all(chunk_header.as_bytes()).await {
                                println!("[xHTTP] Erro ao escrever chunk header: {}", e);
                                break;
                            }
                            if let Err(e) = stream.write_all(&buffer[..n]).await {
                                println!("[xHTTP] Erro ao escrever chunk data: {}", e);
                                break;
                            }
                            if let Err(e) = stream.write_all(b"\r\n").await {
                                println!("[xHTTP] Erro ao escrever chunk footer: {}", e);
                                break;
                            }
                            if let Err(e) = stream.flush().await {
                                println!("[xHTTP] Erro ao flush: {}", e);
                                break;
                            }
                        }
                        Ok(Err(e)) => {
                            println!("[xHTTP] Erro ao ler SSH: {}", e);
                            break;
                        }
                        Err(_) => {
                            // Timeout - enviar chunk vazio para manter alive
                            if let Err(e) = stream.write_all(b"0\r\n\r\n").await {
                                break;
                            }
                            if let Err(e) = stream.flush().await {
                                break;
                            }
                        }
                    }
                }
            }

            // Remover sessão
            {
                let mut sessions = SESSIONS.lock().await;
                if let Some(session) = sessions.get_mut(&session_id) {
                    session.active = false;
                }
                sessions.remove(&session_id);
            }

            // Enviar chunk final
            let _ = stream.write_all(b"0\r\n\r\n").await;
            let _ = stream.flush().await;

            println!("[xHTTP] Streaming encerrado para session {}", session_id);
            Ok(())
        }
        Err(e) => {
            println!("[xHTTP] Falha ao conectar SSH backend: {}", e);
            send_502(stream, status).await?;
            Ok(())
        }
    }
}

/// Handler POST - Uplink sequenciado
/// Cliente envia POST /{basePath}/{sessionId}/{sequence}
/// Body contém dados SSH para enviar ao backend
async fn handle_xhttp_post(
    stream: &mut TcpStream,
    full_request: &str,
    path: &str,
    ssh_only: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Extrair session_id e sequence do path
    // Formato: /ssh/{sessionId}/{sequence}
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();

    let session_id = if parts.len() >= 2 { parts[1].to_string() } else { String::new() };
    let sequence = if parts.len() >= 3 { parts[2].parse::<u64>().unwrap_or(0) } else { 0 };

    if session_id.is_empty() {
        println!("[xHTTP] POST sem session_id válido");
        send_404(stream, "@SDProxy").await?;
        return Ok(());
    }

    println!("[xHTTP] POST session={} seq={}", session_id, sequence);

    // Extrair Content-Length para saber quanto ler do body
    let content_length = extract_content_length(full_request).unwrap_or(0);

    if content_length == 0 {
        println!("[xHTTP] POST sem body");
        send_200(stream, "@SDProxy").await?;
        return Ok(());
    }

    // Ler o body
    let mut body_buf = vec![0u8; content_length as usize];
    match timeout(Duration::from_secs(30), stream.read_exact(&mut body_buf)).await {
        Ok(Ok(_)) => {
            println!("[xHTTP] POST body recebido: {} bytes", body_buf.len());

            // Encontrar sessão e enviar dados ao SSH backend
            let mut sessions = SESSIONS.lock().await;
            if let Some(session) = sessions.get_mut(&session_id) {
                if session.active {
                    // Enviar dados ao SSH backend
                    match session.ssh_stream.write_all(&body_buf).await {
                        Ok(_) => {
                            session.uplink_sequence = sequence + 1;
                            send_200(stream, "@SDProxy").await?;
                        }
                        Err(e) => {
                            println!("[xHTTP] Erro ao enviar ao SSH: {}", e);
                            send_500(stream, "@SDProxy").await?;
                        }
                    }
                } else {
                    println!("[xHTTP] Sessão {} inativa", session_id);
                    send_410(stream, "@SDProxy").await?;
                }
            } else {
                println!("[xHTTP] Sessão {} não encontrada", session_id);
                send_404(stream, "@SDProxy").await?;
            }
        }
        _ => {
            println!("[xHTTP] Timeout ou erro ao ler POST body");
            send_408(stream, "@SDProxy").await?;
        }
    }

    Ok(())
}

/// Handler Proto (fallback para outros protocolos)
pub async fn handle_proto(
    mut stream: TcpStream,
    ssh_only: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("[Proto] Nova conexão proto");

    let addr = if ssh_only { "127.0.0.1:22" } else { "127.0.0.1:22" };

    match TcpStream::connect(addr).await {
        Ok(mut backend) => {
            let (cr, cw) = stream.into_split();
            let (sr, sw) = backend.into_split();
            let cr = Arc::new(Mutex::new(cr));
            let cw = Arc::new(Mutex::new(cw));
            let sr = Arc::new(Mutex::new(sr));
            let sw = Arc::new(Mutex::new(sw));
            tokio::try_join!(
                transfer_bidirectional(cr, sw),
                transfer_bidirectional(sr, cw),
            )?;
        }
        Err(e) => {
            println!("[Proto] Erro backend: {}", e);
        }
    }

    Ok(())
}

// Helper functions

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

async fn send_200(stream: &mut TcpStream, status: &str) -> Result<(), std::io::Error> {
    stream.write_all(format!("HTTP/1.1 200 OK\r\nX-Status: {}\r\nContent-Length: 0\r\n\r\n", status).as_bytes()).await
}

async fn send_404(stream: &mut TcpStream, status: &str) -> Result<(), std::io::Error> {
    stream.write_all(format!("HTTP/1.1 404 Not Found\r\nX-Status: {}\r\nContent-Length: 0\r\n\r\n", status).as_bytes()).await
}

async fn send_502(stream: &mut TcpStream, status: &str) -> Result<(), std::io::Error> {
    stream.write_all(format!("HTTP/1.1 502 Bad Gateway\r\nX-Status: {}\r\nContent-Length: 0\r\n\r\n", status).as_bytes()).await
}

async fn send_500(stream: &mut TcpStream, status: &str) -> Result<(), std::io::Error> {
    stream.write_all(format!("HTTP/1.1 500 Internal Server Error\r\nX-Status: {}\r\nContent-Length: 0\r\n\r\n", status).as_bytes()).await
}

async fn send_410(stream: &mut TcpStream, status: &str) -> Result<(), std::io::Error> {
    stream.write_all(format!("HTTP/1.1 410 Gone\r\nX-Status: {}\r\nContent-Length: 0\r\n\r\n", status).as_bytes()).await
}

async fn send_408(stream: &mut TcpStream, status: &str) -> Result<(), std::io::Error> {
    stream.write_all(format!("HTTP/1.1 408 Request Timeout\r\nX-Status: {}\r\nContent-Length: 0\r\n\r\n", status).as_bytes()).await
}

async fn transfer_bidirectional(
    read_stream: Arc<Mutex<tokio::net::tcp::OwnedReadHalf>>,
    write_stream: Arc<Mutex<tokio::net::tcp::OwnedWriteHalf>>,
) -> Result<(), std::io::Error> {
    let mut buffer = [0u8; 8192];
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
