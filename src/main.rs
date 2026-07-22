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
mod udp;
mod quic;

#[tokio::main]
async fn main() -> Result<(), Error> {
    env_logger::init();
    println!("[SDProxy v0.3.0] Multi-Protocolo Proxy");
    println!("[SDProxy] Iniciando...");

    // Parse de argumentos
    let config = parse_args();
    println!("[SDProxy] Porta TCP: {}", config.port);
    println!("[SDProxy] Porta QUIC: {}", config.quic_port);
    println!("[SDProxy] Status: {}", config.status);
    println!("[SDProxy] TLS habilitado: {}", config.tls_enabled);
    println!("[SDProxy] SSH only: {}", config.ssh_only);
    println!("[SDProxy] UDP ativo: {}", config.udp_enabled);
    println!("[SDProxy] QUIC ativo: {}", config.quic_enabled);

    // Caminhos dos certificados QUIC
    let cert_path = "/opt/sdproxy/cert.pem";
    let key_path = "/opt/sdproxy/key.pem";

    // Garantir diretório
    let _ = std::fs::create_dir_all("/opt/sdproxy");

    // Iniciar listeners em paralelo
    let config_tcp = Arc::new(config.clone());

    let mut handles = Vec::new();

    // TCP listener (sempre ativo)
    let tcp_config = config_tcp.clone();
    handles.push(tokio::spawn(async move {
        if let Err(e) = start_tcp(tcp_config).await {
            eprintln!("[SDProxy] Erro TCP: {}", e);
        }
    }));

    // UDP listener (se habilitado)
    if config.udp_enabled {
        let udp_port = config.port;
        let ssh_only = config.ssh_only;
        handles.push(tokio::spawn(async move {
            if let Err(e) = udp::handle_udp_listener(udp_port, ssh_only).await {
                eprintln!("[SDProxy] Erro UDP: {}", e);
            }
        }));
    }

    // QUIC listener (se habilitado)
    if config.quic_enabled {
        let quic_cert = cert_path.to_string();
        let quic_key = key_path.to_string();
        let quic_port = config.quic_port;
        let ssh_only = config.ssh_only;
        handles.push(tokio::spawn(async move {
            if let Err(e) = quic::start_quic_server(quic_port, &quic_cert, &quic_key, ssh_only).await {
                eprintln!("[SDProxy] Erro QUIC: {}", e);
            }
        }));
    }

    // Aguardar todas as tasks
    println!("[SDProxy] Todos os protocolos iniciados. Aguardando conexões...");
    for handle in handles {
        let _ = handle.await;
    }

    Ok(())
}

#[derive(Clone)]
struct Config {
    port: u16,
    quic_port: u16,
    status: String,
    tls_enabled: bool,
    ssh_only: bool,
    udp_enabled: bool,
    quic_enabled: bool,
}

fn parse_args() -> Config {
    let args: Vec<String> = env::args().collect();
    let mut port: u16 = 80;
    let mut quic_port: u16 = 8001;
    let mut status = String::from("@SDProxy");
    let mut tls_enabled = false;
    let mut ssh_only = false;
    let mut udp_enabled = false;
    let mut quic_enabled = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--port" | "-p" => {
                if i + 1 < args.len() {
                    port = args[i + 1].parse().unwrap_or(80);
                    i += 1;
                }
            }
            "--quic-port" => {
                if i + 1 < args.len() {
                    quic_port = args[i + 1].parse().unwrap_or(8001);
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
            "--udp" | "-u" => {
                udp_enabled = true;
            }
            "--quic" | "-q" => {
                quic_enabled = true;
            }
            _ => {}
        }
        i += 1;
    }

    // TLS habilita UDP e QUIC automaticamente na porta 443
    if tls_enabled && port == 443 {
        udp_enabled = true;
        quic_enabled = true;
    }

    Config {
        port,
        quic_port,
        status,
        tls_enabled,
        ssh_only,
        udp_enabled,
        quic_enabled,
    }
}

async fn start_tcp(config: Arc<Config>) -> Result<(), Error> {
    let listener = TcpListener::bind(format!("[::]:{}", config.port)).await?;
    println!("[SDProxy] TCP rodando na porta: {}", config.port);

    loop {
        match listener.accept().await {
            Ok((client_stream, addr)) => {
                let config = config.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_client(client_stream, &config).await {
                        println!("[SDProxy] Erro TCP cliente {}: {}", addr, e);
                    }
                });
            }
            Err(e) => {
                println!("[SDProxy] Erro ao aceitar TCP: {}", e);
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

    // Detectar protocolo
    let proto = protocol::detect_protocol(&peek_data);
    println!("[SDProxy] TCP - Protocolo detectado: {} ({} bytes)", proto, peek_data.len());

    match proto.as_str() {
        "TLS" => {
            if config.tls_enabled {
                if let Err(e) = tls::handle_tls(client_stream, config.ssh_only).await {
                    println!("[SDProxy] Erro TLS: {}", e);
                }
            } else {
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
                    println!("[SDProxy] Erro SSH: {}", e);
                }
            } else {
                if peek_data.contains("SSH-") {
                    if let Err(e) = ssh::handle_ssh_tunnel(client_stream, "127.0.0.1:22").await {
                        println!("[SDProxy] Erro SSH: {}", e);
                    }
                } else {
                    // Handshake HTTP básico
                    let _ = client_stream.write_all(
                        format!("HTTP/1.1 101 {}\r\n\r\n", config.status).as_bytes()
                    ).await;
                    let _ = client_stream.write_all(
                        format!("HTTP/1.1 200 {}\r\n\r\n", config.status).as_bytes()
                    ).await;

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
                            println!("[SDProxy] Erro backend: {}", e);
                        }
                    }
                }
            }
        }
        _ => {
            // Fallback TCP
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
