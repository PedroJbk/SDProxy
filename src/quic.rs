use std::sync::Arc;
use std::path::Path;
use anyhow::Result;
use log::info;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};

/// Configura o servidor QUIC com certificado auto-assinado ou arquivos existentes.
pub fn configure_server(cert_path: &str, key_path: &str) -> Result<quinn::ServerConfig> {
    info!("🔐 Configurando servidor QUIC...");

    let certs = load_certs(cert_path)?;
    let key = load_key(key_path)?;

    let mut server_config = quinn::ServerConfig::with_single_cert(certs, key)?;

    // Configurar transporte QUIC
    let transport_config = Arc::new(quinn::TransportConfig::default());
    server_config.transport = transport_config;

    info!("✅ Servidor QUIC configurado");
    Ok(server_config)
}

/// Gera certificado auto-assinado e salva nos caminhos especificados.
pub fn generate_self_signed_cert(cert_path: &str, key_path: &str) -> Result<()> {
    info!("📜 Gerando certificado auto-assinado...");

    let cert = rcgen::generate_simple_self_signed(vec![
        "localhost".to_string(),
    ])?;

    // Salvar certificado em formato PEM
    std::fs::write(cert_path, cert.cert.pem())?;
    // Salvar chave privada em formato PEM
    std::fs::write(key_path, cert.key_pair.serialize_pem())?;

    info!("✅ Certificado salvo em: {}", cert_path);
    info!("✅ Chave salva em: {}", key_path);
    Ok(())
}

/// Carrega certificados de arquivo PEM
fn load_certs(path: &str) -> Result<Vec<CertificateDer<'static>>> {
    let cert_pem = std::fs::read_to_string(path)?;
    let certs: Vec<CertificateDer<'static>> = rustls_pemfile::certs(&mut cert_pem.as_bytes())
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(certs)
}

/// Carrega chave privada de arquivo PEM
fn load_key(path: &str) -> Result<PrivateKeyDer<'static>> {
    let key_pem = std::fs::read_to_string(path)?;
    let mut keys: Vec<_> = rustls_pemfile::pkcs8_private_keys(&mut key_pem.as_bytes())
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if let Some(key) = keys.pop() {
        Ok(PrivateKeyDer::Pkcs8(key))
    } else {
        // Tentar RSA
        let key_pem2 = std::fs::read_to_string(path)?;
        let mut rsa_keys: Vec<_> = rustls_pemfile::rsa_private_keys(&mut key_pem2.as_bytes())
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if let Some(key) = rsa_keys.pop() {
            Ok(PrivateKeyDer::Pkcs1(key))
        } else {
            Err(anyhow::anyhow!("Nenhuma chave privada válida encontrada em {}", path))
        }
    }
}

/// Handler principal do servidor QUIC.
pub async fn start_quic_server(
    port: u16,
    cert_path: &str,
    key_path: &str,
    ssh_only: bool,
) -> Result<()> {
    info!("🚀 Iniciando servidor QUIC na porta {}", port);

    // Garantir que o certificado existe
    if !Path::new(cert_path).exists() || !Path::new(key_path).exists() {
        generate_self_signed_cert(cert_path, key_path)?;
    }

    let server_config = configure_server(cert_path, key_path)?;

    let endpoint = quinn::Endpoint::server(server_config, format!("[::]:{}", port).parse()?)?;
    info!("✅ QUIC endpoint ativo na porta {}", port);

    loop {
        match endpoint.accept().await {
            Some(incoming) => {
                let ssh_only = ssh_only;
                tokio::spawn(async move {
                    if let Err(e) = handle_quic_tunnel(incoming, ssh_only).await {
                        info!("Erro na conexão QUIC: {}", e);
                    }
                });
            }
            None => {
                info!("QUIC endpoint fechado");
                break;
            }
        }
    }

    Ok(())
}

/// Proxy QUIC com tunnel bidirecional completo (stream mode)
pub async fn handle_quic_tunnel(incoming: quinn::Incoming, ssh_only: bool) -> Result<()> {
    let connection = incoming.await?;
    info!("🔗 QUIC tunnel de: {}", connection.remote_address());

    let addr_proxy = if ssh_only {
        "127.0.0.1:22"
    } else {
        "127.0.0.1:22" // Default para SSH
    };

    // Tentar conectar ao backend
    let backend = match tokio::net::TcpStream::connect(addr_proxy).await {
        Ok(s) => s,
        Err(e) => {
            // Fallback para VPN
            if !ssh_only {
                info!("QUIC SSH falhou, tentando VPN...");
                tokio::net::TcpStream::connect("127.0.0.1:1194").await?
            } else {
                return Err(e.into());
            }
        }
    };

    // Aceitar stream bidirecional QUIC
    let (mut send, mut recv) = connection.accept_bi().await?;

    let (backend_read, backend_write) = backend.into_split();
    let backend_read = Arc::new(tokio::sync::Mutex::new(backend_read));

    // QUIC -> Backend
    let backend_write_arc = Arc::new(tokio::sync::Mutex::new(backend_write));
    let backend_write_clone = backend_write_arc.clone();
    let quic_to_backend = tokio::spawn(async move {
        let mut buf = [0u8; 8192];
        loop {
            match recv.read(&mut buf).await {
                Ok(Some(n)) => {
                    let mut bw = backend_write_clone.lock().await;
                    if bw.write_all(&buf[..n]).await.is_err() {
                        break;
                    }
                }
                _ => break,
            }
        }
    });

    // Backend -> QUIC
    let backend_to_quic = tokio::spawn(async move {
        let mut buf = [0u8; 8192];
        let mut reader = backend_read.lock().await;
        loop {
            match reader.read(&mut buf).await {
                Ok(n) if n > 0 => {
                    if send.write_all(&buf[..n]).await.is_err() {
                        break;
                    }
                }
                _ => break,
            }
        }
        let _ = send.finish();
    });

    tokio::try_join!(quic_to_backend, backend_to_quic)?;
    info!("QUIC tunnel finalizado");
    Ok(())
}
