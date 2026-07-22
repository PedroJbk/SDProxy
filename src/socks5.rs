use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use anyhow::Result;
use log::info;

pub async fn handle_socks5(mut client: TcpStream) -> Result<()> {
    info!("🔐 SOCKS5");

    let mut header = [0u8; 2];
    client.read_exact(&mut header).await?;
    let nmethods = header[1] as usize;

    let mut methods = vec![0u8; nmethods];
    client.read_exact(&mut methods).await?;

    client.write_all(&[0x05, 0x00]).await?;

    let mut req = [0u8; 4];
    client.read_exact(&mut req).await?;
    let cmd = req[1];
    let atyp = req[3];

    let target_addr = match atyp {
        0x01 => {
            let mut addr = [0u8; 4];
            client.read_exact(&mut addr).await?;
            let port = read_port(&mut client).await?;
            format!("{}.{}.{}.{}:{}", addr[0], addr[1], addr[2], addr[3], port)
        }
        0x03 => {
            let mut len_buf = [0u8; 1];
            client.read_exact(&mut len_buf).await?;
            let len = len_buf[0] as usize;

            let mut domain = vec![0u8; len];
            client.read_exact(&mut domain).await?;
            let port = read_port(&mut client).await?;
            format!("{}:{}", String::from_utf8_lossy(&domain), port)
        }
        _ => {
            send_reply(&mut client, 0x08).await?;
            anyhow::bail!("Unsupported address type");
        }
    };

    if cmd != 0x01 {
        send_reply(&mut client, 0x07).await?;
        anyhow::bail!("Unsupported SOCKS command");
    }

    info!("SOCKS5 -> {}", target_addr);

    match TcpStream::connect(&target_addr).await {
        Ok(remote) => {
            send_reply(&mut client, 0x00).await?;
            let (mut client_reader, mut client_writer) = client.into_split();
            let (mut remote_reader, mut remote_writer) = remote.into_split();

            tokio::try_join!(
                tokio::io::copy(&mut client_reader, &mut remote_writer),
                tokio::io::copy(&mut remote_reader, &mut client_writer)
            )?;
            Ok(())
        }
        Err(e) => {
            send_reply(&mut client, 0x05).await?;
            anyhow::bail!("Connection failed: {}", e);
        }
    }
}

async fn read_port(client: &mut TcpStream) -> std::io::Result<u16> {
    let mut port_buf = [0u8; 2];
    client.read_exact(&mut port_buf).await?;
    Ok(u16::from_be_bytes(port_buf))
}

async fn send_reply(client: &mut TcpStream, code: u8) -> std::io::Result<()> {
    client
        .write_all(&[0x05, code, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
        .await
}
