use std::env;
use std::io::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt, copy_bidirectional};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{timeout, Duration};

#[tokio::main]
async fn main() -> Result<(), Error> {
    let port = get_port();
    let listener = TcpListener::bind(format!("[::]:{}", port)).await?;
    println!("Servidor iniciado na porta: {}", port);
    start_proxy(listener).await;
    Ok(())
}

async fn start_proxy(listener: TcpListener) {
    loop {
        match listener.accept().await {
            Ok((client_stream, addr)) => {
                println!("Nova conexão de: {}", addr);
                tokio::spawn(async move {
                    if let Err(e) = handle_client(client_stream).await {
                        eprintln!("Erro ao processar cliente {}: {}", addr, e);
                    }
                });
            }
            Err(e) => eprintln!("Erro ao aceitar conexão: {}", e),
        }
    }
}

async fn handle_client(mut client_stream: TcpStream) -> Result<(), Error> {
    let status = get_status();

    // Primeiro handshake 101
    client_stream
        .write_all(format!("HTTP/1.1 101 {}\r\n\r\n", status).as_bytes())
        .await?;

    let mut buffer = [0; 1024];
    client_stream.read(&mut buffer).await?;

    // Segundo handshake 101
    client_stream
        .write_all(format!("HTTP/1.1 101 {}\r\n\r\n", status).as_bytes())
        .await?;

    // Resposta 200 opcional
    client_stream
        .write_all(format!("HTTP/1.1 200 {}\r\n\r\n", status).as_bytes())
        .await?;

    let addr_proxy = match timeout(Duration::from_secs(5), peek_stream(&mut client_stream)).await {
        Ok(Ok(data)) if data.contains("SSH") || data.is_empty() => "0.0.0.0:22",
        Ok(_) => "0.0.0.0:1194",
        Err(_) => "0.0.0.0:22",
    };

    let mut server_stream = match TcpStream::connect(addr_proxy).await {
        Ok(stream) => stream,
        Err(_) => {
            eprintln!("Erro ao conectar-se ao servidor proxy em {}", addr_proxy);
            return Ok(());
        }
    };

    // Transfere dados entre cliente e servidor com buffer dinâmico
    let _ = copy_bidirectional(&mut client_stream, &mut server_stream).await;

    Ok(())
}

async fn peek_stream(stream: &TcpStream) -> Result<String, Error> {
    let mut buffer = vec![0; 8192];
    let bytes_peeked = stream.peek(&mut buffer).await?;
    Ok(String::from_utf8_lossy(&buffer[..bytes_peeked]).to_string())
}

fn get_port() -> u16 {
    env::args().nth(2).unwrap_or_else(|| "80".to_string()).parse().unwrap_or(80)
}

use std::env;

fn get_status() -> String {
    let protocol = env::args().nth(4).unwrap_or_else(|| "http".to_string());
    let code = env::args().nth(5).unwrap_or_else(|| "200".to_string());
    
    match protocol.to_lowercase().as_str() {
        // HTTP/1.1
        "http" | "http/1.1" => {
            format!("HTTP/1.1 {} {}", code, get_status_text(&code))
        },
        // HTTP/2
        "http/2" | "h2" => {
            format!("HTTP/2 {} {}", code, get_status_text(&code))
        },
        // HTTP/3
        "http/3" | "h3" => {
            format!("HTTP/3 {} {}", code, get_status_text(&code))
        },
        // WebSocket - sempre 101
        "ws" | "websocket" => {
            "101 Switching Protocols".to_string()
        },
        // HTTPS
        "https" => {
            format!("HTTP/1.1 {} {}", code, get_status_text(&code))
        },
        // Se for só o status (ex: "200 OK" ou "404")
        s if s.contains(' ') || s.parse::<u16>().is_ok() => {
            if s.contains(' ') {
                s.to_string()
            } else {
                format!("{} {}", s, get_status_text(s))
            }
        },
        // Fallback
        _ => "200 OK".to_string()
    }
}

fn get_status_text(code: &str) -> &'static str {
    match code {
        "100" => "Continue",
        "101" => "Switching Protocols",
        "102" => "Processing",
        "200" => "OK",
        "201" => "Created",
        "202" => "Accepted",
        "203" => "Non-Authoritative Information",
        "204" => "No Content",
        "205" => "Reset Content",
        "206" => "Partial Content",
        "300" => "Multiple Choices",
        "301" => "Moved Permanently",
        "302" => "Found",
        "303" => "See Other",
        "304" => "Not Modified",
        "305" => "Use Proxy",
        "307" => "Temporary Redirect",
        "308" => "Permanent Redirect",
        "400" => "Bad Request",
        "401" => "Unauthorized",
        "402" => "Payment Required",
        "403" => "Forbidden",
        "404" => "Not Found",
        "405" => "Method Not Allowed",
        "406" => "Not Acceptable",
        "407" => "Proxy Authentication Required",
        "408" => "Request Timeout",
        "409" => "Conflict",
        "410" => "Gone",
        "411" => "Length Required",
        "412" => "Precondition Failed",
        "413" => "Payload Too Large",
        "414" => "URI Too Long",
        "415" => "Unsupported Media Type",
        "416" => "Range Not Satisfiable",
        "417" => "Expectation Failed",
        "418" => "I'm a teapot",
        "426" => "Upgrade Required",
        "429" => "Too Many Requests",
        "431" => "Request Header Fields Too Large",
        "451" => "Unavailable For Legal Reasons",
        "500" => "Internal Server Error",
        "501" => "Not Implemented",
        "502" => "Bad Gateway",
        "503" => "Service Unavailable",
        "504" => "Gateway Timeout",
        "505" => "HTTP Version Not Supported",
        "511" => "Network Authentication Required",
        _ => "Unknown Status"
        
    }
 }
