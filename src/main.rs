use std::env;
use std::io::Error;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tokio::time::{timeout, Duration};

mod websocket;
mod security;
mod tcp_fallback;
mod tls;
mod ssh;
mod xhttp;

#[tokio::main]
async fn main() -> Result<(), Error> {
    let args: Vec<String> = env::args().collect();
    let mut port: u16 = 80;
    let mut status = String::from("@SDProxy");

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--port" | "-p" => {
                if i + 1 < args.len() {
                    port = args[i + 1].parse().unwrap_or(80);
                    i += 1;
                }
            }
            "--status" | "-s" => {
                if i + 1 < args.len() {
                    status = args[i + 1].clone();
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }

    println!("[SDProxy v2.0] Porta: {}", port);
    println!("[SDProxy] Status: {}", status);

    let listener = TcpListener::bind(format!("[::]:{}", port)).await?;
    println!("[SDProxy] Servico rodando na porta: {}", port);

    let status_arc = Arc::new(status);

    loop {
        match listener.accept().await {
            Ok((client_stream, addr)) => {
                let status = status_arc.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_client(client_stream, &status).await {
                        println!("[SDProxy] Erro cliente {}: {}", addr, e);
                    }
                });
            }
            Err(e) => {
                println!("[SDProxy] Erro aceitar conexao: {}", e);
            }
        }
    }
}

async fn handle_client(mut client_stream: TcpStream, status: &str) -> Result<(), Error> {
    // PADRAO BSProxy: SEMPRE envia 101 primeiro
    client_stream
        .write_all(format!("HTTP/1.1 101 {}\r\n\r\n", status).as_bytes())
        .await?;

    // SEMPRE le do cliente
    let mut buffer = vec![0; 1024];
    let n = client_stream.read(&mut buffer).await?;

    // SEMPRE envia 200
    client_stream
        .write_all(format!("HTTP/1.1 200 {}\r\n\r\n", status).as_bytes())
        .await?;

    // Detecta SSH vs VPN pelo payload lido
    let payload = String::from_utf8_lossy(&buffer[..n]);
    let mut addr_proxy = "0.0.0.0:22";

    // Detectar se é SSH ou VPN
    if payload.contains("SSH") || payload.contains("ssh") || payload.is_empty() || n == 0 {
        addr_proxy = "0.0.0.0:22";
    } else if payload.contains("SSH-") || payload.contains("SSH-2.0") {
        addr_proxy = "0.0.0.0:22";
    } else {
        // Pode ser TLS, VPN, etc - tentar SSH primeiro, depois VPN
        addr_proxy = "0.0.0.0:22";
    }

    // Conectar ao backend
    match TcpStream::connect(addr_proxy).await {
        Ok(server_stream) => {
            let (cr, cw) = client_stream.into_split();
            let (sr, sw) = server_stream.into_split();

            let cr = Arc::new(Mutex::new(cr));
            let cw = Arc::new(Mutex::new(cw));
            let sr = Arc::new(Mutex::new(sr));
            let sw = Arc::new(Mutex::new(sw));

            let c2s = transfer_data(cr, sw);
            let s2c = transfer_data(sr, cw);

            tokio::try_join!(c2s, s2c)?;
            Ok(())
        }
        Err(_) => {
            // Se SSH falhou, tentar VPN
            match TcpStream::connect("0.0.0.0:1194").await {
                Ok(server_stream) => {
                    let (cr, cw) = client_stream.into_split();
                    let (sr, sw) = server_stream.into_split();

                    let cr = Arc::new(Mutex::new(cr));
                    let cw = Arc::new(Mutex::new(cw));
                    let sr = Arc::new(Mutex::new(sr));
                    let sw = Arc::new(Mutex::new(sw));

                    let c2s = transfer_data(cr, sw);
                    let s2c = transfer_data(sr, cw);

                    tokio::try_join!(c2s, s2c)?;
                    Ok(())
                }
                Err(e) => {
                    println!("[SDProxy] Ambos backend falharam: {}", e);
                    Ok(())
                }
            }
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
