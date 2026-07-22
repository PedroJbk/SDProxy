use std::env;
use std::io::Error;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tokio::time::{timeout, Duration};

mod protocol;
mod websocket;
mod security;
mod tcp_fallback;
mod tls;
mod ssh;
mod xhttp;
mod socks5;

#[tokio::main]
async fn main() -> Result<(), Error> {
    env_logger::init();
    println!("[SDProxy] Iniciando...");

    // Parse de argumentos
    let config = parse_args();
    println!("[SDProxy] Porta: {}", config.port);
    println!("[SDProxy] Status: {}", config.status);
    println!("[SDProxy] TLS habilitado: {}", config.tls_enabled);
    println!("[SDProxy] SSH only: {}", config.ssh_only);

    let listener = TcpListener::bind(format!("[::]:{}", config.port)).await?;
    println!("[SDProxy] Serviço rodando na porta: {}", config.port);

    start_http(listener, config).await;
    Ok(())
}

struct Config {
    port: u16,
    status: String,
    tls_enabled: bool,
    ssh_only: bool,
}

fn parse_args() -> Config {
    let args: Vec<String> = env::args().collect();
    let mut port: u16 = 80;
    let mut status = String::from("@SDProxy");
    let mut tls_enabled = false;
    let mut ssh_only = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--port" | "-p" => {
                if i + 1 < args.len() {
                    port = args[i + 1].parse().unwrap_or(80);
                    i += 1;
                }
            }
            "--status" | "-s" => {
                if i + 1 < args.len() {
                    status = args[i + 1].clone();
                    i += 1;
                }
            }
            "--tls" | "-t" => {
                tls_enabled = true;
            }
            "--ssh" | "-ssh" => {
                ssh_only = true;
            }
            _ => {}
        }
        i += 1;
    }

    Config {
        port,
        status,
        tls_enabled,
        ssh_only,
    }
}

async fn start_http(listener: TcpListener, config: Config) {
    let config = Arc::new(config);
    loop {
        match listener.accept().await {
            Ok((client_stream, addr)) => {
                let config = config.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_client(client_stream, &config).await {
                        println!("[SDProxy] Erro ao processar cliente {}: {}", addr, e);
                    }
                });
            }
            Err(e) => {
                println!("[SDProxy] Erro ao aceitar conexão: {}", e);
            }
        }
    }
}

async fn handle_client(mut client_stream: TcpStream, config: &Config) -> Result<(), Error> {
    // Peek nos primeiros bytes para detectar o protocolo
    let mut peek_buf = vec![0u8; 8192];
    let peek_result = timeout(Duration::from_secs(2), client_stream.peek(&mut peek_buf)).await;

    let peek_data = match peek_result {
        Ok(Ok(n)) if n > 0 => {
            String::from_utf8_lossy(&peek_buf[..n]).to_string()
        }
        _ => {
            String::new()
        }
    };

    // Detectar protocolo usando o módulo protocol
    let proto = protocol::detect_protocol(&peek_data);
    println!("[SDProxy] Cliente detectado - Protocolo: {}", proto);

    match proto.as_str() {
        "TLS" => {
            if config.tls_enabled {
                // TLS com terminação no proxy
                if let Err(e) = tls::handle_tls(client_stream, config.ssh_only).await {
                    println!("[SDProxy] Erro TLS terminação: {}", e);
                }
            } else {
                // TLS passthrough direto
                if let Err(e) = tls::handle_tls_terminated(client_stream, config.ssh_only).await {
                    println!("[SDProxy] Erro TLS passthrough: {}", e);
                }
            }
        }
        "WEBSOCKET" => {
            if let Err(e) = websocket::handle_websocket(client_stream, &config.status).await {
                println!("[SDProxy] Erro WebSocket: {}", e);
            }
        }
        "XHTTP" => {
            if let Err(e) = xhttp::handle_xhttp(client_stream, &config.status, config.ssh_only).await {
                println!("[SDProxy] Erro xHTTP: {}", e);
            }
        }
        "PROTO" => {
            if let Err(e) = xhttp::handle_proto(client_stream, config.ssh_only).await {
                println!("[SDProxy] Erro Proto: {}", e);
            }
        }
        "SOCKS5" => {
            if let Err(e) = socks5::handle_socks5(client_stream).await {
                println!("[SDProxy] Erro SOCKS5: {}", e);
            }
        }
        "HTTP" | "SECURITY" => {
            if let Err(e) = security::handle_security(client_stream, &config.status).await {
                println!("[SDProxy] Erro HTTP/Security: {}", e);
            }
        }
        "SSH" => {
            if config.ssh_only {
                if let Err(e) = ssh::handle_ssh_tunnel(client_stream, "127.0.0.1:22").await {
                    println!("[SDProxy] Erro SSH tunnel: {}", e);
                }
            } else {
                // Detectar se é SSH puro ou dados mistos
                if peek_data.contains("SSH-") {
                    if let Err(e) = ssh::handle_ssh_tunnel(client_stream, "127.0.0.1:22").await {
                        println!("[SDProxy] Erro SSH tunnel: {}", e);
                    }
                } else {
                    // Handshake HTTP básico (101 + 200)
                    let _ = client_stream.write_all(
                        format!("HTTP/1.1 101 {}\r\n\r\n", config.status).as_bytes()
                    ).await;
                    let _ = client_stream.write_all(
                        format!("HTTP/1.1 200 {}\r\n\r\n", config.status).as_bytes()
                    ).await;

                    // Determinar backend
                    let addr_proxy = if peek_data.contains("SSH") || peek_data.is_empty() {
                        "127.0.0.1:22"
                    } else {
                        "127.0.0.1:1194"
                    };

                    match TcpStream::connect(addr_proxy).await {
                        Ok(server) => {
                            let (cr, cw) = client_stream.into_split();
                            let (sr, sw) = server.into_split();
                            let cr = Arc::new(Mutex::new(cr));
                            let cw = Arc::new(Mutex::new(cw));
                            let sr = Arc::new(Mutex::new(sr));
                            let sw = Arc::new(Mutex::new(sw));
                            let _ = tokio::try_join!(
                                transfer_data(cr, sw),
                                transfer_data(sr, cw),
                            );
                        }
                        Err(e) => {
                            println!("[SDProxy] Erro ao conectar ao backend: {}", e);
                        }
                    }
                }
            }
        }
        _ => {
            // Fallback TCP - encaminhar diretamente
            if let Err(e) = tcp_fallback::handle_tcp(client_stream).await {
                println!("[SDProxy] Erro TCP fallback: {}", e);
            }
        }
    }

    Ok(())
}

async fn transfer_data(
    read_stream: Arc<Mutex<tokio::net::tcp::OwnedReadHalf>>,
    write_stream: Arc<Mutex<tokio::net::tcp::OwnedWriteHalf>>,
) -> Result<(), Error> {
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
