mod socks5;
mod tls;
mod websocket;
mod tcp_fallback;
mod security;

use tokio::net::TcpListener;
use tokio::io::AsyncReadExt;
use clap::Parser;
use anyhow::Result;
use log::{info, error};
use std::process::Command;

#[derive(Parser)]
#[command(name = "awproxy")]
#[command(about = "Multiprotocol proxy server")]
struct Cli {
    #[arg(short = 'p', long = "port", default_value = "8080")]
    port: u16,
    #[arg(short = 'd', long = "debug")]
    debug: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    
    if cli.debug {
        env_logger::init();
    } else {
        env_logger::builder()
            .filter_level(log::LevelFilter::Info)
            .init();
    }
    
    let addr = format!("0.0.0.0:{}", cli.port);
    let listener = TcpListener::bind(&addr).await?;
    info!("🚀 AWProxy listening on {}", addr);
    info!("📡 Protocols: SOCKS5, TLS, WebSocket, SECURITY, TCP");

    while let Ok((socket, _)) = listener.accept().await {
        tokio::spawn(async move {
            let mut buf = [0u8; 24];
            match socket.peek(&mut buf).await {
                Ok(n) if n > 0 => {
                    match buf[0] {
                        0x05 => {
                            info!("🔐 SOCKS5");
                            let _ = socks5::handle_socks5(socket).await;
                        }
                        0x16 => {
                            info!("🔒 TLS");
                            let _ = tls::handle_tls(socket).await;
                        }
                        _ => {
                            let data_str = String::from_utf8_lossy(&buf[..n]);
                            // Verifica se é uma requisição HTTP (qualquer método)
                            if data_str.starts_with("GET ") || 
                               data_str.starts_with("POST ") || 
                               data_str.starts_with("PUT ") || 
                               data_str.starts_with("DELETE ") || 
                               data_str.starts_with("PATCH ") || 
                               data_str.starts_with("HEAD ") || 
                               data_str.starts_with("CONNECT ") || 
                               data_str.starts_with("OPTIONS ") || 
                               data_str.starts_with("TRACE ") || 
                               data_str.starts_with("HTTP/") {
                                info!("🌐 WebSocket/HTTP");
                                let _ = websocket::handle_websocket(socket).await;
                            } else if data_str.starts_with("SECURITY") || data_str.starts_with("AUTH") {
                                info!("🔐 SECURITY");
                                let _ = security::handle_security(socket).await;
                            } else {
                                info!("📦 TCP");
                                let _ = tcp_fallback::handle_tcp(socket).await;
                            }
                        }
                    }
                }
                Ok(_) => info!("Connection closed"),
                Err(e) => error!("Peek error: {}", e),
            }
        });
    }
    Ok(())
}
