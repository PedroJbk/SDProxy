use std::env;
use std::io::Error;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tokio::time::{timeout, Duration};

mod socks5;
mod websocket;
mod security;
mod tcp_fallback;
mod tls;
mod ssh;

#[tokio::main]
async fn main() -> Result<(), Error> {
    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "info");
    }
    env_logger::init();

    let args: Vec<String> = env::args().collect();
    let config = parse_args(&args);

    let port = config.port;
    let status = config.status.clone();
    let use_tls = config.tls;

    log::info!("🚀 AWProxy iniciando na porta {} | Status: '{}'", port, status);

    let listener = TcpListener::bind(format!("[::]:{}", port)).await?;
    println!("Servidor iniciado na porta: {}", port);

    start_proxy(listener, status, use_tls).await;
    Ok(())
}

async fn start_proxy(listener: TcpListener, status: String, use_tls: bool) {
    loop {
        let status_clone = status.clone();
        match listener.accept().await {
            Ok((client_stream, addr)) => {
                tokio::spawn(async move {
                    if let Err(e) = handle_client(client_stream, &status_clone, use_tls).await {
                        eprintln!("Erro ao processar cliente {}: {}", addr, e);
                    }
                });
            }
            Err(e) => eprintln!("Erro ao aceitar conexão: {}", e),
        }
    }
}

async fn handle_client(mut client_stream: TcpStream, status: &str, use_tls: bool) -> Result<(), Error> {

    // Modo TLS/HTTPS: apenas passthrough (sem handshake HTTP)
    if use_tls {
        return tls::handle_tls(client_stream).await.map_err(|e| Error::new(std::io::ErrorKind::Other, e));
    }

    // ============================================================
    // FLUXO PRINCIPAL: Baseado no padrão HTTP Injector com [split]
    // ============================================================
    //
    // O Injector envia o payload em 2 partes (separadas por [split]):
    //   Parte 1: "ACL /HTTP/1.1" (sem [split])
    //   Parte 2: "\r\nHost: ... \r\nConnection: Upgrade\nUpgrade: websocket\n\n" (com [split])
    //
    // Fluxo correto:
    //   1. Recebe parte 1 do payload do Injector
    //   2. Envia HTTP/1.1 101 {status}\r\n\r\n
    //   3. Recebe parte 2 do payload do Injector
    //   4. Envia HTTP/1.1 200 {status}\r\n\r\n
    //   5. Conecta ao backend SSH
    //   6. Faz tunnel bidirecional

    // PASSO 1: Recebe parte 1 do payload (antes do [split])
    log::info!("📥 Aguardando parte 1 do payload...");
    let mut buf = [0u8; 4096];
    let n1 = match timeout(Duration::from_millis(3000), client_stream.read(&mut buf)).await {
        Ok(Ok(n)) => n,
        Ok(Err(e)) => {
            log::warn!("⚠️ Erro ao ler parte 1: {}", e);
            return Ok(());
        }
        Err(_) => {
            log::warn!("⚠️ Timeout ao receber parte 1");
            return Ok(());
        }
    };

    let part1 = String::from_utf8_lossy(&buf[..n1]);
    log::info!("📥 Parte 1 recebida: {} bytes - {:?}", n1, &part1[..std::cmp::min(n1, 200)]);

    // PASSO 2: Envia 101 Switching Protocols (resposta à parte 1)
    log::info!("📤 Enviando 101 Switching Protocols...");
    client_stream
        .write_all(format!("HTTP/1.1 101 {}\r\n\r\n", status).as_bytes())
        .await?;
    client_stream.flush().await?;

    // PASSO 3: Recebe parte 2 do payload (depois do [split])
    log::info!("📥 Aguardando parte 2 do payload...");
    let mut buf2 = [0u8; 4096];
    let n2 = match timeout(Duration::from_millis(3000), client_stream.read(&mut buf2)).await {
        Ok(Ok(n)) => n,
        Ok(Err(e)) => {
            log::warn!("⚠️ Erro ao ler parte 2: {}", e);
            return Ok(());
        }
        Err(_) => {
            log::debug!("⚠️ Timeout ao receber parte 2 - usando 0 bytes");
            0
        }
    };

    let part2 = String::from_utf8_lossy(&buf2[..n2]);
    log::info!("📥 Parte 2 recebida: {} bytes - {:?}", n2, &part2[..std::cmp::min(n2, 200)]);

    // Detecta SSH vs VPN pelo conteúdo do payload completo
    let full_payload = format!("{}{}", part1, part2);
    let addr_proxy = if full_payload.contains("SSH") || (part1.contains("SSH") || part2.contains("SSH")) {
        "127.0.0.1:22"
    } else {
        "127.0.0.1:1194"
    };

    // PASSO 4: Envia 200 OK (resposta à parte 2)
    log::info!("📤 Enviando 200 OK...");
    client_stream
        .write_all(format!("HTTP/1.1 200 {}\r\n\r\n", status).as_bytes())
        .await?;
    client_stream.flush().await?;

    // PASSO 5: Conecta ao backend
    log::info!("🔗 Conectando ao backend: {}", addr_proxy);

    let server_stream = match TcpStream::connect(addr_proxy).await {
        Ok(s) => s,
        Err(e) => {
            log::warn!("⚠️ Falha em {}: {}. Tentando fallback...", addr_proxy, e);
            let alt = if addr_proxy == "127.0.0.1:22" { "127.0.0.1:1194" } else { "127.0.0.1:22" };
            match TcpStream::connect(alt).await {
                Ok(s) => {
                    log::info!("✅ Conectado ao fallback: {}", alt);
                    s
                }
                Err(e2) => {
                    log::error!("❌ Ambos backends falharam: {}, {}", e, e2);
                    return Ok(());
                }
            }
        }
    };

    log::info!("✅ Conectado ao backend: {}", addr_proxy);

    // PASSO 6: Tunnel bidirecional
    let (client_r, client_w) = client_stream.into_split();
    let (server_r, server_w) = server_stream.into_split();

    let client_r = Arc::new(Mutex::new(client_r));
    let client_w = Arc::new(Mutex::new(client_w));
    let server_r = Arc::new(Mutex::new(server_r));
    let server_w = Arc::new(Mutex::new(server_w));

    log::info!("🔗 Túnel bidirecional iniciado");
    tokio::try_join!(
        transfer_data(client_r, server_w.clone()),
        transfer_data(server_r, client_w.clone()),
    )?;

    log::info!("🔚 Túnel finalizado.");
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

struct ProxyConfig {
    port: u16,
    status: String,
    tls: bool,
}

fn parse_args(args: &[String]) -> ProxyConfig {
    let mut port = 80u16;
    let mut status = "200 OK".to_string();
    let mut tls = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-p" => { if i+1 < args.len() { port = args[i+1].parse().unwrap_or(80); i+=1; } }
            "-s" => { if i+1 < args.len() { status = args[i+1].clone(); i+=1; } }
            "-t" => { tls = true; }
            _ => {}
        }
        i += 1;
    }
    ProxyConfig { port, status, tls }
}
