#!/bin/bash
# SDProxy Installer v2.0
set -e

BLUE='\033[1;34m'
GREEN='\033[1;32m'
RED='\033[1;31m'
YELLOW='\033[1;33m'
WHITE='\033[1;37m'
NC='\033[0m'
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
echo -e "${WHITE} Multi-Protocolo Proxy v2.0"
echo -e "${WHITE} GitHub: ${BLUE}github.com/PedroJbk/SDProxy${NC}"
echo -e "${BLUE}${BOLD}--------------------------------------------------------------${NC}"
echo ""

# Etapa 1
echo -e "${GREEN}[1/4]${NC} Verificando dependências..."
apt-get update -qq >/dev/null 2>&1 || true
for pkg in git build-essential openssl openssh-server; do
    if ! dpkg -l | grep -q "^ii  $pkg "; then
        apt-get install -y -qq "$pkg" >/dev/null 2>&1
        echo -e "  ${GREEN}✔${NC} $pkg instalado"
    fi
done

if ! command -v cargo &> /dev/null; then
    echo -e "  ${YELLOW}→${NC} Instalando Rust..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y >/dev/null 2>&1
    source "$HOME/.cargo/env" 2>/dev/null || true
    export PATH="$HOME/.cargo/bin:$PATH"
    echo -e "  ${GREEN}✔${NC} Rust instalado"
fi

echo ""

# Etapa 2
echo -e "${GREEN}[2/4]${NC} Baixando arquivos..."
rm -rf "${BUILD_DIR}"
mkdir -p "${BUILD_DIR}/src"

wget -q "${RAW}/Cargo.toml" -O "${BUILD_DIR}/Cargo.toml" 2>/dev/null
wget -q "${RAW}/Cargo.lock" -O "${BUILD_DIR}/Cargo.lock" 2>/dev/null || true

for file in main.rs xhttp.rs websocket.rs security.rs tcp_fallback.rs tls.rs ssh.rs; do
    wget -q "${RAW}/src/${file}" -O "${BUILD_DIR}/src/${file}" 2>/dev/null
done

if [ ! -f "${BUILD_DIR}/Cargo.toml" ] || [ ! -f "${BUILD_DIR}/src/main.rs" ]; then
    echo -e "  ${RED}✘ Erro ao baixar arquivos${NC}"
    exit 1
fi

echo -e "  ${GREEN}✔${NC} $(find ${BUILD_DIR}/src -name '*.rs' | wc -l) módulos baixados"
echo ""

# Etapa 3
echo -e "${GREEN}[3/4]${NC} Compilando..."
cd "${BUILD_DIR}"

if ! command -v cargo &> /dev/null; then
    export PATH="$HOME/.cargo/bin:$PATH"
fi

cargo build --release >/dev/null 2>&1

if [ ! -f "${BUILD_DIR}/target/release/sdproxy" ]; then
    echo -e "  ${RED}✘ Compilação falhou${NC}"
    rm -rf "${BUILD_DIR}"
    exit 1
fi

echo -e "  ${GREEN}✔${NC} Compilado com sucesso"
echo ""

# Etapa 4
echo -e "${GREEN}[4/4]${NC} Instalando..."
mkdir -p "${INSTALL_DIR}"

cp "${BUILD_DIR}/target/release/sdproxy" "${INSTALL_DIR}/proxy"
chmod +x "${INSTALL_DIR}/proxy"

wget -q "${RAW}/menu.sh" -O "${INSTALL_DIR}/menu.sh" 2>/dev/null || true
chmod +x "${INSTALL_DIR}/menu.sh" 2>/dev/null
ln -sf "${INSTALL_DIR}/menu.sh" /usr/local/bin/sdproxy 2>/dev/null || true

# Gerar certificados se não existirem
if [ ! -f "${INSTALL_DIR}/cert.pem" ] || [ ! -f "${INSTALL_DIR}/key.pem" ]; then
    openssl req -x509 -newkey rsa:2048 -keyout "${INSTALL_DIR}/key.pem" \
        -out "${INSTALL_DIR}/cert.pem" -days 365 -nodes \
        -subj "/CN=sdproxy/O=SDProxy/C=BR" 2>/dev/null
fi

rm -rf "${BUILD_DIR}"

# Limpar serviços antigos
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
echo -e "    ├─ TLS/SSL Proxy"
echo -e "    └─ Security Check"
echo ""
echo -e "  ${WHITE}${BOLD}Comando:${NC} sdproxy"
echo ""
echo -e "${BLUE}${BOLD}--------------------------------------------------------------${NC}"
