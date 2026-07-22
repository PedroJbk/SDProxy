use tokio::io::copy_bidirectional;
use tokio::net::TcpStream;
use anyhow::Result;
use log::info;

pub async fn handle_tcp(mut socket: TcpStream) -> Result<()> {
    info!("📦 TCP fallback - encaminhando para SSH...");

    // Tentar SSH primeiro
    match TcpStream::connect("127.0.0.1:22").await {
        Ok(mut remote) => {
            info!("✅ TCP fallback -> SSH conectado");
            let _ = copy_bidirectional(&mut socket, &mut remote).await;
            info!("🔚 Conexão TCP fallback->SSH encerrada");
            Ok(())
        }
        Err(_) => {
            // Se SSH falhar, tentar VPN
            info!("⚠️ SSH falhou, tentando VPN...");
            match TcpStream::connect("127.0.0.1:1194").await {
                Ok(mut remote) => {
                    info!("✅ TCP fallback -> VPN conectado");
                    let _ = copy_bidirectional(&mut socket, &mut remote).await;
                    Ok(())
                }
                Err(e) => {
                    info!("❌ Falha TCP fallback: {}", e);
                    Err(e.into())
                }
            }
        }
    }
}
