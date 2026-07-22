use std::env;
use std::io::Error;
use std::sync::Arc;
use std::io::{Read, Cursor};
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
    env_logger::init();
    println!("[SDProxy v0.3.1] Multi-Protocolo Proxy + xHTTP SplitHTTP");
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
    let mut cert_file = std::fs::File::open(cert_path).ok()?;
    let mut key_file = std::fs::File::open(key_path).ok()?;

    let mut cert_buf = Vec::new();
    cert_file.read_to_end(&mut cert_buf).ok()?;
    let mut key_buf = Vec::new();
    key_file.read_to_end(&mut key_buf).ok()?;

    let cert_reader = Cursor::new(&cert_buf);
    let certs: Vec<rustls::pki_types::CertificateDer<'_>> =
        rustls_pemfile::certs(cert_reader).filter_map(|c| c.ok()).collect();

    let key_der = rustls_pemfile::private_key(&mut Cursor::new(&key_buf)).ok()?.ok()?;

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
    let mut peek_buf = vec![0u8; 65536];
    let peek_result = timeout(Duration::from_secs(5), client_stream.peek(&mut peek_buf)).await;

    let peek_data = match peek_result {
        Ok(Ok(n)) if n > 0 => {
            String::from_utf8_lossy(&peek_buf[..n]).to_string()
        }
        _ => String::new(),
    };

    let proto = protocol::detect_protocol(&peek_data);
    println!("[SDProxy] TCP - Protocolo: {} ({} bytes) TLS={}", proto, peek_data.len(), config.tls_enabled);

    match proto.as_str() {
        "TLS" => {
            if config.tls_enabled {
                if let Some(acceptor) = tls_acceptor {
                    println!("[SDProxy] TLS: handshake com terminação local...");
                    match acceptor.accept(client_stream).await {
                        Ok(tls_stream) => {
                            println!("[SDProxy] TLS handshake OK - xHTTP mode={}", xhttp_mode);
                            handle_tls_decoded(tls_stream, &config.status, config.ssh_only, xhttp_mode).await?;
                        }
                        Err(e) => {
                            eprintln!("[SDProxy] TLS handshake falhou: {} - fallback passthrough", e);
                            let addr = if config.ssh_only { "127.0.0.1:22" } else { "127.0.0.1:22" };
                            if let Ok(mut backend) = TcpStream::connect(addr).await {
                                let _ = tokio::io::copy_bidirectional(&mut client_stream, &mut backend).await;
                            }
                        }
                    }
                } else {
                    if let Err(e) = tls::handle_tls(client_stream, config.ssh_only).await {
                        println!("[SDProxy] Erro TLS passthrough: {}", e);
                    }
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
                    let _ = client_stream.write_all(
                        format!("HTTP/1.1 101 {}\r\n\r\n", config.status).as_bytes()
                    ).await;
                    let _ = client_stream.read(&mut [0u8; 1024]).await;
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
            if let Err(e) = tcp_fallback::handle_tcp(client_stream).await {
                println!("[SDProxy] Erro TCP fallback: {}", e);
            }
        }
    }

    Ok(())
}

/// Após TLS termination, roteia os dados decodificados para xHTTP ou SSH
/// O stream TLS decodificado contém HTTP/2 puro
async fn handle_tls_decoded<S>(
    mut tls_stream: S,
    status: &str,
    ssh_only: bool,
    xhttp_mode: bool,
) -> Result<(), Error>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    // O cliente xHTTP envia: TLS → HTTP/2 GET /path/session-id
    // Após TLS termination, temos HTTP/2 puro
    // Precisamos ler e rotear para xHTTP
    
    if xhttp_mode {
        // Em modo xHTTP: tentar ler HTTP request
        // O stream agora é HTTP/2 puro (decodificado do TLS)
        
        // Conectar ao SSH backend para fazer o tunnel
        let addr = "127.0.0.1:22";
        
        match TcpStream::connect(addr).await {
            Ok(mut backend) => {
                println!("[SDProxy] TLS→SSH: tunnel bidirecional estabelecido");
                // O stream TLS decodificado vai direto para o SSH backend
                // O SSH backend entende o TLS encapsulado porque o cliente
                // SocksRevive faz: SSH dentro de TLS dentro de HTTP/2
                
                // Mas na verdade, o SSH backend espera SSH puro.
                // O que o SocksRevive envia é: TLS(handshake) + HTTP/2(GET/POST) + SSH payload
                // Após TLS termination, temos: HTTP/2 frames com SSH payload dentro
                
                // Solução: fazer proxy direto entre TLS stream e SSH backend
                let (mut cr, cw) = tls_stream.into_split_pair();
                // Precisamos manter ambos os lados
                
                // Usar split approach com Arc
                let r = Arc::new(Mutex::new(cr));
                let w = Arc::new(Mutex::new(cw));
                let (sr, sw) = backend.into_split();
                let sr = Arc::new(Mutex::new(sr));
                let sw = Arc::new(Mutex::new(sw));
                
                tokio::try_join!(
                    transfer_data_tls(r, sw),
                    transfer_data_tls(sr, w),
                )?;
                Ok(())
            }
            Err(e) => Err(Error::new(
                std::io::ErrorKind::ConnectionRefused,
                format!("Backend SSH não disponível: {}", e),
            ))
        }
    } else {
        // TLS normal: tunnel direto para SSH
        let addr = if ssh_only { "127.0.0.1:22" } else { "127.0.0.1:22" };
        match TcpStream::connect(addr).await {
            Ok(mut backend) => {
                let (cr, cw) = tls_stream.into_split_pair();
                let r = Arc::new(Mutex::new(cr));
                let w = Arc::new(Mutex::new(cw));
                let (sr, sw) = backend.into_split();
                let sr = Arc::new(Mutex::new(sr));
                let sw = Arc::new(Mutex::new(sw));
                tokio::try_join!(
                    transfer_data_tls(r, sw),
                    transfer_data_tls(sr, w),
                )?;
                Ok(())
            }
            Err(e) => Err(Error::new(
                std::io::ErrorKind::ConnectionRefused,
                format!("Backend não disponível: {}", e),
            ))
        }
    }
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

/// Versão genérica para TLS stream (AsyncRead + AsyncWrite)
async fn transfer_data_tls<R, W>(
    read_stream: Arc<Mutex<R>>,
    write_stream: Arc<Mutex<W>>,
) -> Result<(), Error>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
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
