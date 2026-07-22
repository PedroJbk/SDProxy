use tokio::net::TcpStream;
use tokio_rustls::TlsAcceptor;
use rustls::{ServerConfig, Certificate, PrivateKey};
use std::sync::Arc;
use anyhow::Result;
use log::info;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub async fn handle_tls(socket: TcpStream) -> Result<()> {
    info!("🔒 TLS handshake...");
    
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()])?;
    let cert_der = cert.serialize_der()?;
    let key_der = cert.serialize_private_key_der();
    
    let config = ServerConfig::builder()
        .with_safe_defaults()
        .with_no_client_auth()
        .with_single_cert(vec![Certificate(cert_der)], PrivateKey(key_der))?;
    
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let mut tls_stream = acceptor.accept(socket).await?;
    
    info!("🔒 TLS handshake complete!");
    
    // Após handshake TLS, encaminhar para SSH
    let mut buf = [0u8; 1024];
    match tls_stream.read(&mut buf).await? {
        0 => return Ok(()),
        _ => {
            info!("TLS -> Encaminhando para SSH...");
            match TcpStream::connect("127.0.0.1:22").await {
                Ok(remote) => {
                    let (mut tls_reader, mut tls_writer) = tls_stream.into_split();
                    let (mut remote_reader, mut remote_writer) = remote.into_split();
                    
                    tokio::try_join!(
                        tokio::io::copy(&mut tls_reader, &mut remote_writer),
                        tokio::io::copy(&mut remote_reader, &mut tls_writer)
                    )?;
                    info!("✅ Conexão TLS->SSH encerrada");
                    Ok(())
                }
                Err(e) => {
                    info!("❌ Falha ao conectar ao SSH via TLS: {}", e);
                    Err(e.into())
                }
            }
        }
    }
}
