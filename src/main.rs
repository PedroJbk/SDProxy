use std::env;
use std::io::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt, copy_bidirectional};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{timeout, Duration};

// Módulos de protocolos
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

    if use_tls {
        println!("TLS habilitado");
    }

    if ssh_only {
        println!("Modo SSH apenas habilitado");
    }

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
        // Modo TLS/HTTPS - usa o handler TLS
        if let Err(e) = tls::handle_tls(client_stream).await {
            eprintln!("Erro TLS: {}", e);
        }
        return Ok(());
    }

    if ssh_only {
        // Modo SSH apenas - envia resposta HTTP e encaminha direto para SSH
        client_stream
            .write_all(format!("HTTP/1.1 101 {}\r\n\r\n", status).as_bytes())
            .await?;

        let mut buffer = [0; 1024];
        client_stream.read(&mut buffer).await?;

        client_stream
            .write_all(format!("HTTP/1.1 101 {}\r\n\r\n", status).as_bytes())
            .await?;

        client_stream
            .write_all(format!("HTTP/1.1 200 {}\r\n\r\n", status).as_bytes())
            .await?;

        let mut server_stream = match TcpStream::connect("0.0.0.0:22").await {
            Ok(stream) => stream,
            Err(_) => {
                eprintln!("Erro ao conectar-se ao SSH");
                return Ok(());
            }
        };

        let _ = copy_bidirectional(&mut client_stream, &mut server_stream).await;
        return Ok(());
    }

    // Modo automático - detectar protocolo
    let mut buffer = [0u8; 8192];
    let bytes_read = match timeout(Duration::from_secs(5), client_stream.peek(&mut buffer)).await {
        Ok(Ok(n)) => n,
        Ok(Err(e)) => return Err(e),
        Err(_) => {
            // Timeout - assume TCP fallback
            return tcp_fallback::handle_tcp(client_stream).await.map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::Other, "TCP fallback error")
            });
        }
    };

    let data = String::from_utf8_lossy(&buffer[..bytes_read]).to_string();

    // Detectar protocolo pelo primeiro byte
    if bytes_read > 0 {
        let first_byte = buffer[0];

        match first_byte {
            // SOCKS5 - byte 0x05
            0x05 => {
                return socks5::handle_socks5(client_stream).await.map_err(|_| {
                    std::io::Error::new(std::io::ErrorKind::Other, "SOCKS5 error")
                });
            }

            // TLS Client Hello - byte 0x16
            0x16 => {
                if let Err(e) = tls::handle_tls(client_stream).await {
                    eprintln!("Erro TLS handshake: {}", e);
                }
                return Ok(());
            }

            // HTTP/WebSocket - começa com GET ou CONNECT
            _ if data.starts_with("GET") || data.starts_with("CONNECT") || data.starts_with("POST") || data.starts_with("PUT") || data.starts_with("DELETE") || data.starts_with("OPTIONS") || data.starts_with("HEAD") || data.starts_with("PATCH") => {
                // WebSocket ou HTTP
                return websocket::handle_websocket(client_stream).await.map_err(|_| {
                    std::io::Error::new(std::io::ErrorKind::Other, "WebSocket error")
                });
            }

            // SECURITY - contém "AUTH" ou "SECURITY"
            _ if data.contains("AUTH") || data.contains("SECURITY") => {
                return security::handle_security(client_stream).await.map_err(|_| {
                    std::io::Error::new(std::io::ErrorKind::Other, "Security error")
                });
            }

            // SSH - começa com "SSH" ou byte 0x00 seguido de "SSH"
            _ if data.contains("SSH") || data.contains("\x00SSH") => {
                // Encaminhar para SSH diretamente
                client_stream
                    .write_all(format!("HTTP/1.1 101 {}\r\n\r\n", status).as_bytes())
                    .await?;

                client_stream.read(&mut buffer).await?;

                let mut server_stream = match TcpStream::connect("0.0.0.0:22").await {
                    Ok(stream) => stream,
                    Err(_) => {
                        eprintln!("Erro ao conectar-se ao SSH");
                        return Ok(());
                    }
                };

                let _ = copy_bidirectional(&mut client_stream, &mut server_stream).await;
                return Ok(());
            }

            _ => {}
        }
    }

    // Fallback: padrão é enviar resposta HTTP e encaminhar para SSH ou VPN
    // Detectar se é SSH pelo conteúdo ou enviar resposta HTTP
    if data.is_empty() {
        // Dados vazios - assume SSH
        client_stream
            .write_all(format!("HTTP/1.1 101 {}\r\n\r\n", status).as_bytes())
            .await?;

        client_stream.read(&mut buffer).await?;

        client_stream
            .write_all(format!("HTTP/1.1 101 {}\r\n\r\n", status).as_bytes())
            .await?;

        client_stream
            .write_all(format!("HTTP/1.1 200 {}\r\n\r\n", status).as_bytes())
            .await?;

        let mut server_stream = match TcpStream::connect("0.0.0.0:22").await {
            Ok(stream) => stream,
            Err(_) => {
                eprintln!("Erro ao conectar-se ao SSH");
                return Ok(());
            }
        };

        let _ = copy_bidirectional(&mut client_stream, &mut server_stream).await;
    } else {
        // Default - TCP fallback para VPN (1194) ou SSH (22)
        match timeout(Duration::from_secs(5), peek_stream(&mut client_stream)).await {
            Ok(Ok(peek_data)) if peek_data.contains("SSH") || peek_data.is_empty() => {
                // SSH
                client_stream
                    .write_all(format!("HTTP/1.1 101 {}\r\n\r\n", status).as_bytes())
                    .await?;

                client_stream.read(&mut buffer).await?;

                client_stream
                    .write_all(format!("HTTP/1.1 101 {}\r\n\r\n", status).as_bytes())
                    .await?;

                client_stream
                    .write_all(format!("HTTP/1.1 200 {}\r\n\r\n", status).as_bytes())
                    .await?;

                let mut server_stream = match TcpStream::connect("0.0.0.0:22").await {
                    Ok(stream) => stream,
                    Err(_) => {
                        eprintln!("Erro ao conectar-se ao SSH");
                        return Ok(());
                    }
                };

                let _ = copy_bidirectional(&mut client_stream, &mut server_stream).await;
            }
            _ => {
                // VPN/OpenVPN
                client_stream
                    .write_all(format!("HTTP/1.1 101 {}\r\n\r\n", status).as_bytes())
                    .await?;

                client_stream.read(&mut buffer).await?;

                client_stream
                    .write_all(format!("HTTP/1.1 101 {}\r\n\r\n", status).as_bytes())
                    .await?;

                client_stream
                    .write_all(format!("HTTP/1.1 200 {}\r\n\r\n", status).as_bytes())
                    .await?;

                let mut server_stream = match TcpStream::connect("0.0.0.0:1194").await {
                    Ok(stream) => stream,
                    Err(_) => {
                        eprintln!("Erro ao conectar-se ao VPN");
                        return Ok(());
                    }
                };

                let _ = copy_bidirectional(&mut client_stream, &mut server_stream).await;
            }
        }
    }

    Ok(())
}

async fn peek_stream(stream: &TcpStream) -> Result<String, Error> {
    let mut buffer = vec![0; 8192];
    let bytes_peeked = stream.peek(&mut buffer).await?;
    Ok(String::from_utf8_lossy(&buffer[..bytes_peeked]).to_string())
}

// Configuração do proxy
struct ProxyConfig {
    port: u16,
    status: String,
    tls: bool,
    ssh_only: bool,
}

fn parse_args(args: &[String]) -> ProxyConfig {
    let mut port = 80u16;
    let mut status = "@AWProxy1".to_string();
    let mut tls = false;
    let mut ssh_only = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-p" => {
                if i + 1 < args.len() {
                    port = args[i + 1].parse().unwrap_or(80);
                    i += 1;
                }
            }
            "-s" => {
                if i + 1 < args.len() {
                    status = args[i + 1].clone();
                    i += 1;
                }
            }
            "-t" => {
                tls = true;
            }
            "-ssh" => {
                ssh_only = true;
            }
            _ => {}
        }
        i += 1;
    }

    ProxyConfig { port, status, tls, ssh_only }
}
