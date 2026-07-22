//! UDP Handler
//! Recebe pacotes UDP e encaminha para SSH via TCP ou UDPGW

use std::io::Error;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::Mutex;

pub async fn handle_udp_listener(
    port: u16,
    ssh_only: bool,
) -> Result<(), Error> {
    println!("[UDP] Listener rodando na porta: {}", port);

    let addr = format!("[::]:{}", port);
    let socket = UdpSocket::bind(&addr).await?;

    let mut buf = [0u8; 65536];

    loop {
        match socket.recv_from(&mut buf).await {
            Ok((len, src_addr)) => {
                if len > 0 {
                    println!("[UDP] Recebido {} bytes de {}", len, src_addr);

                    if ssh_only {
                        // Encaminhar para SSH via TCP
                        let _ = handle_udp_to_ssh(&buf[..len]).await;
                    } else {
                        // Encaminhar para VPN (UDPGW)
                        let _ = handle_udp_to_udpgw(&buf[..len]).await;
                    }
                }
            }
            Err(e) => {
                println!("[UDP] Erro: {}", e);
            }
        }
    }
}

async fn handle_udp_to_ssh(data: &[u8]) -> Result<(), Error> {
    let mut stream = TcpStream::connect("127.0.0.1:22").await?;
    stream.write_all(data).await?;
    Ok(())
}

async fn handle_udp_to_udpgw(data: &[u8]) -> Result<(), Error> {
    let addr = "127.0.0.1:1194";
    let socket = UdpSocket::bind("[::]:0").await?;
    socket.send_to(data, addr).await?;
    Ok(())
}
