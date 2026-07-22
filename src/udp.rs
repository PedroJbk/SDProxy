use tokio::net::UdpSocket;
use anyhow::Result;
use log::info;
/// Handler para proxy UDP.
/// Recebe datagramas UDP na porta configurada e encaminha para o backend.
/// Suporta múltiplos clientes simultâneos.
pub async fn handle_udp_listener(port: u16, ssh_only: bool) -> Result<()> {
    info!("📡 Iniciando listener UDP na porta {}", port);

    let socket = UdpSocket::bind(format!("[::]:{}", port)).await?;
    info!("✅ UDP listener ativo na porta {}", port);

    let mut buf = [0u8; 65535];
    let backend_ssh = "127.0.0.1:22";
    let backend_vpn = "127.0.0.1:1194";

    loop {
        let (len, addr) = socket.recv_from(&mut buf).await?;
        let data = &buf[..len];

        // Determinar backend baseado no conteúdo
        let target = if ssh_only {
            backend_ssh
        } else {
            // Se começa com SSH, vai para SSH, senão VPN
            if data.starts_with(b"SSH") {
                backend_ssh
            } else {
                backend_vpn
            }
        };

        // Encaminhar para o backend e esperar resposta
        let response = forward_udp(data, target).await;

        if let Ok(resp) = response {
            let _ = socket.send_to(&resp, addr).await;
        }
    }
}

/// Encaminha um datagrama UDP para o backend e retorna a resposta
async fn forward_udp(data: &[u8], target: &str) -> Result<Vec<u8>> {
    let socket = UdpSocket::bind("127.0.0.1:0").await?;
    socket.connect(target).await?;
    socket.send(data).await?;

    let mut buf = [0u8; 65535];
    let timeout = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        socket.recv(&mut buf),
    ).await;

    match timeout {
        Ok(Ok(n)) => Ok(buf[..n].to_vec()),
        Ok(Err(e)) => Err(e.into()),
        Err(_) => {
            info!("⏱️ Timeout UDP para {}", target);
            Err(anyhow::anyhow!("Timeout"))
        }
    }
}

/// Proxy UDP bidirecional para uma conexão específica
pub async fn handle_udp_connection(
    data: &[u8],
    _client_addr: std::net::SocketAddr,
    ssh_only: bool,
) -> Result<Vec<u8>> {
    let target = if ssh_only {
        "127.0.0.1:22"
    } else if data.starts_with(b"SSH") {
        "127.0.0.1:22"
    } else {
        "127.0.0.1:1194"
    };

    forward_udp(data, target).await
}
