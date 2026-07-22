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
    // O payload HTTP do Injector é APENAS o handshake para estabelecer o tunnel.
    // O SSH server na porta 22 é OpenSSH puro - ele NÃO espera receber HTTP.
    // Depois do handshake 101+200, o tunnel é bidirecional entre Injector e SSH.
    //
    // 1. Envia HTTP/1.1 101 {status}\r\n\r\n
    // 2. Lê payload HTTP do Injector (handshake - NÃO enviar ao SSH!)
    // 3. Envia HTTP/1.1 200 {status}\r\n\r\n
    // 4. Conecta ao backend SSH
    // 5. Faz tunnel bidirecional (sem enviar payload ao SSH)

    // PASSO 1: SEMPRE envia 101 Switching Protocols primeiro
    log::info!("📤 Enviando 101 Switching Protocols...");
    client_stream
        .write_all(format!("HTTP/1.1 101 {}\r\n\r\n", status).as_bytes())
        .await?;
    client_stream.flush().await?;

    // PASSO 2: SEMPRE lê do cliente (payload HTTP do Injector = handshake)
    // Este payload é APENAS para o handshake, NÃO deve ser enviado ao SSH server!
    let mut payload_buf = vec![0u8; 4096];
    let bytes_read = match timeout(Duration::from_millis(2000), client_stream.read(&mut payload_buf)).await {
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
    log::info!("📩 Payload handshake lido: {} bytes - {:?}", bytes_read, &payload[..std::cmp::min(bytes_read, 200)]);

    // PASSO 3: SEMPRE envia 200 OK
    log::info!("📤 Enviando 200 OK...");
    client_stream
        .write_all(format!("HTTP/1.1 200 {}\r\n\r\n", status).as_bytes())
        .await?;
    client_stream.flush().await?;

    // PASSO 4: Detecta backend e conecta
    if bytes_read == 0 {
        // Sem payload - fallback para TCP puro
        log::info!("📦 Sem payload - TCP fallback");
        return tcp_fallback::handle_tcp(client_stream).await.map_err(|e| Error::new(std::io::ErrorKind::Other, e));
    }

    // Detecta SSH vs VPN pelo conteúdo do payload
    let addr_proxy = if payload.contains("SSH") || payload.is_empty() {
        "127.0.0.1:22"
    } else {
        "127.0.0.1:1194"
    };

    log::info!("🔗 Backend detectado: {}", addr_proxy);

    // PASSO 5: Conecta ao backend
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
    // O payload HTTP NÃO é enviado ao SSH - é apenas o handshake!
    // O SSH server espera dados SSH, não HTTP.
    // O Injector agora envia dados SSH pelo tunnel.
    let (mut client_r, mut client_w) = client_stream.into_split();
    let (mut server_r, mut server_w) = server_stream.into_split();

    let client_r = Arc::new(Mutex::new(client_r));
    let client_w = Arc::new(Mutex::new(client_w));
    let server_r = Arc::new(Mutex::new(server_r));
    let server_w = Arc::new(Mutex::new(server_w));

    log::info!("🔗 Túnel bidirecional iniciado (payload HTTP descartado - apenas handshake)");
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
