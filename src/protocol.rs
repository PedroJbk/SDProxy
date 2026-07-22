pub fn detect_protocol(data: &str) -> String {
    if data.len() > 2 && data.as_bytes()[0] == 0x16 && data.as_bytes()[1] == 0x03 {
        return "TLS".to_string();
    }
    if data.contains("SSH-") {
        return "SSH".to_string();
    }
    if data.contains("Upgrade: websocket") || data.contains("Sec-WebSocket-Key") {
        return "WEBSOCKET".to_string();
    }
    if data.contains("X-") || data.contains("XHTTP") {
        return "XHTTP".to_string();
    }
    if data.contains("HTTP/") || data.contains("GET ") || data.contains("POST ") || data.contains("CONNECT") {
        return "HTTP".to_string();
    }
    "SSH".to_string()
}
