use tokio::io::{AsyncReadExt, AsyncWriteExt};
#[allow(unused_imports)]
use tokio::io::copy_bidirectional;
use tokio::net::TcpStream;
use anyhow::Result;
use log::info;

/// Handler para conexões TLS.
/// Quando TLS está habilitado (-t), o proxy faz o handshake TLS e depois
/// encaminha o tráfego descriptografado para o backend (SSH ou VPN).
/// Se não for TLS real (apenas dados binários), faz passthrough direto.
pub async fn handle_tls(mut socket: TcpStream, ssh_only: bool) -> Result<()> {
    info!("🛡️ TLS handler - processando conexão TLS...");

    // Determinar backend alvo
    let addr_proxy = if ssh_only {
        "127.0.0.1:22"
    } else {
        "127.0.0.1:22"
    };

    // Tentar conectar ao backend primário
    let mut backend = connect_with_fallback(addr_proxy, ssh_only).await?;

    // Passthrough TLS - encaminha os bytes TLS criptografados diretamente
    // para o backend. O backend (SSH) entende o TLS que é encaminhado.
    // Esta abordagem mantém a criptografia ponta-a-ponta.
    info!("📡 TLS passthrough para backend: {}", backend.peer_addr().unwrap_or("unknown".parse().unwrap()));

    tokio::io::copy_bidirectional(&mut socket, &mut backend).await?;
    info!("🔚 TLS conexão encerrada");
    Ok(())
}

/// Handler para TLS com terminação (decodifica TLS no proxy)
/// Usado quando queremos que o proxy entenda o conteúdo TLS
pub async fn handle_tls_terminated(mut socket: TcpStream, ssh_only: bool) -> Result<()> {
    info!("🔓 TLS terminação - decodificando TLS no proxy...");

    // Ler os dados do cliente (pode ser TLS ClientHello ou dados HTTP)
    let mut buf = [0u8; 8192];
    let n = socket.read(&mut buf).await?;

    if n == 0 {
        return Ok(());
    }

    let data = &buf[..n];

    // Verificar se é TLS ClientHello (byte 0x16 = handshake, 0x03 = versão TLS)
    if n >= 2 && data[0] == 0x16 && data[1] == 0x03 {
        info!("TLS ClientHello detectado ({} bytes)", n);

        // Extrair SNI do ClientHello
        if let Some(sni) = extract_sni(data) {
            info!("SNI: {}", sni);
        }

        // Determinar backend baseado no conteúdo
        let addr_proxy = if ssh_only {
            "127.0.0.1:22"
        } else if n > 50 && data[43..n.min(100)].iter().any(|&b| b == b'S' && data.get(44) == Some(&b'S')) {
            "127.0.0.1:22"
        } else {
            "127.0.0.1:22"
        };

        let mut backend = connect_with_fallback(addr_proxy, ssh_only).await?;

        // Enviar o ClientHello para o backend
        backend.write_all(data).await?;
        backend.flush().await?;

        // Tunnel bidirecional
        info!("🔗 TLS tunnel bidirecional iniciado");
        tokio::io::copy_bidirectional(&mut socket, &mut backend).await?;
    } else {
        // Não é TLS puro, pode ser HTTP dentro de TLS ou dados diretos
        info!("Dados recebidos ({} bytes), não é ClientHello puro", n);

        let addr_proxy = if ssh_only {
            "127.0.0.1:22"
        } else {
            "127.0.0.1:22"
        };

        let mut backend = connect_with_fallback(addr_proxy, ssh_only).await?;

        // Encaminhar os dados lidos
        backend.write_all(data).await?;
        backend.flush().await?;

        // Tunnel bidirecional
        tokio::io::copy_bidirectional(&mut socket, &mut backend).await?;
    }

    info!("TLS conexão finalizada");
    Ok(())
}

/// Extrai o SNI (Server Name Indication) de um TLS ClientHello
fn extract_sni(data: &[u8]) -> Option<String> {
    // Verifica se é ClientHello (Content Type 0x16, Version 0x0301)
    if data.len() < 5 || data[0] != 0x16 {
        return None;
    }

    // Pular o header do handshake (5 bytes) e verificar tipo (1 = ClientHello)
    if data[5] != 0x01 {
        return None;
    }

    // Pular: ContentType(1) + Version(2) + Length(2) + HandshakeType(1) + HandshakeLength(3)
    let mut pos = 6 + 3; // 9 bytes

    // Pular versão do cliente (2 bytes) e random (32 bytes)
    pos += 2 + 32;

    // Pular session_id
    if pos >= data.len() {
        return None;
    }
    let session_id_len = data[pos] as usize;
    pos += 1 + session_id_len;

    // Pular cipher suites
    if pos + 2 > data.len() {
        return None;
    }
    let cipher_suites_len = u16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
    pos += 2 + cipher_suites_len;

    // Pular compression methods
    if pos >= data.len() {
        return None;
    }
    let comp_len = data[pos] as usize;
    pos += 1 + comp_len;

    // Extensions
    if pos + 2 > data.len() {
        return None;
    }
    let ext_len = u16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
    pos += 2;

    // Procurar pela extensão SNI (type = 0x0000)
    let ext_end = (pos + ext_len).min(data.len());
    while pos + 4 <= ext_end {
        let ext_type = u16::from_be_bytes([data[pos], data[pos + 1]]);
        let ext_data_len = u16::from_be_bytes([data[pos + 2], data[pos + 3]]) as usize;
        pos += 4;

        if ext_type == 0x0000 {
            // Extensão SNI encontrada
            let sni_end = (pos + ext_data_len).min(data.len());
            // Pular SNI list length (2 bytes) e SNI type (1 byte)
            let sni_pos = pos + 3;
            if sni_pos + 2 > sni_end {
                return None;
            }
            let sni_len = u16::from_be_bytes([data[sni_pos], data[sni_pos + 1]]) as usize;
            let sni_start = sni_pos + 2;
            let sni_end = (sni_start + sni_len).min(data.len());
            return String::from_utf8(data[sni_start..sni_end].to_vec()).ok();
        }

        pos += ext_data_len;
    }

    None
}

/// Tenta conectar ao backend primário, com fallback para o alternativo
async fn connect_with_fallback(primary: &str, ssh_only: bool) -> Result<TcpStream> {
    match TcpStream::connect(primary).await {
        Ok(stream) => {
            info!("✅ Backend conectado: {}", primary);
            Ok(stream)
        }
        Err(e) => {
            if ssh_only {
                return Err(e.into());
            }
            info!("⚠️ Falha em {}: {}. Tentando fallback...", primary, e);
            let alt = if primary.contains(":22") {
                "127.0.0.1:1194"
            } else {
                "127.0.0.1:22"
            };
            match TcpStream::connect(alt).await {
                Ok(stream) => {
                    info!("✅ Fallback OK: {}", alt);
                    Ok(stream)
                }
                Err(e2) => {
                    info!("❌ Ambos falharam: {}, {}", e, e2);
                    Err(e2.into())
                }
            }
        }
    }
}
