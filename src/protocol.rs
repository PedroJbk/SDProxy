/// Detecta o protocolo usado pela conexão baseado nos primeiros bytes/dados.
/// Retorna uma string indicando o tipo de protocolo detectado.
pub fn detect_protocol(data: &str) -> String {
    if data.is_empty() {
        return "SSH".to_string();
    }

    let bytes = data.as_bytes();

    // TLS ClientHello: começa com 0x16 (handshake) seguido de 0x03 (TLS version)
    if bytes.len() >= 2 && bytes[0] == 0x16 && bytes[1] == 0x03 {
        return "TLS".to_string();
    }

    // TLS Application Data: 0x17
    if bytes.len() >= 2 && bytes[0] == 0x17 && bytes[1] == 0x03 {
        return "TLS".to_string();
    }

    // SSH banner
    if data.contains("SSH-") {
        return "SSH".to_string();
    }

    // WebSocket upgrade
    if data.contains("Upgrade: websocket") || data.contains("Sec-WebSocket-Key") ||
       data.contains("Upgrade: WebSocket") || data.contains("upgrade: websocket") {
        return "WEBSOCKET".to_string();
    }

    // xHTTP/Proto: headers customizados X- ou keyword xHTTP
    if data.contains("xHTTP") || data.contains("XHTTP") || data.contains("X-Proto") ||
       data.contains("x-proto") || data.contains("X-Split") || data.contains("Split") {
        return "XHTTP".to_string();
    }

    // HTTP methods com headers de segurança/xHTTP-like
    if data.starts_with("CONNECT") || data.starts_with("PROXY") ||
       data.contains("X-Status") || data.contains("X-Protocol") ||
       data.contains("Sec-WebSocket-Protocol") {
        return "SECURITY".to_string();
    }

    // HTTP genérico
    if data.starts_with("GET ") || data.starts_with("POST ") ||
       data.starts_with("PUT ") || data.starts_with("DELETE ") ||
       data.starts_with("HEAD ") || data.starts_with("OPTIONS ") ||
       data.starts_with("HTTP/") {
        return "HTTP".to_string();
    }

    // SOCKS5: versão 0x05
    if bytes.len() >= 2 && bytes[0] == 0x05 && (bytes[1] == 0x01 || bytes[1] == 0x00) {
        return "SOCKS5".to_string();
    }

    // Dados binários/diretos (Proto)
    if bytes.len() > 0 && !data.is_empty() {
        // Se não é texto legível e não é TLS, é provavelmente Proto
        if bytes.iter().take(10).filter(|&&b| (b as char).is_control() && b != 0x0d && b != 0x0a && b != 0x09).count() > 5 {
            return "PROTO".to_string();
        }
    }

    "SSH".to_string()
}
