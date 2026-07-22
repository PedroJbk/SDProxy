//! Protocol Detector
//! Detecta o protocolo baseado nos primeiros bytes recebidos
//!
//! Ordem de detecção:
//! 1. TLS/SSL (0x16 0x03) - TLS ClientHello
//! 2. SSH (SSH-2.0 ou SSH-1.99)
//! 3. WebSocket (GET / HTTP/1.1 com Upgrade: websocket)
//! 4. HTTP GET (GET /path...)
//! 5. HTTP POST (POST /path...)
//! 6. SOCKS5 (0x05)
//! 7. Proto (Protocolo genérico)
//! 8. TCP Fallback

pub fn detect_protocol(data: &str) -> String {
    if data.is_empty() {
        return "TCP".to_string();
    }

    // TLS/SSL: primeiro byte 0x16 (Handshake), segundo 0x03 (TLS 1.0+)
    let bytes = data.as_bytes();
    if bytes.len() >= 2 && bytes[0] == 0x16 && bytes[1] == 0x03 {
        return "TLS".to_string();
    }

    // SSH: começa com "SSH-"
    if data.starts_with("SSH-") {
        return "SSH".to_string();
    }

    // WebSocket: GET / com Upgrade: websocket
    if data.contains("GET ") && data.contains("Upgrade: websocket") {
        return "WEBSOCKET".to_string();
    }

    // HTTP GET com path que sugere xHTTP
    // GET /ssh/{session-id} ou GET /{path}/{session-id}
    if data.starts_with("GET ") {
        let first_line = data.lines().next().unwrap_or("");
        let parts: Vec<&str> = first_line.split_whitespace().collect();
        if parts.len() >= 2 {
            let path = parts[1];
            // Se o path tem mais de 2 segmentos (ex: /ssh/{id} ou /{base}/{session})
            // é provavelmente xHTTP
            let path_parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
            if path_parts.len() >= 2 {
                // Pode ser xHTTP ou HTTP normal
                // Verificar se tem headers xHTTP específicos
                if data.contains("X-Session") || data.contains("Transfer-Encoding") {
                    return "XHTTP".to_string();
                }
                // Se é na porta 443 com path /ssh/ ou similar, é xHTTP
                if path.contains("/ssh/") || path.contains("/xhttp/") || path.contains("/split/") {
                    return "XHTTP".to_string();
                }
                // Path genérico com múltiplos segmentos → HTTP
                return "HTTP".to_string();
            }
        }
        return "HTTP".to_string();
    }

    // HTTP POST - usado pelo xHTTP para uplink
    if data.starts_with("POST ") {
        let first_line = data.lines().next().unwrap_or("");
        let parts: Vec<&str> = first_line.split_whitespace().collect();
        if parts.len() >= 2 {
            let path = parts[1];
            // POST /ssh/{session-id}/{seq} → xHTTP
            let path_parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
            if path_parts.len() >= 2 {
                if path.contains("/ssh/") || path.contains("/xhttp/") || path.contains("/split/") {
                    return "XHTTP".to_string();
                }
                return "XHTTP".to_string(); // POST com path estruturado → xHTTP
            }
        }
        return "HTTP".to_string();
    }

    // SOCKS5: primeiro byte 0x05
    if bytes[0] == 0x05 {
        return "SOCKS5".to_string();
    }

    // Proto: começa com @ ou #
    if data.starts_with("@") || data.starts_with("#") || data.starts_with("!") {
        return "PROTO".to_string();
    }

    // Security/HTTP básico
    if data.starts_with("CONNECT ") || data.starts_with("HEAD ") || data.starts_with("OPTIONS ") {
        return "SECURITY".to_string();
    }

    // Fallback TCP
    "TCP".to_string()
}
