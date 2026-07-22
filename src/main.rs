use std::env;
use std::io::Error;
use tokio::io::{AsyncWriteExt, copy_bidirectional};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{timeout, Duration};

mod socks5;
mod websocket;
mod security;
mod tcp_fallback;
mod tls;

#[tokio::main]
async fn main() -> Result<(), Error> {
    let args: Vec<String> = env::args().collect();
    let config = parse_args(&args);

    let port = config.port;
    let status = config.status.clone();
    let use_tls = config.tls;
    let ssh_only = config.ssh_only;

    let listener = TcpListener::bind(format!("[::]:{}", port)).await?;
    println!("Servidor iniciado na porta: {}", port);

    start_proxy(listener, status, ssh_only, use_tls).await;
    Ok(())
}

async fn start_proxy(listener: TcpListener, status: String, ssh_only: bool, use_tls: bool) {
    loop {
        let status_clone = status.clone();
        match listener.accept().await {
            Ok((client_stream, addr)) => {
                tokio::spawn(async move {
                    if let Err(e) = handle_client(client_stream, &status_clone, ssh_only, use_tls).await {
                        eprintln!("Erro ao processar cliente {}: {}", addr, e);
                    }
                });
            }
            Err(e) => eprintln!("Erro ao aceitar conexão: {}", e),
        }
    }
}

async fn handle_client(mut client_stream: TcpStream, status: &str, ssh_only: bool, use_tls: bool) -> Result<(), Error> {

    if use_tls {
        return tls::handle_tls(client_stream).await.map_err(|e| Error::new(std::io::ErrorKind::Other, e));
    }

    if ssh_only {
        // Aplica a lógica de Tripla Resposta mesmo em SSH Only se for HTTP
        let mut buffer = [0u8; 1024];
        let bytes_peeked = match timeout(Duration::from_millis(500), client_stream.peek(&mut buffer)).await {
            Ok(Ok(n)) => n,
            _ => 0,
        };

        if bytes_peeked > 0 {
            let data = String::from_utf8_lossy(&buffer[..bytes_peeked]);
            if is_http_request(&data) {
                return websocket::handle_websocket(client_stream, status).await.map_err(|e| Error::new(std::io::ErrorKind::Other, e));
            }
        }

        // Se não for HTTP, faz o túnel direto
        let mut server_stream = match TcpStream::connect("127.0.0.1:22").await {
            Ok(s) => s,
            Err(_) => return Ok(()),
        };
        let _ = copy_bidirectional(&mut client_stream, &mut server_stream).await;
        return Ok(());
    }

    // Espiada rápida no buffer (Peek)
    let mut buffer = [0u8; 1024];
    let bytes_read = match timeout(Duration::from_millis(500), client_stream.peek(&mut buffer)).await {
        Ok(Ok(n)) => n,
        _ => 0,
    };

    if bytes_read > 0 {
        let first_byte = buffer[0];
        let data = String::from_utf8_lossy(&buffer[..bytes_read]);

        // 1. SOCKS5
        if first_byte == 0x05 {
            return socks5::handle_socks5(client_stream).await.map_err(|e| Error::new(std::io::ErrorKind::Other, e));
        }

        // 2. TLS/SSL Handshake (0x16)
        if first_byte == 0x16 {
            return tls::handle_tls(client_stream).await.map_err(|e| Error::new(std::io::ErrorKind::Other, e));
        }

        // 3. HTTP / WebSocket / Custom Methods
        if is_http_request(&data) {
            // Se contiver SECURITY ou métodos como ACL/PATCH/etc, usamos a lógica de Security
            if data.contains("SECURITY") || data.contains("Upgrade: security") || data.starts_with("ACL") || data.starts_with("PATCH") {
                return security::handle_security(client_stream, status).await.map_err(|e| Error::new(std::io::ErrorKind::Other, e));
            }
            // Aqui injetamos a nova lógica de Tripla Resposta para WebSocket padrão
            return websocket::handle_websocket(client_stream, status).await.map_err(|e| Error::new(std::io::ErrorKind::Other, e));
        }
    }

    // Fallback: TCP puro
    tcp_fallback::handle_tcp(client_stream).await.map_err(|e| Error::new(std::io::ErrorKind::Other, e))
}

fn is_http_request(data: &str) -> bool {
    let methods = ["GET", "POST", "PUT", "DELETE", "CONNECT", "OPTIONS", "HEAD", "PATCH", "ACL", "MOVE", "PROPFIND"];
    for m in methods {
        if data.starts_with(m) { return true; }
    }
    data.contains("HTTP/1.") || data.contains("HTTP/2.")
}

struct ProxyConfig {
    port: u16,
    status: String,
    tls: bool,
    ssh_only: bool,
}

fn parse_args(args: &[String]) -> ProxyConfig {
    let mut port = 80u16;
    let mut status = "200 OK".to_string();
    let mut tls = false;
    let mut ssh_only = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-p" => { if i+1 < args.len() { port = args[i+1].parse().unwrap_or(80); i+=1; } }
            "-s" => { if i+1 < args.len() { status = args[i+1].clone(); i+=1; } }
            "-t" => { tls = true; }
            "-ssh" => { ssh_only = true; }
            _ => {}
        }
        i += 1;
    }
    ProxyConfig { port, status, tls, ssh_only }
}
