#!/bin/bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# SDProxy - Multi-Protocol Proxy Installer
# Version: 2.1 Professional
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

set -e

# ═══════════════════════════════════════════════════════════════════
# CONFIGURAÇÃO
# ═══════════════════════════════════════════════════════════════════
SDPROXY="/opt/sdproxy"
PROXY_BIN="${SDPROXY}/proxy"
CERT_DIR="${SDPROXY}"
BUILD_DIR="/tmp/sdproxy_build_$$"
RAW="https://raw.githubusercontent.com/PedroJbk/SDProxy/main"

# ═══════════════════════════════════════════════════════════════════
# CORES & ESTILOS
# ═══════════════════════════════════════════════════════════════════
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
CYAN='\033[0;36m'
WHITE='\033[0;37m'
BOLD='\033[1m'
DIM='\033[2m'
NC='\033[0m'

# ═══════════════════════════════════════════════════════════════════
# FUNÇÕES DE UI
# ═══════════════════════════════════════════════════════════════════

banner() {
    clear
    echo -e "${CYAN}${BOLD}"
    echo "  ╔══════════════════════════════════════════════════════╗"
    echo "  ║                                                      ║"
    echo "  ║   ███████╗███████╗██████╗ ██╗      ██████╗ ██╗████████╗  ║"
    echo "  ║   ██╔════╝██╔════╝██╔══██╗██║     ██╔═══██╗██║╚══██╔══╝  ║"
    echo "  ║   ███████╗█████╗  ██████╔╝██║     ██║   ██║██║   ██║     ║"
    echo "  ║   ╚════██║██╔══╝  ██╔══██╗██║     ██║   ██║██║   ██║     ║"
    echo "  ║   ███████║███████╗██║  ██║███████╗╚██████╔╝██║   ██║     ║"
    echo "  ║   ╚══════╝╚══════╝╚═╝  ╚═╝╚══════╝ ╚═════╝ ╚═╝   ╚═╝     ║"
    echo "  ║                                                      ║"
    echo "  ║       P R O X Y                                       ║"
    echo "  ║                                                      ║"
    echo "  ║   ┌──────────────────────────────────────────────┐   ║"
    echo "  ║   │  Multi-Protocol • xHTTP • TLS • WebSocket   │   ║"
    echo "  ║   │  SSH Tunnel • SOCKS5 • QUIC • UDP          │   ║"
    echo "  ║   └──────────────────────────────────────────────┘   ║"
    echo "  ║                                                      ║"
    echo "  ║   Version 2.1  │  github.com/PedroJbk/SDProxy       ║"
    echo "  ║                                                      ║"
    echo "  ╚══════════════════════════════════════════════════════╝"
    echo -e "${NC}"
}

separator() {
    echo -e "${DIM}${CYAN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
}

step_header() {
    local num=$1
    local total=$2
    local title=$3
    echo -e ""
    echo -e "${BOLD}${GREEN}[${num}/${total}]${NC} ${BOLD}${CYAN}${title}${NC}"
    separator
}

progress_bar() {
    local percent=$1
    local bar_width=40
    local filled=$((percent * bar_width / 100))
    local empty=$((bar_width - filled))
    local bar=""
    
    for i in $(seq 1 $filled); do bar="${bar}█"; done
    for i in $(seq 1 $empty); do bar="${bar}░"; done
    
    echo -e "${GREEN}[${bar}]${NC} ${WHITE}${percent}%${NC}"
}

check_ok() {
    echo -e "  ${GREEN}✔${NC} $1"
}

check_err() {
    echo -e "  ${RED}✘${NC} $1"
}

check_warn() {
    echo -e "  ${YELLOW}⚠${NC} $1"
}

download_file() {
    local src=$1
    local dst=$2
    wget -q "${src}" -O "${dst}" 2>/dev/null
    local rc=$?
    if [ $rc -eq 0 ]; then
        return 0
    else
        return 1
    fi
}

# ═══════════════════════════════════════════════════════════════════
# BANNER INICIAL
# ═══════════════════════════════════════════════════════════════════
banner

# Sistema info
echo -e "  ${DIM}Sistema:${NC} ${WHITE}$(cat /etc/os-release 2>/dev/null | grep PRETTY_NAME | cut -d'"' -f2)${NC}"
echo -e "  ${DIM}Kernel:${NC}  ${WHITE}$(uname -r)${NC}"
echo -e "  ${DIM}Arq:${NC}     ${WHITE}$(uname -m)${NC}"
echo -e "  ${DIM}Data:${NC}    ${WHITE}$(date '+%d/%m/%Y %H:%M:%S')${NC}"
echo ""

# ═══════════════════════════════════════════════════════════════════
# ETAPA 1: DEPENDÊNCIAS
# ═══════════════════════════════════════════════════════════════════
step_header 1 5 "Instalando dependências"

echo -e "  ${YELLOW}Atualizando pacotes...${NC}"
apt-get update -qq >/dev/null 2>&1 || true

# Verificar e instalar cada dependência
DEPS=0

if ! command -v git &> /dev/null; then
    echo -e "  ${YELLOW}→ Instalando git...${NC}"
    apt-get install -y -qq git >/dev/null 2>&1 && check_ok "git instalado" || check_err "git falhou"
    DEPS=$((DEPS+1))
else
    check_ok "git já instalado ($(git --version | awk '{print $3}'))"
fi

if ! command -v gcc &> /dev/null; then
    echo -e "  ${YELLOW}→ Instalando build-essential...${NC}"
    apt-get install -y -qq build-essential >/dev/null 2>&1 && check_ok "build-essential instalado" || check_err "build-essential falhou"
    DEPS=$((DEPS+1))
else
    check_ok "gcc já instalado ($(gcc --version | head -1 | awk '{print $4}'))"
fi

if ! command -v openssl &> /dev/null; then
    echo -e "  ${YELLOW}→ Instalando openssl...${NC}"
    apt-get install -y -qq openssl >/dev/null 2>&1 && check_ok "openssl instalado" || check_err "openssl falhou"
    DEPS=$((DEPS+1))
else
    check_ok "openssl já instalado ($(openssl version | awk '{print $2}'))"
fi

if [ $DEPS -eq 0 ]; then
    echo -e "  ${GREEN}Todas as dependências já estão instaladas${NC}"
fi

# Rust
echo -e "  ${YELLOW}→ Verificando Rust/Cargo...${NC}"
if ! command -v cargo &> /dev/null; then
    echo -e "  ${YELLOW}→ Instalando Rust...${NC}"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y 2>/dev/null
    source "$HOME/.cargo/env" 2>/dev/null || true
    export PATH="$HOME/.cargo/bin:$PATH"
fi

if command -v cargo &> /dev/null; then
    check_ok "cargo $(cargo --version | awk '{print $2}')"
else
    check_err "cargo não encontrado no PATH"
    echo -e "  ${RED}Aborte. Execute: source \$HOME/.cargo/env${NC}"
    exit 1
fi

# SSH
if ! systemctl is-active --quiet ssh 2>/dev/null; then
    echo -e "  ${YELLOW}→ Ativando OpenSSH Server...${NC}"
    apt-get install -y -qq openssh-server >/dev/null 2>&1 || true
    systemctl enable ssh 2>/dev/null || true
    systemctl start ssh 2>/dev/null || true
    check_ok "SSH iniciado"
else
    check_ok "SSH já rodando"
fi

echo ""

# ═══════════════════════════════════════════════════════════════════
# ETAPA 2: DOWNLOAD
# ═══════════════════════════════════════════════════════════════════
step_header 2 5 "Baixando arquivos do GitHub"

rm -rf "${BUILD_DIR}"
mkdir -p "${BUILD_DIR}/src"

# Lista de arquivos para baixar
FILES=(
    "Cargo.toml:Cargo.toml"
    "Cargo.lock:Cargo.lock"
    "src/main.rs:src/main.rs"
    "src/xhttp.rs:src/xhttp.rs"
    "src/tls.rs:src/tls.rs"
    "src/protocol.rs:src/protocol.rs"
    "src/websocket.rs:src/websocket.rs"
    "src/security.rs:src/security.rs"
    "src/tcp_fallback.rs:src/tcp_fallback.rs"
    "src/ssh.rs:src/ssh.rs"
    "src/socks5.rs:src/socks5.rs"
    "src/udp.rs:src/udp.rs"
    "src/quic.rs:src/quic.rs"
)

TOTAL=${#FILES[@]}
DOWNLOADED=0
FAILED=0

for entry in "${FILES[@]}"; do
    SRC="${entry%%:*}"
    DST="${entry##*:}"
    
    download_file "${RAW}/${SRC}" "${BUILD_DIR}/${DST}"
    
    if [ $? -eq 0 ] && [ -f "${BUILD_DIR}/${DST}" ]; then
        check_ok "${DST}"
        DOWNLOADED=$((DOWNLOADED+1))
    else
        check_err "${DST} (falha)"
        FAILED=$((FAILED+1))
    fi
done

PERCENT=$((DOWNLOADED * 100 / TOTAL))
echo -e ""
echo -e "  ${DIM}Baixados: ${WHITE}${DOWNLOADED}/${TOTAL}${NC}  ${DIM}(${PERCENT}%)${NC}"
echo -e "  $(progress_bar $PERCENT)"

if [ $FAILED -gt 0 ]; then
    echo -e "  ${RED}${FAILED} arquivo(s) falharam no download${NC}"
    echo -e "  ${YELLOW}Verifique sua conexão com o GitHub${NC}"
    rm -rf "${BUILD_DIR}"
    exit 1
fi

RS_COUNT=$(find "${BUILD_DIR}/src" -name "*.rs" | wc -l)
check_ok "${RS_COUNT} módulos Rust prontos"

echo ""

# ═══════════════════════════════════════════════════════════════════
# ETAPA 3: COMPILAÇÃO
# ═══════════════════════════════════════════════════════════════════
step_header 3 5 "Compilando SDProxy (Rust Release)"

echo -e "  ${DIM}Isso pode levar alguns minutos na primeira vez...${NC}"
echo -e "  ${DIM}Compilando com otimizações (--release)...${NC}"
echo -e ""

cd "${BUILD_DIR}"

# Mostrar progresso de compilação
cargo build --release 2>&1 | while IFS= read -r line; do
    if echo "$line" | grep -q "^error"; then
        echo -e "  ${RED}${line}${NC}"
    elif echo "$line" | grep -q "Compiling"; then
        echo -e "  ${DIM}  ${CYAN}→${NC} ${line#*Compiling }${NC}"
    elif echo "$line" | grep -q "Finished"; then
        echo -e "  ${GREEN}${line}${NC}"
    fi
done

if [ -f "${BUILD_DIR}/target/release/sdproxy" ]; then
    echo ""
    check_ok "Binário gerado com sucesso"
    BINARY_SIZE=$(du -h "${BUILD_DIR}/target/release/sdproxy" | awk '{print $1}')
    echo -e "  ${DIM}Tamanho: ${WHITE}${BINARY_SIZE}${NC}"
else
    echo ""
    check_err "Compilação falhou"
    rm -rf "${BUILD_DIR}"
    exit 1
fi

echo ""

# ═══════════════════════════════════════════════════════════════════
# ETAPA 4: INSTALAÇÃO
# ═══════════════════════════════════════════════════════════════════
step_header 4 5 "Instalando no sistema"

mkdir -p "${SDPROXY}"

# Binário
cp "${BUILD_DIR}/target/release/sdproxy" "${PROXY_BIN}"
chmod +x "${PROXY_BIN}"
check_ok "Binário em ${PROXY_BIN}"

# Menu
download_file "${RAW}/menu.sh" "${SDPROXY}/menu.sh"
if [ -f "${SDPROXY}/menu.sh" ]; then
    chmod +x "${SDPROXY}/menu.sh"
    ln -sf "${SDPROXY}/menu.sh" /usr/local/bin/sdproxy
    check_ok "Menu instalado (comando: sdproxy)"
else
    check_warn "menu.sh não baixado"
fi

# Certificados TLS
echo -e "  ${YELLOW}→ Certificados TLS...${NC}"
if [ ! -f "${CERT_DIR}/cert.pem" ] || [ ! -f "${CERT_DIR}/key.pem" ]; then
    openssl req -x509 -newkey rsa:2048 -keyout "${CERT_DIR}/key.pem" \
        -out "${CERT_DIR}/cert.pem" -days 365 -nodes \
        -subj "/CN=sdproxy/O=SDProxy/C=BR" 2>/dev/null
    check_ok "Certificados gerados (${CERT_DIR}/)"
else
    check_ok "Certificados existentes"
fi

echo ""

# ═══════════════════════════════════════════════════════════════════
# ETAPA 5: LIMPEZA & RESUMO
# ═══════════════════════════════════════════════════════════════════
step_header 5 5 "Finalizando"

# Limpar build
rm -rf "${BUILD_DIR}"
check_ok "Arquivos temporários removidos"

# Parar serviços antigos
STOPPED=0
for svc in /etc/systemd/system/proxy-*.service; do
    if [ -f "$svc" ]; then
        PORT=$(basename "$svc" .service | sed 's/proxy-//')
        systemctl stop "proxy-${PORT}.service" 2>/dev/null || true
        systemctl disable "proxy-${PORT}.service" 2>/dev/null || true
        STOPPED=$((STOPPED+1))
    fi
done
[ $STOPPED -gt 0 ] && check_ok "${STOPPED} serviço(s) antigo(s) parados"

# ═══════════════════════════════════════════════════════════════════
# BANNER FINAL
# ═══════════════════════════════════════════════════════════════════
clear
echo -e "${CYAN}${BOLD}"
echo "  ╔══════════════════════════════════════════════════════╗"
echo "  ║                                                      ║"
echo "  ║   ███████╗███████╗██████╗ ██║  ██║                   ║"
echo "  ║   ██╔════╝██╔════╝██╔══██╗██║  ██║                   ║"
echo "  ║   ███████╗█████╗  ██████╔╝███████║                   ║"
echo "  ║   ╚════██║██╔══╝  ██╔══██╗╚════██║                   ║"
echo "  ║   ███████║███████╗██║  ██║     ██║                   ║"
echo "  ║   ╚══════╝╚══════╝╚═╝  ╚═╝     ╚═╝                   ║"
echo "  ║                                                      ║"
echo "  ║              I N S T A L L E D                       ║"
echo "  ║                                                      ║"
echo "  ╚══════════════════════════════════════════════════════╝"
echo -e "${NC}"

separator
echo -e ""
echo -e "  ${BOLD}${GREEN}✔ Instalação concluída com sucesso!${NC}"
echo -e ""
separator
echo -e ""
echo -e "  ${DIM}Binário:${NC}    ${WHITE}${PROXY_BIN}${NC}"
echo -e "  ${DIM}Menu:${NC}       ${WHITE}sdproxy${NC}  ${DIM}(comando global)${NC}"
echo -e "  ${DIM}Certificados:${NC} ${WHITE}${CERT_DIR}/${NC}"
echo -e ""
separator
echo -e ""
echo -e "  ${BOLD}${CYAN}Protocolos suportados:${NC}"
echo -e "  ${WHITE}├──${NC} SSH Tunnel"
echo -e "  ${WHITE}├──${NC} WebSocket"
echo -e "  ${WHITE}├──${NC} xHTTP / SplitHTTP ${YELLOW}(porta 443)${NC}"
echo -e "  ${WHITE}├──${NC} TLS/SSL"
echo -e "  ${WHITE}├──${NC} SOCKS5"
echo -e "  ${WHITE}├──${NC} QUIC"
echo -e "  ${WHITE}└──${NC} UDP"
echo -e ""
separator
echo -e ""
echo -e "  ${BOLD}${GREEN}Para iniciar:${NC}"
echo -e "  ${WHITE}$ sdproxy${NC}"
echo -e ""
echo -e "  ${DIM}[01] Abrir Porta          [04] xHTTP SplitHTTP (443)${NC}"
echo -e "  ${DIM}[02] Fechar Porta         [00] Sair${NC}"
echo -e ""
echo -e "  ${CYAN}github.com/PedroJbk/SDProxy${NC}"
echo -e ""
