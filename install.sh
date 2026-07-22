#!/bin/bash
# SDProxy Install v2.1
# Robusto: baixa arquivos um a um via wget, compila e instala
set -e

SDPROXY="/opt/sdproxy"
PROXY_BIN="${SDPROXY}/proxy"
CERT_DIR="${SDPROXY}"
BUILD_DIR="/tmp/sdproxy_build_$$"
RAW="https://raw.githubusercontent.com/PedroJbk/SDProxy/main"

# Cores
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
CYAN='\033[0;36m'
NC='\033[0m'

echo -e "${CYAN}╔══════════════════════════════════╗${NC}"
echo -e "${CYAN}║      SDProxy Install v2.1        ║${NC}"
echo -e "${CYAN}║   + xHTTP SplitHTTP Support      ║${NC}"
echo -e "${CYAN}╚══════════════════════════════════╝${NC}"
echo ""

# Dependências
echo -e "${GREEN}[1/6] Verificando dependências...${NC}"
apt-get update -qq >/dev/null 2>&1 || true

if ! command -v git &> /dev/null; then
    echo -e "${YELLOW}  Instalando git...${NC}"
    apt-get install -y -qq git
fi

if ! command -v gcc &> /dev/null; then
    echo -e "${YELLOW}  Instalando build-essential...${NC}"
    apt-get install -y -qq build-essential
fi

if ! command -v openssl &> /dev/null; then
    echo -e "${YELLOW}  Instalando openssl...${NC}"
    apt-get install -y -qq openssl
fi

# Rust
if ! command -v cargo &> /dev/null; then
    echo -e "${YELLOW}  Instalando Rust...${NC}"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y 2>/dev/null
    source "$HOME/.cargo/env" 2>/dev/null || true
    export PATH="$HOME/.cargo/bin:$PATH"
fi

if ! command -v cargo &> /dev/null; then
    echo -e "${RED}  ❌ Cargo não encontrado no PATH!${NC}"
    echo -e "${YELLOW}  Execute: source \$HOME/.cargo/env${NC}"
    exit 1
fi
echo -e "${GREEN}  ✅ Rust/Cargo OK${NC}"

# SSH
if ! systemctl is-active --quiet ssh 2>/dev/null; then
    echo -e "${YELLOW}  Ativando SSH...${NC}"
    apt-get install -y -qq openssh-server 2>/dev/null || true
    systemctl enable ssh 2>/dev/null || true
    systemctl start ssh 2>/dev/null || true
fi

# Baixar arquivos do repositório
echo -e "${GREEN}[2/6] Baixando arquivos do GitHub...${NC}"
mkdir -p "${BUILD_DIR}/src"

echo -e "  ${YELLOW}Baixando Cargo.toml...${NC}"
wget -q "${RAW}/Cargo.toml" -O "${BUILD_DIR}/Cargo.toml" || { echo -e "${RED}  ❌ Falha!${NC}"; exit 1; }

echo -e "  ${YELLOW}Baixando Cargo.lock...${NC}"
wget -q "${RAW}/Cargo.lock" -O "${BUILD_DIR}/Cargo.lock" || true

echo -e "  ${YELLOW}Baixando main.rs...${NC}"
wget -q "${RAW}/src/main.rs" -O "${BUILD_DIR}/src/main.rs" || { echo -e "${RED}  ❌ Falha!${NC}"; exit 1; }

echo -e "  ${YELLOW}Baixando xhttp.rs...${NC}"
wget -q "${RAW}/src/xhttp.rs" -O "${BUILD_DIR}/src/xhttp.rs" || { echo -e "${RED}  ❌ Falha!${NC}"; exit 1; }

echo -e "  ${YELLOW}Baixando tls.rs...${NC}"
wget -q "${RAW}/src/tls.rs" -O "${BUILD_DIR}/src/tls.rs" || true

echo -e "  ${YELLOW}Baixando protocol.rs...${NC}"
wget -q "${RAW}/src/protocol.rs" -O "${BUILD_DIR}/src/protocol.rs" || true

echo -e "  ${YELLOW}Baixando websocket.rs...${NC}"
wget -q "${RAW}/src/websocket.rs" -O "${BUILD_DIR}/src/websocket.rs" || true

echo -e "  ${YELLOW}Baixando security.rs...${NC}"
wget -q "${RAW}/src/security.rs" -O "${BUILD_DIR}/src/security.rs" || true

echo -e "  ${YELLOW}Baixando tcp_fallback.rs...${NC}"
wget -q "${RAW}/src/tcp_fallback.rs" -O "${BUILD_DIR}/src/tcp_fallback.rs" || true

echo -e "  ${YELLOW}Baixando ssh.rs...${NC}"
wget -q "${RAW}/src/ssh.rs" -O "${BUILD_DIR}/src/ssh.rs" || true

echo -e "  ${YELLOW}Baixando socks5.rs...${NC}"
wget -q "${RAW}/src/socks5.rs" -O "${BUILD_DIR}/src/socks5.rs" || true

echo -e "  ${YELLOW}Baixando udp.rs...${NC}"
wget -q "${RAW}/src/udp.rs" -O "${BUILD_DIR}/src/udp.rs" || true

echo -e "  ${YELLOW}Baixando quic.rs...${NC}"
wget -q "${RAW}/src/quic.rs" -O "${BUILD_DIR}/src/quic.rs" || true

# Verificar que o Cargo.toml existe e é válido
echo -e "${GREEN}[3/6] Verificando arquivos baixados...${NC}"
if [ ! -f "${BUILD_DIR}/Cargo.toml" ]; then
    echo -e "${RED}  ❌ Cargo.toml não baixou! Verifique sua conexão.${NC}"
    rm -rf "${BUILD_DIR}"
    exit 1
fi

if [ ! -f "${BUILD_DIR}/src/main.rs" ]; then
    echo -e "${RED}  ❌ src/main.rs não baixou!${NC}"
    rm -rf "${BUILD_DIR}"
    exit 1
fi

FILES_COUNT=$(find "${BUILD_DIR}/src" -name "*.rs" | wc -l)
echo -e "  ✅ ${FILES_COUNT} módulos Rust baixados"

# Compilar
echo -e "${GREEN}[4/6] Compilando SDProxy (isso pode demorar alguns minutos)...${NC}"
cd "${BUILD_DIR}"
cargo build --release 2>&1 || {
    echo -e "${RED}  ❌ Compilação falhou!${NC}"
    echo -e "${YELLOW}  Logs: ver acima${NC}"
    rm -rf "${BUILD_DIR}"
    exit 1
}

# Instalar binário
echo -e "${GREEN}[5/6] Instalando...${NC}"
mkdir -p "${SDPROXY}"

if [ -f "${BUILD_DIR}/target/release/sdproxy" ]; then
    cp "${BUILD_DIR}/target/release/sdproxy" "${PROXY_BIN}"
    chmod +x "${PROXY_BIN}"
    echo -e "  ✅ Binário: ${PROXY_BIN}"
else
    echo -e "${RED}  ❌ Binário não gerado!${NC}"
    ls -la "${BUILD_DIR}/target/release/" 2>/dev/null
    rm -rf "${BUILD_DIR}"
    exit 1
fi

# Instalar menu
wget -q "${RAW}/menu.sh" -O "${SDPROXY}/menu.sh" 2>/dev/null || true
if [ -f "${SDPROXY}/menu.sh" ]; then
    chmod +x "${SDPROXY}/menu.sh"
    ln -sf "${SDPROXY}/menu.sh" /usr/local/bin/sdproxy
    echo -e "  ✅ Menu: sdproxy"
fi

# Certificados TLS
echo -e "${GREEN}[6/6] Gerando certificados TLS...${NC}"
if [ ! -f "${CERT_DIR}/cert.pem" ] || [ ! -f "${CERT_DIR}/key.pem" ]; then
    openssl req -x509 -newkey rsa:2048 -keyout "${CERT_DIR}/key.pem" \
        -out "${CERT_DIR}/cert.pem" -days 365 -nodes \
        -subj "/CN=sdproxy/O=SDProxy/C=BR" 2>/dev/null
    echo -e "  ✅ Certificados gerados"
else
    echo -e "  ✅ Certificados existentes"
fi

# Limpar
rm -rf "${BUILD_DIR}"

# Parar serviços antigos
for svc in /etc/systemd/system/proxy-*.service; do
    [ -f "$svc" ] && systemctl stop "$(basename "$svc" .service)" 2>/dev/null
done

echo ""
echo -e "${GREEN}╔══════════════════════════════════╗${NC}"
echo -e "${GREEN}║     ✅ SDProxy v2.1 Instalado!   ║${NC}"
echo -e "${GREEN}╠══════════════════════════════════╣${NC}"
echo -e "${GREEN}║  Binário: ${PROXY_BIN}          ║${NC}"
echo -e "${GREEN}║  Certs:   ${CERT_DIR}/           ║${NC}"
echo -e "${GREEN}║  Menu:    sdproxy                ║${NC}"
echo -e "${GREEN}╚══════════════════════════════════╝${NC}"
echo ""
echo -e "${CYAN}Execute:${NC} sdproxy"
echo -e "${CYAN}Escolha:${NC} [04] xHTTP SplitHTTP (porta 443)"
echo ""
