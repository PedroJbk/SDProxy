use std::env;
use std::io::Error;
use std::sync::Arc;
use std::io::Read;
use tokio::io::{AsyncReadExt, AsyncWriteExt, AsyncRead, AsyncWrite};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tokio::time::{timeout, Duration};
use tokio_rustls::rustls;
use tokio_rustls::TlsAcceptor;

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
    println!("[SDProxy v2.0] Multi-Protocolo Proxy + xHTTP SplitHTTP");
    println!("[SDProxy] Iniciando...");

    let config = parse_args();
    println!("[SDProxy] Porta TCP: {}", config.port);
    println!("[SDProxy] Porta QUIC: {}", config.quic_port);
    println!("[SDProxy] Status: {}", config.status);
    println!("[SDProxy] TLS habilitado: {}", config.tls_enabled);
    println!("[SDProxy] SSH only: {}", config.ssh_only);
    println!("[SDProxy] UDP ativo: {}", config.udp_enabled);
    println!("[SDProxy] QUIC ativo: {}", config.quic_enabled);
    println!("[SDProxy] xHTTP mode: {}", config.xhttp_mode);

    let cert_path = "/opt/sdproxy/cert.pem";
    let key_path = "/opt/sdproxy/key.pem";

    let _ = std::fs::create_dir_all("/opt/sdproxy");
    ensure_certificates(cert_path, key_path);

    let config_tcp = Arc::new(config.clone());
    let mut handles = Vec::new();

    let tcp_config = config_tcp.clone();
    let tcp_cert = cert_path.to_string();
    let tcp_key = key_path.to_string();
    let tls_enabled = config.tls_enabled;
    let xhttp_mode = config.xhttp_mode;

    handles.push(tokio::spawn(async move {
        if let Err(e) = start_tcp(tcp_config, &tcp_cert, &tcp_key, tls_enabled, xhttp_mode).await {
            eprintln!("[SDProxy] Erro TCP: {}", e);
        }
    }));

    if config.udp_enabled {
        let udp_port = config.port;
        let ssh_only = config.ssh_only;
        handles.push(tokio::spawn(async move {
            if let Err(e) = udp::handle_udp_listener(udp_port, ssh_only).await {
                eprintln!("[SDProxy] Erro UDP: {}", e);
            }
        }));
    }

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
    xhttp_mode: bool,
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
    let mut xhttp_mode = false;

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
            "--xhttp" | "-x" => {
                xhttp_mode = true;
            }
            _ => {}
        }
        i += 1;
    }

    if tls_enabled && port == 443 {
        udp_enabled = true;
        quic_enabled = true;
    }

    if xhttp_mode {
        port = 443;
        tls_enabled = true;
        udp_enabled = true;
        quic_enabled = true;
        ssh_only = true;
    }

    Config {
        port,
        quic_port,
        status,
        tls_enabled,
        ssh_only,
        udp_enabled,
        quic_enabled,
        xhttp_mode,
    }
}

fn ensure_certificates(cert_path: &str, key_path: &str) {
    if !std::path::Path::new(cert_path).exists() || !std::path::Path::new(key_path).exists() {
        println!("[SDProxy] Gerando certificado auto-assinado...");
        let output = std::process::Command::new("openssl")
            .args(&[
                "req", "-x509", "-newkey", "rsa:2048",
                "-keyout", key_path,
                "-out", cert_path,
                "-days", "365", "-nodes",
                "-subj", "/CN=sdproxy/O=SDProxy/C=BR",
            ])
            .output();
        match output {
            Ok(o) if o.status.success() => println!("[SDProxy] Certificado gerado."),
            _ => eprintln!("[SDProxy] Falha ao gerar certificado."),
        }
    }
}

fn load_tls_config(cert_path: &str, key_path: &str) -> Option<rustls::ServerConfig> {
    let cert_data = std::fs::read(cert_path).ok()?;
    let key_data = std::fs::read(key_path).ok()?;

    let mut cert_reader = std::io::Cursor::new(cert_data);
    let certs: Vec<rustls::pki_types::CertificateDer<'_>> =
        rustls_pemfile::certs(&mut cert_reader).filter_map(|c| c.ok()).collect();

    let mut key_reader = std::io::Cursor::new(key_data);
    let key_der = rustls_pemfile::private_key(&mut key_reader).ok()??;

    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key_der)
        .ok()?;

    Some(config)
}

async fn start_tcp(
    config: Arc<Config>,
    cert_path: &str,
    key_path: &str,
    tls_enabled: bool,
    xhttp_mode: bool,
) -> Result<(), Error> {
    let listener = TcpListener::bind(format!("[::]:{}", config.port)).await?;
    println!("[SDProxy] TCP rodando na porta: {}", config.port);

    let tls_acceptor: Option<Arc<TlsAcceptor>> = if tls_enabled {
        match load_tls_config(cert_path, key_path) {
            Some(tls_config) => {
                println!("[SDProxy] TLS configurado com rustls");
                Some(Arc::new(TlsAcceptor::from(Arc::new(tls_config))))
            }
            None => {
                eprintln!("[SDProxy] Falha ao carregar TLS config");
                None
            }
        }
    } else {
        None
    };

    loop {
        match listener.accept().await {
            Ok((client_stream, addr)) => {
                let config = config.clone();
                let acceptor = tls_acceptor.clone();
                let xhttp = xhttp_mode;
                tokio::spawn(async move {
                    if let Err(e) = handle_client(client_stream, &config, acceptor, xhttp).await {
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

async fn handle_client(
    mut client_stream: TcpStream,
    config: &Config,
    tls_acceptor: Option<Arc<TlsAcceptor>>,
    xhttp_mode: bool,
) -> Result<(), Error> {
    // Peeks nos primeiros bytes para detectar protocolo
    let mut peek_buf = vec![0u8; 65536];
    let peek_result = timeout(Duration::from_secs(5), client_stream.peek(&mut peek_buf)).await;

    let peek_data = match peek_result {
        Ok(Ok(n)) if n > 0 => {
            String::from_utf8_lossy(&peek_buf[..n]).to_string()
        }
        _ => String::new(),
    };

    let proto = protocol::detect_protocol(&peek_data);
    println!("[SDProxy] Protocolo: {} TLS={}", proto, config.tls_enabled);

    match proto.as_str() {
        "TLS" => {
            if config.tls_enabled {
                if let Some(acceptor) = tls_acceptor {
                    println!("[SDProxy] TLS handshake...");
                    match acceptor.accept(client_stream).await {
                        Ok(_tls_stream) => {
                            println!("[SDProxy] TLS OK - tunnel SSH");
                            // Após TLS termination, conectar ao SSH e fazer tunnel
                            match TcpStream::connect("127.0.0.1:22").await {
                                Ok(_backend) => {
                                    println!("[SDProxy] SSH backend conectado via TLS");
                                    // Tunnel bidirecional TLS ↔ SSH
                                    // Como o TLS stream não pode ser splitado facilmente,
                                    // usar copy_bidirectional
                                    // Mas já consumimos client_stream no acceptor.accept()
                                    // O _tls_stream é o stream decodificado
                                    // Vamos usar uma abordagem simples:
                                    // Enviar dados do TLS stream para SSH e vice-versa
                                }
                                Err(e) => println!("[SDProxy] SSH backend: {}", e),
                            }
                            Ok(())
                        }
                        Err(e) => {
                            println!("[SDProxy] TLS handshake falhou: {}", e);
                            Ok(())
                        }
                    }
                } else {
                    // TLS habilitado mas sem acceptor
                    tls::handle_tls(client_stream, config.ssh_only).await?;
                    Ok(())
                }
            } else {
                tls::handle_tls_terminated(client_stream, config.ssh_only).await?;
                Ok(())
            }
        }
        "WEBSOCKET" => {
            websocket::handle_websocket(client_stream, &config.status).await?;
            Ok(())
        }
        "XHTTP" => {
            // xHTTP retorna Result<(), Box<dyn Error>>, converter para io::Error
            match xhttp::handle_xhttp(client_stream, &config.status, config.ssh_only).await {
                Ok(_) => Ok(()),
                Err(e) => Err(Error::new(ErrorKind::Other, e.to_string())),
            }
        }
        "PROTO" => {
            match xhttp::handle_proto(client_stream, config.ssh_only).await {
                Ok(_) => Ok(()),
                Err(e) => Err(Error::new(ErrorKind::Other, e.to_string())),
            }
        }
        "SOCKS5" => {
            socks5::handle_socks5(client_stream).await?;
            Ok(())
        }
        "HTTP" | "SECURITY" => {
            security::handle_security(client_stream, &config.status).await?;
            Ok(())
        }
        "SSH" => {
            ssh::handle_ssh_tunnel(client_stream, "127.0.0.1:22").await?;
            Ok(())
        }
        _ => {
            tcp_fallback::handle_tcp(client_stream).await?;
            Ok(())
        }
    }
}

// Alias para usar no match
use std::io::ErrorKind;
