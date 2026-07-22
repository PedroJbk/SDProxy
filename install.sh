#!/bin/bash

# ============================================
# SDProxy Install Script v2.0
# Compila e instala o SDProxy com suporte xHTTP
# ============================================

set -e

SDPROXY="/opt/sdproxy"
PROXY_BIN="${SDPROXY}/proxy"
CERT_DIR="${SDPROXY}"
SERVICE_DIR="/etc/systemd/system"

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

# Verificar Rust
if ! command -v cargo &> /dev/null; then
    echo -e "${YELLOW}Instalando Rust...${NC}"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source $HOME/.cargo/env
fi

# Criar diretório
mkdir -p "${SDPROXY}"

# Verificar dependências
echo -e "${GREEN}Verificando dependências...${NC}"

# Verificar openssl
if ! command -v openssl &> /dev/null; then
    echo -e "${YELLOW}Instalando OpenSSL...${NC}"
    apt-get update && apt-get install -y openssl
fi

# Verificar systemd
if ! command -v systemctl &> /dev/null; then
    echo -e "${RED}❌ systemd não encontrado!${NC}"
    echo -e "${YELLOW}Este script requer um sistema com systemd.${NC}"
    exit 1
fi

# Verificar SSH
if ! command -v ssh &> /dev/null; then
    echo -e "${YELLOW}Instalando OpenSSH Server...${NC}"
    apt-get update && apt-get install -y openssh-server
    systemctl enable ssh
    systemctl start ssh
fi

# Compilar
echo -e "${GREEN}Compilando SDProxy...${NC}"
cd "$(dirname "$0")"

# Verificar se há Cargo.toml
if [ ! -f "Cargo.toml" ]; then
    echo -e "${RED}❌ Cargo.toml não encontrado!${NC}"
    exit 1
fi

# Build
cargo build --release 2>&1

# Copiar binário
if [ -f "target/release/sdproxy" ]; then
    cp target/release/sdproxy "${PROXY_BIN}"
    chmod +x "${PROXY_BIN}"
    echo -e "${GREEN}✅ Binário instalado em ${PROXY_BIN}${NC}"
else
    echo -e "${RED}❌ Falha ao compilar!${NC}"
    exit 1
fi

# Gerar certificados
echo -e "${GREEN}Gerando certificados TLS...${NC}"
if [ ! -f "${CERT_DIR}/cert.pem" ]; then
    openssl req -x509 -newkey rsa:2048 -keyout "${CERT_DIR}/key.pem" \
        -out "${CERT_DIR}/cert.pem" -days 365 -nodes \
        -subj "/CN=sdproxy/O=SDProxy/C=BR" 2>/dev/null
    echo -e "${GREEN}✅ Certificados gerados em ${CERT_DIR}${NC}"
else
    echo -e "${GREEN}✅ Certificados já existem${NC}"
fi

# Instalar menu.sh
echo -e "${GREEN}Instalando menu...${NC}"
if [ -f "menu.sh" ]; then
    cp menu.sh "${SDPROXY}/menu.sh"
    chmod +x "${SDPROXY}/menu.sh"
    
    # Criar symlink no /usr/local/bin
    ln -sf "${SDPROXY}/menu.sh" /usr/local/bin/sdproxy
    echo -e "${GREEN}✅ Menu instalado. Use 'sdproxy' para executar.${NC}"
else
    echo -e "${YELLOW}⚠️ menu.sh não encontrado, pulando instalação do menu.${NC}"
fi

echo ""
echo -e "${GREEN}╔══════════════════════════════════╗${NC}"
echo -e "${GREEN}║     ✅ SDProxy Instalado!        ║${NC}"
echo -e "${GREEN}╠══════════════════════════════════╣${NC}"
echo -e "${GREEN}║  Binário: ${PROXY_BIN}          ║${NC}"
echo -e "${GREEN}║  Certs:   ${CERT_DIR}/           ║${NC}"
echo -e "${GREEN}║  Menu:    sdproxy (qualquer dir)  ║${NC}"
echo -e "${GREEN}╚══════════════════════════════════╝${NC}"
echo ""
echo -e "${CYAN}Para iniciar:${NC}"
echo -e "  sdproxy           → Menu interativo"
echo -e "  ${YELLOW}[04] xHTTP SplitHTTP → Porta 443 exclusiva${NC}"
echo -e ""
echo -e "${CYAN}Flags do binário:${NC}"
echo -e "  -p PORTA       → Porta TCP"
echo -e "  -s STATUS      → Status HTTP (ex: @SDProxy)"
echo -e "  -t             → Habilitar TLS"
echo -e "  -ssh           → SSH only"
echo -e "  -u             → UDP"
echo -e "  -q             → QUIC"
echo -e "  -x             → xHTTP mode (porta 443, TLS, SSH only)"
echo ""
