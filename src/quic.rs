//! QUIC Handler
//! Servidor QUIC para VPN over QUIC (usando quinn + rustls)

use std::io::Error;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

pub async fn start_quic_server(
    port: u16,
    cert_path: &str,
    key_path: &str,
    ssh_only: bool,
) -> Result<(), Error> {
    println!("[QUIC] Servidor QUIC rodando na porta: {}", port);

    use quinn::Endpoint;
    use std::net::SocketAddr;

    let addr: SocketAddr = format!("[::]:{}", port).parse()
        .map_err(|e: std::net::AddrParseError| Error::new(std::io::ErrorKind::Other, e))?;

    // Carregar certificados
    let cert_data = std::fs::read(cert_path)?;
    let key_data = std::fs::read(key_path)?;

    // Parse certificados
    let mut cert_reader = std::io::Cursor::new(cert_data);
    let certs: Vec<rustls::pki_types::CertificateDer<'_>> =
        rustls_pemfile::certs(&mut cert_reader).filter_map(|c| c.ok()).collect();

    let mut key_reader = std::io::Cursor::new(key_data);
    let key_der = rustls_pemfile::private_key(&mut key_reader)
        .map_err(|e| Error::new(std::io::ErrorKind::Other, e))?
        .ok_or_else(|| Error::new(std::io::ErrorKind::Other, "No private key found"))?;

    // Configurar quinn server
    let mut server_config = quinn::ServerConfig::with_single_cert(certs, key_der)
        .map_err(|e| Error::new(std::io::ErrorKind::Other, format!("QUIC cert error: {}", e)))?;

    // Configurar transport
    let mut transport_config = quinn::TransportConfig::default();
    transport_config.max_concurrent_bidi_streams(256_u32.into());
    transport_config.max_concurrent_uni_streams(256_u32.into());
    server_config.transport_config(Arc::new(transport_config));

    let endpoint = Endpoint::server(server_config, addr)
        .map_err(|e| Error::new(std::io::ErrorKind::Other, format!("QUIC bind error: {}", e)))?;

    println!("[QUIC] Endpoint criado, aguardando conexões...");

    loop {
        match endpoint.accept().await {
            Some(conn) => {
                let ssh_only = ssh_only;
                tokio::spawn(async move {
                    match conn.await {
                        Ok(connection) => {
                            println!("[QUIC] Nova conexão estabelecida");
                            if let Err(e) = handle_quic_connection(connection, ssh_only).await {
                                println!("[QUIC] Erro na conexão: {}", e);
                            }
                        }
                        Err(e) => println!("[QUIC] Erro ao aceitar: {}", e),
                    }
                });
            }
            None => {
                println!("[QUIC] Endpoint fechado");
                break;
            }
        }
    }

    Ok(())
}

async fn handle_quic_connection(
    connection: quinn::Connection,
    ssh_only: bool,
) -> Result<(), Error> {
    loop {
        match connection.accept_bi().await {
            Ok((mut send, mut recv)) => {
                tokio::spawn(async move {
                    let addr = if ssh_only { "127.0.0.1:22" } else { "127.0.0.1:22" };
                    match TcpStream::connect(addr).await {
                        Ok(backend) => {
                            let (cr, cw) = backend.into_split();
                            let cr = Arc::new(tokio::sync::Mutex::new(cr));
                            let cw = Arc::new(tokio::sync::Mutex::new(cw));
                            let send = Arc::new(tokio::sync::Mutex::new(send));
                            let recv = Arc::new(tokio::sync::Mutex::new(recv));
                            let _ = tokio::try_join!(
                                quic_to_tcp(recv, cw),
                                tcp_to_quic(cr, send),
                            );
                        }
                        Err(e) => println!("[QUIC] Erro backend: {}", e),
                    }
                });
            }
            Err(quinn::ConnectionError::LocallyClosed) => {
                println!("[QUIC] Conexão fechada localmente");
                break;
            }
            Err(e) => {
                println!("[QUIC] Erro ao aceitar stream: {}", e);
                break;
            }
        }
    }

    Ok(())
}

async fn quic_to_tcp(
    quic_recv: Arc<tokio::sync::Mutex<quinn::RecvStream>>,
    tcp_write: Arc<tokio::sync::Mutex<tokio::net::tcp::OwnedWriteHalf>>,
) -> Result<(), Error> {
    let mut buffer = [0u8; 8192];
    loop {
        let bytes_read = {
            let mut recv = quic_recv.lock().await;
            match recv.read(&mut buffer).await {
                Ok(Some(n)) => n,
                Ok(None) => break,
                Err(e) => return Err(Error::new(std::io::ErrorKind::Other, e)),
            }
        };
        let mut write = tcp_write.lock().await;
        write.write_all(&buffer[..bytes_read]).await?;
    }
    Ok(())
}

async fn tcp_to_quic(
    tcp_read: Arc<tokio::sync::Mutex<tokio::net::tcp::OwnedReadHalf>>,
    quic_send: Arc<tokio::sync::Mutex<quinn::SendStream>>,
) -> Result<(), Error> {
    let mut buffer = [0u8; 8192];
    loop {
        let bytes_read = {
            let mut read = tcp_read.lock().await;
            match read.read(&mut buffer).await {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) => return Err(e),
            }
        };
        let mut send = quic_send.lock().await;
        match send.write_all(&buffer[..bytes_read]).await {
            Ok(_) => {},
            Err(e) => return Err(Error::new(std::io::ErrorKind::Other, e)),
        }
    }
    Ok(())
}
