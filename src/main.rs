use std::env;
use std::io::Error;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tokio::time::{timeout, Duration};

mod xhttp;
mod tls;

#[tokio::main]
async fn main() -> Result<(), Error> {
    let args: Vec<String> = env::args().collect();
    let mut port: u16 = 80;
    let mut status = String::from("@SDProxy");
    let mut tls_mode = false;
    let mut ssh_only = true;

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
            "--tls" | "-t" => {
                tls_mode = true;
            }
            "-ssh" | "--ssh" => {
                ssh_only = true;
            }
            _ => {}
        }
        i += 1;
    }

    println!("[SDProxy v2.1] Porta: {} | TLS: {} | Status: {}", port, tls_mode, status);
    println!("[SDProxy] SSH Only: {}", ssh_only);

    let listener = TcpListener::bind(format!("[::]:{}", port)).await?;
    println!("[SDProxy] Servico rodando na porta: {}", port);

    let status_arc = Arc::new(status);
    let ssh_only_arc = Arc::new(ssh_only);

    loop {
        match listener.accept().await {
            Ok((client_stream, addr)) => {
                let status = status_arc.clone();
                let ssh_only = ssh_only_arc.clone();
                let is_tls = tls_mode;
                let current_port = port;
                tokio::spawn(async move {
                    if let Err(e) = handle_client(client_stream, &status, ssh_only, is_tls, current_port).await {
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

async fn handle_client(
    mut client_stream: TcpStream,
    status: &str,
    ssh_only: Arc<bool>,
    tls_mode: bool,
    port: u16,
) -> anyhow::Result<()> {
    // Ler primeiros bytes para detectar protocolo
    let mut buf = [0u8; 1];
    let n = match timeout(Duration::from_secs(5), client_stream.read(&mut buf)).await {
        Ok(Ok(n)) => n,
        _ => return Ok(()),
    };

    if n == 0 {
        return Ok(());
    }

    let first_byte = buf[0];

    // Detectar TLS (0x16 = Handshake)
    let is_tls_conn = first_byte == 0x16;

    // Na porta 443 com TLS: rotear para xHTTP handler
    if port == 443 && is_tls_conn {
        println!("[SDProxy] Porta 443 + TLS detectado -> xHTTP handler");
        return tls::handle_tls_with_xhttp(client_stream, status, &*ssh_only).await;
    }

    // Na porta 443 SEM TLS: tratar como BSProxy normal
    if port == 443 && !is_tls_conn {
        println!("[SDProxy] Porta 443 sem TLS -> BSProxy normal");
    }

    // Portas 80/8080: padrão BSProxy
    // Precisa re-colocar o primeiro byte que lemos
    // Padrão BSProxy: 101 -> read -> 200 -> tunnel
    println!("[SDProxy] Porta {} -> BSProxy padrao", port);

    // Enviar 101
    client_stream
        .write_all(format!("HTTP/1.1 101 {}\r\n\r\n", status).as_bytes())
        .await?;

    // Re-construir o buffer com o primeiro byte + restante
    let mut buffer = Vec::with_capacity(8192);
    buffer.push(first_byte);

    // Ler o restante
    let mut rest = vec![0u8; 8192];
    let rest_n = match timeout(Duration::from_secs(10), client_stream.read(&mut rest)).await {
        Ok(Ok(n)) => n,
        _ => 0,
    };
    buffer.extend_from_slice(&rest[..rest_n]);

    // Enviar 200
    client_stream
        .write_all(format!("HTTP/1.1 200 {}\r\n\r\n", status).as_bytes())
        .await?;

    // Detectar backend
    let payload = String::from_utf8_lossy(&buffer);
    let addr_proxy = if payload.contains("SSH") || payload.contains("ssh") || payload.is_empty() || payload.starts_with('\0') {
        "127.0.0.1:22"
    } else {
        "127.0.0.1:22"
    };

    // Conectar ao backend
    let mut backend = match TcpStream::connect(addr_proxy).await {
        Ok(s) => s,
        Err(_) => {
            match TcpStream::connect("127.0.0.1:1194").await {
                Ok(s) => s,
                Err(e) => {
                    println!("[SDProxy] Backend falhou: {}", e);
                    return Ok(());
                }
            }
        }
    };

    // Enviar o payload lido para o backend
    backend.write_all(&buffer).await?;
    backend.flush().await?;

    // Tunnel bidirecional
    let (cr, cw) = client_stream.into_split();
    let (sr, sw) = backend.into_split();
    let cr = Arc::new(Mutex::new(cr));
    let cw = Arc::new(Mutex::new(cw));
    let sr = Arc::new(Mutex::new(sr));
    let sw = Arc::new(Mutex::new(sw));

    tokio::try_join!(
        transfer_data(cr, sw),
        transfer_data(sr, cw),
    )?;

    Ok(())
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
