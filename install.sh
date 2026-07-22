#!/bin/bash

# ============================================
# SDProxy Install Script v2.0
# Baixa, compila e instala o SDProxy com suporte xHTTP
# ============================================

SDPROXY="/opt/sdproxy"
PROXY_BIN="${SDPROXY}/proxy"
CERT_DIR="${SDPROXY}"
BUILD_DIR="/tmp/sdproxy_build"
REPO_URL="https://github.com/PedroJbk/SDProxy.git"
BRANCH="main"

# Cores
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
CYAN='\033[0;36m'
NC='\033[0m'

echo -e "${CYAN}╔══════════════════════════════════╗${NC}"
echo -e "${CYAN}║      SDProxy Install v2.0        ║${NC}"
echo -e "${CYAN}║   + xHTTP SplitHTTP Support      ║${NC}"
echo -e "${CYAN}╚══════════════════════════════════╝${NC}"
echo ""

# Verificar systemd
if ! command -v systemctl &> /dev/null; then
    echo -e "${RED}❌ systemd não encontrado!${NC}"
    exit 1
fi

# Instalar dependências do sistema
echo -e "${GREEN}Verificando dependências...${NC}"

# OpenSSL
if ! command -v openssl &> /dev/null; then
    echo -e "${YELLOW}Instalando OpenSSL...${NC}"
    apt-get update -qq && apt-get install -y -qq openssl
fi

# Git
if ! command -v git &> /dev/null; then
    echo -e "${YELLOW}Instalando Git...${NC}"
    apt-get update -qq && apt-get install -y -qq git
fi

# Build essentials (gcc, make, etc.)
if ! dpkg -l | grep -q build-essential; then
    echo -e "${YELLOW}Instalando build-essential...${NC}"
    apt-get update -qq && apt-get install -y -qq build-essential
fi

# Rust
if ! command -v cargo &> /dev/null; then
    echo -e "${YELLOW}Instalando Rust...${NC}"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y 2>/dev/null
    source "$HOME/.cargo/env" 2>/dev/null
    export PATH="$HOME/.cargo/bin:$PATH"
fi

# Verificar se cargo está no PATH agora
if ! command -v cargo &> /dev/null; then
    echo -e "${RED}❌ Falha ao instalar Rust. Verifique o PATH.${NC}"
    exit 1
fi

# SSH
if ! command -v ssh &> /dev/null; then
    echo -e "${YELLOW}Instalando OpenSSH Server...${NC}"
    apt-get install -y -qq openssh-server
    systemctl enable ssh
    systemctl start ssh
fi

# Limpar build anterior
rm -rf "${BUILD_DIR}"
mkdir -p "${BUILD_DIR}"

# Baixar repositório
echo -e "${GREEN}Baixando SDProxy v2.0 do GitHub...${NC}"
git clone --depth 1 --branch "${BRANCH}" "${REPO_URL}" "${BUILD_DIR}" 2>&1

if [ ! -f "${BUILD_DIR}/Cargo.toml" ]; then
    echo -e "${RED}❌ Falha ao baixar o repositório!${NC}"
    echo -e "${YELLOW}Verifique sua conexão com o GitHub.${NC}"
    exit 1
fi

# Compilar
echo -e "${GREEN}Compilando SDProxy...${NC}"
cd "${BUILD_DIR}"
cargo build --release 2>&1

# Copiar binário
mkdir -p "${SDPROXY}"
if [ -f "${BUILD_DIR}/target/release/sdproxy" ]; then
    cp "${BUILD_DIR}/target/release/sdproxy" "${PROXY_BIN}"
    chmod +x "${PROXY_BIN}"
    echo -e "${GREEN}✅ Binário instalado em ${PROXY_BIN}${NC}"
else
    echo -e "${RED}❌ Falha ao compilar! Verifique os logs acima.${NC}"
    exit 1
fi

# Copiar menu.sh
echo -e "${GREEN}Instalando menu...${NC}"
if [ -f "${BUILD_DIR}/menu.sh" ]; then
    cp "${BUILD_DIR}/menu.sh" "${SDPROXY}/menu.sh"
    chmod +x "${SDPROXY}/menu.sh"
    ln -sf "${SDPROXY}/menu.sh" /usr/local/bin/sdproxy
    echo -e "${GREEN}✅ Menu instalado. Use 'sdproxy' para executar.${NC}"
else
    echo -e "${YELLOW}⚠️ menu.sh não encontrado no repositório.${NC}"
fi

# Gerar certificados TLS
echo -e "${GREEN}Gerando certificados TLS...${NC}"
if [ ! -f "${CERT_DIR}/cert.pem" ] || [ ! -f "${CERT_DIR}/key.pem" ]; then
    openssl req -x509 -newkey rsa:2048 -keyout "${CERT_DIR}/key.pem" \
        -out "${CERT_DIR}/cert.pem" -days 365 -nodes \
        -subj "/CN=sdproxy/O=SDProxy/C=BR" 2>/dev/null
    echo -e "${GREEN}✅ Certificados gerados em ${CERT_DIR}${NC}"
else
    echo -e "${GREEN}✅ Certificados já existem${NC}"
fi

# Limpar build
rm -rf "${BUILD_DIR}"

# Verificar se já tem serviço rodando e parar
echo -e "${YELLOW}Verificando serviços existentes...${NC}"
for svc in /etc/systemd/system/proxy-*.service; do
    if [ -f "$svc" ]; then
        PORT=$(basename "$svc" .service | sed 's/proxy-//')
        echo -e "  ${YELLOW}Parando proxy-${PORT}...${NC}"
        systemctl stop "proxy-${PORT}.service" 2>/dev/null
        systemctl disable "proxy-${PORT}.service" 2>/dev/null
    fi
done

echo ""
echo -e "${GREEN}╔══════════════════════════════════╗${NC}"
echo -e "${GREEN}║     ✅ SDProxy v2.0 Instalado!   ║${NC}"
echo -e "${GREEN}╠══════════════════════════════════╣${NC}"
echo -e "${GREEN}║  Binário: ${PROXY_BIN}          ║${NC}"
echo -e "${GREEN}║  Certs:   ${CERT_DIR}/           ║${NC}"
echo -e "${GREEN}║  Menu:    sdproxy (qualquer dir)  ║${NC}"
echo -e "${GREEN}╚══════════════════════════════════╝${NC}"
echo ""
echo -e "${CYAN}Para iniciar:${NC}"
echo -e "  sdproxy           → Menu interativo"
echo -e "  ${YELLOW}[04] xHTTP SplitHTTP → Porta 443 exclusiva${NC}"
echo ""
echo -e "${CYAN}Flags do binário:${NC}"
echo -e "  -p PORTA       → Porta TCP"
echo -e "  -s STATUS      → Status HTTP (ex: @SDProxy)"
echo -e "  -t             → Habilitar TLS"
echo -e "  -ssh           → SSH only"
echo -e "  -u             → UDP"
echo -e "  -q             → QUIC"
echo -e "  -x             → xHTTP mode (porta 443, TLS, SSH only)"
echo ""
