#!/bin/bash
# SDProxy Installer v2.1
set -e

BLUE='\033[1;34m'
GREEN='\033[1;32m'
RED='\033[1;31m'
YELLOW='\033[1;33m'
WHITE='\033[1;37m'
NC='\033[0m'
BOLD='\033[1m'
BOLD='\033[1m'

RAW="https://raw.githubusercontent.com/PedroJbk/SDProxy/main"
BUILD_DIR="/tmp/sdproxy_build"
INSTALL_DIR="/opt/sdproxy"

clear

echo -e "${BLUE}${BOLD} ███████╗██████╗ ██████╗ ██████╗  ██████╗ ██╗  ██╗██╗   ██╗${NC}"
echo -e "${WHITE}${BOLD} ██╔════╝██╔══██╗██╔══██╗██╔══██╗██╔═══██╗╚██╗██╔╝╚██╗ ██╔╝${NC}"
echo -e "${BLUE}${BOLD} ███████╗██║  ██║██████╔╝██████╔╝██║   ██║ ╚███╔╝  ╚████╔╝ ${NC}"
echo -e "${WHITE}${BOLD} ╚════██║██║  ██║██╔═══╝ ██╔══██╗██║   ██║ ██╔██╗   ╚██╔╝  ${NC}"
echo -e "${BLUE}${BOLD} ███████║██████╔╝██║     ██║  ██║╚██████╔╝██╔╝ ██╗   ██║   ${NC}"
echo -e "${WHITE}${BOLD} ╚══════╝╚═════╝ ╚═╝     ╚═╝  ╚═╝ ╚═════╝ ╚═╝  ╚═╝   ╚═╝   ${NC}"
echo -e "${BLUE}${BOLD}--------------------------------------------------------------${NC}"
echo -e "${WHITE} Multi-Protocolo Proxy v2.1"
echo -e "${WHITE} GitHub: ${BLUE}github.com/PedroJbk/SDProxy${NC}"
echo -e "${BLUE}${BOLD}--------------------------------------------------------------${NC}"
echo ""

# Etapa 1: Dependências
echo -e "${GREEN}[1/5]${NC} Verificando dependências..."
apt-get update -qq >/dev/null 2>&1 || true

if ! command -v git &> /dev/null; then
    apt-get install -y -qq git >/dev/null 2>&1
    echo -e "  ${GREEN}✔${NC} git instalado"
fi

if ! command -v gcc &> /dev/null; then
    apt-get install -y -qq build-essential >/dev/null 2>&1
    echo -e "  ${GREEN}✔${NC} build-essential instalado"
fi

if ! command -v openssl &> /dev/null; then
    apt-get install -y -qq openssl >/dev/null 2>&1
    echo -e "  ${GREEN}✔${NC} openssl instalado"
fi

if ! command -v cargo &> /dev/null; then
    echo -e "  ${YELLOW}→${NC} Instalando Rust..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y >/dev/null 2>&1
    source "$HOME/.cargo/env" 2>/dev/null || true
    export PATH="$HOME/.cargo/bin:$PATH"
    echo -e "  ${GREEN}✔${NC} Rust instalado"
else
    echo -e "  ${GREEN}✔${NC} Rust já instalado"
fi

if ! systemctl is-active --quiet ssh 2>/dev/null; then
    apt-get install -y -qq openssh-server >/dev/null 2>&1 || true
    systemctl enable ssh 2>/dev/null || true
    systemctl start ssh 2>/dev/null || true
    echo -e "  ${GREEN}✔${NC} SSH ativado"
fi

echo ""

# Etapa 2: Download
echo -e "${GREEN}[2/5]${NC} Baixando arquivos..."
rm -rf "${BUILD_DIR}"
mkdir -p "${BUILD_DIR}/src"

for file in Cargo.toml Cargo.lock src/main.rs src/xhttp.rs src/tls.rs src/protocol.rs src/websocket.rs src/security.rs src/tcp_fallback.rs src/ssh.rs src/socks5.rs src/udp.rs src/quic.rs; do
    wget -q "${RAW}/${file}" -O "${BUILD_DIR}/${file}" 2>/dev/null
done

if [ ! -f "${BUILD_DIR}/Cargo.toml" ] || [ ! -f "${BUILD_DIR}/src/main.rs" ]; then
    echo -e "  ${RED}✘ Erro ao baixar arquivos${NC}"
    exit 1
fi

echo -e "  ${GREEN}✔${NC} $(find ${BUILD_DIR}/src -name '*.rs' | wc -l) módulos baixados"
echo ""

# Etapa 3: Compilação
echo -e "${GREEN}[3/5]${NC} Compilando SDProxy..."
cd "${BUILD_DIR}"
cargo build --release >/dev/null 2>&1

if [ ! -f "${BUILD_DIR}/target/release/sdproxy" ]; then
    echo -e "  ${RED}✘ Compilação falhou${NC}"
    rm -rf "${BUILD_DIR}"
    exit 1
fi

echo -e "  ${GREEN}✔${NC} Compilado com sucesso"
echo ""

# Etapa 4: Instalação
echo -e "${GREEN}[4/5]${NC} Instalando..."
mkdir -p "${INSTALL_DIR}"

cp "${BUILD_DIR}/target/release/sdproxy" "${INSTALL_DIR}/proxy"
chmod +x "${INSTALL_DIR}/proxy"

wget -q "${RAW}/menu.sh" -O "${INSTALL_DIR}/menu.sh" 2>/dev/null || true
chmod +x "${INSTALL_DIR}/menu.sh" 2>/dev/null
ln -sf "${INSTALL_DIR}/menu.sh" /usr/local/bin/sdproxy 2>/dev/null || true

if [ ! -f "${INSTALL_DIR}/cert.pem" ] || [ ! -f "${INSTALL_DIR}/key.pem" ]; then
    openssl req -x509 -newkey rsa:2048 -keyout "${INSTALL_DIR}/key.pem" \
        -out "${INSTALL_DIR}/cert.pem" -days 365 -nodes \
        -subj "/CN=sdproxy/O=SDProxy/C=BR" 2>/dev/null
fi

rm -rf "${BUILD_DIR}"

echo -e "  ${GREEN}✔${NC} Binário instalado"
echo -e "  ${GREEN}✔${NC} Certificados TLS gerados"
echo ""

# Etapa 5: Finalizar
echo -e "${GREEN}[5/5]${NC} Finalizando..."

for svc in /etc/systemd/system/proxy-*.service; do
    [ -f "$svc" ] && systemctl stop "$(basename "$svc" .service)" 2>/dev/null && systemctl disable "$(basename "$svc" .service)" 2>/dev/null
done

clear

echo -e "${BLUE}${BOLD} ███████╗██████╗ ██████╗ ██████╗  ██████╗ ██╗  ██╗██╗   ██╗${NC}"
echo -e "${WHITE}${BOLD} ██╔════╝██╔══██╗██╔══██╗██╔══██╗██╔═══██╗╚██╗██╔╝╚██╗ ██╔╝${NC}"
echo -e "${BLUE}${BOLD} ███████╗██║  ██║██████╔╝██████╔╝██║   ██║ ╚███╔╝  ╚████╔╝ ${NC}"
echo -e "${WHITE}${BOLD} ╚════██║██║  ██║██╔═══╝ ██╔══██╗██║   ██║ ██╔██╗   ╚██╔╝  ${NC}"
echo -e "${BLUE}${BOLD} ███████║██████╔╝██║     ██║  ██║╚██████╔╝██╔╝ ██╗   ██║   ${NC}"
echo -e "${WHITE}${BOLD} ╚══════╝╚═════╝ ╚═╝     ╚═╝  ╚═╝ ╚═════╝ ╚═╝  ╚═╝   ╚═╝   ${NC}"
echo -e "${BLUE}${BOLD}--------------------------------------------------------------${NC}"
echo ""
echo -e "  ${GREEN}${BOLD}✔ Instalação concluída!${NC}"
echo ""
echo -e "  ${BLUE}${BOLD}Protocolos:${NC}"
echo -e "    ├─ SSH Tunnel"
echo -e "    ├─ WebSocket"
echo -e "    ├─ ${YELLOW}xHTTP / SplitHTTP (porta 443)${NC}"
echo -e "    ├─ TLS/SSL"
echo -e "    ├─ SOCKS5"
echo -e "    ├─ QUIC"
echo -e "    └─ UDP"
echo ""
echo -e "  ${WHITE}${BOLD}Comando:${NC} sdproxy"
echo -e "  ${WHITE}${BOLD}Opção:${NC} [04] xHTTP SplitHTTP"
echo ""
echo -e "${BLUE}${BOLD}--------------------------------------------------------------${NC}"
