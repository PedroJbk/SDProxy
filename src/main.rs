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
    // FLUXO PRINCIPAL: Baseado no BSProxy que funciona perfeitamente
    // ============================================================
    //
    // O BSProxy tem um fluxo MUITO simples no handle_client:
    // 1. SEMPRE envia HTTP/1.1 101 {status}\r\n\r\n primeiro
    // 2. SEMPRE lê do cliente
    // 3. SEMPRE envia HTTP/1.1 200 {status}\r\n\r\n
    // 4. Depois detecta SSH vs VPN pelo peek
    //
    // O AWProxy anterior estava tentando fazer detecção ANTES de responder,
    // o que faz o Injector fechar a conexão.

    // PASSO 1: SEMPRE envia 101 Switching Protocols primeiro
    log::info!("📤 Enviando 101 Switching Protocols...");
    client_stream
        .write_all(format!("HTTP/1.1 101 {}\r\n\r\n", status).as_bytes())
        .await?;
    client_stream.flush().await?;

    // PASSO 2: SEMPRE lê do cliente (payload do Injector)
    let mut payload_buf = vec![0u8; 1024];
    let bytes_read = match timeout(Duration::from_millis(500), client_stream.read(&mut payload_buf)).await {
        Ok(Ok(n)) => n,
        Ok(Err(e)) => {
            log::warn!("⚠️ Erro ao ler payload: {}", e);
            0
        }
        Err(_) => {
            log::debug!("⚠️ Timeout ao ler payload");
            0
        }
    };

    let payload = String::from_utf8_lossy(&payload_buf[..bytes_read]);
    log::debug!("📩 Payload ({} bytes): {:?}", bytes_read, &payload[..std::cmp::min(bytes_read, 200)]);

    // PASSO 3: SEMPRE envia 200 OK
    log::info!("📤 Enviando 200 OK...");
    client_stream
        .write_all(format!("HTTP/1.1 200 {}\r\n\r\n", status).as_bytes())
        .await?;
    client_stream.flush().await?;

    // PASSO 4: Detecta protocolo e encaminha para backend
    // Só agora fazemos detecção, depois do handshake completo

    if bytes_read == 0 {
        // Sem payload - fallback para TCP puro
        log::info!("📦 Sem payload - TCP fallback");
        return tcp_fallback::handle_tcp(client_stream).await.map_err(|e| Error::new(std::io::ErrorKind::Other, e));
    }

    let first_byte = payload_buf[0];

    // SOCKS5 (primeiro byte = 0x05) - mas já enviamos 101+200, então é tarde demais
    // Na prática, SOCKS5 não passa pelo Injector, então não deveria chegar aqui
    if first_byte == 0x05 {
        log::info!("🔐 SOCKS5 detectado (após handshake) - encaminhando para SOCKS5 handler");
        return Ok(());
    }

    // TLS/SSL Handshake (0x16)
    if first_byte == 0x16 {
        log::info!("🛡️ TLS detectado (após handshake) - passthrough");
        return tls::handle_tls(client_stream).await.map_err(|e| Error::new(std::io::ErrorKind::Other, e));
    }

    // Detecta SSH vs VPN pelo conteúdo do payload
    let addr_proxy = if payload.contains("SSH") || payload.is_empty() {
        "127.0.0.1:22"
    } else {
        "127.0.0.1:1194"
    };

    log::info!("🔗 Backend detectado: {} (payload: {:?})", addr_proxy, &payload[..std::cmp::min(payload.len(), 100)]);

    // Conecta ao backend e faz tunnel bidirecional
    let server_connect = TcpStream::connect(addr_proxy).await;
    if server_connect.is_err() {
        let alt = if addr_proxy == "127.0.0.1:22" { "127.0.0.1:1194" } else { "127.0.0.1:22" };
        log::warn!("⚠️ Falha em {}, tentando {}", addr_proxy, alt);
        match TcpStream::connect(alt).await {
            Ok(s) => {
                log::info!("✅ Túnel iniciado para {}", alt);
                let (cr, cw) = client_stream.into_split();
                let (sr, sw) = s.into_split();
                let cr = Arc::new(Mutex::new(cr));
                let cw = Arc::new(Mutex::new(cw));
                let sr = Arc::new(Mutex::new(sr));
                let sw = Arc::new(Mutex::new(sw));
                tokio::try_join!(transfer_data(cr, sw), transfer_data(sr, cw))?;
                Ok(())
            }
            Err(_) => {
                log::warn!("⚠️ Ambos backends falharam");
                Ok(())
            }
        }
    } else {
        let server_stream = server_connect?;
        log::info!("✅ Túnel iniciado para {}", addr_proxy);
        let (cr, cw) = client_stream.into_split();
        let (sr, sw) = server_stream.into_split();
        let cr = Arc::new(Mutex::new(cr));
        let cw = Arc::new(Mutex::new(cw));
        let sr = Arc::new(Mutex::new(sr));
        let sw = Arc::new(Mutex::new(sw));
        tokio::try_join!(transfer_data(cr, sw), transfer_data(sr, cw))?;
        Ok(())
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
