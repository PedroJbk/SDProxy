#!/bin/bash

# ============================================
# SDProxy Menu - Free v2.2
# ============================================

SDPROXY="/opt/sdproxy/proxy"
SDPROXY_XHTTP="/opt/sdproxy/proxy-xhttp"
SYSTEMD_DIR="/etc/systemd/system"

# Cores
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
CYAN='\033[0;36m'
WHITE='\033[0;37m'
BLUE='\033[0;34m'
MAGENTA='\033[0;35m'
NC='\033[0m'

# ============================================
# Banner SDPROXY
# ============================================
show_banner() {
    echo -e "${BLUE}${BOLD} ███████╗██████╗ ██████╗ ██████╗  ██████╗ ██╗  ██╗██╗   ██╗"
    echo -e "${NC} ██╔════╝██╔══██╗██╔══██╗██╔══██╗██╔═══██╗╚██╗██╔╝╚██╗ ██╔╝"
    echo -e "${BLUE}${BOLD} ███████╗██║  ██║██████╔╝██████╔╝██║   ██║ ╚███╔╝  ╚████╔╝ "
    echo -e "${NC} ╚════██║██║  ██║██╔═══╝ ██╔══██╗██║   ██║ ██╔██╗   ╚██╔╝  "
    echo -e "${BLUE}${BOLD} ███████║██████╔╝██║     ██║  ██║╚██████╔╝██╔╝ ██╗   ██║   "
    echo -e "${NC} ╚══════╝╚═════╝ ╚═╝     ╚═╝  ╚═╝ ╚═════╝ ╚═╝  ╚═╝   ╚═╝   "
    echo -e "${BLUE}${BOLD}--------------------------------------------------------------${NC}"
}

# ============================================
# Menu Principal
# ============================================
show_menu() {
    clear
    show_banner
    echo ""
    echo -e "${CYAN}╔══════════════════════════════════╗${NC}"
    echo -e "${CYAN}║       SDProxy Menu Free v2.2     ║${NC}"
    echo -e "${CYAN}╠══════════════════════════════════╣${NC}"
    echo -e "${CYAN}║                                  ║${NC}"
    echo -e "${CYAN}║ ${WHITE}[01]${NC} - ABRIR PORTA               ${CYAN}║${NC}"
    echo -e "${CYAN}║ ${WHITE}[02]${NC} - FECHAR PORTA              ${CYAN}║${NC}"
    echo -e "${CYAN}║ ${WHITE}[03]${NC} - REINICIAR PORTA           ${CYAN}║${NC}"
    echo -e "${CYAN}║ ${MAGENTA}[04]${NC} - xHTTP SPLITHTTP (${GREEN}443${NC})  ${CYAN}║${NC}"
    echo -e "${CYAN}║                                  ║${NC}"
    echo -e "${CYAN}║ ${WHITE}[00]${NC} - SAIR                      ${CYAN}║${NC}"
    echo -e "${CYAN}║                                  ║${NC}"
    echo -e "${CYAN}╚══════════════════════════════════╝${NC}"
    echo ""
    echo -n "Escolha uma opção: "
}

show_active_ports() {
    ACTIVE=""
    for service_file in ${SYSTEMD_DIR}/proxy-*.service; do
        if [ -f "$service_file" ]; then
            PORT=$(basename "$service_file" .service | sed 's/proxy-//')
            if systemctl is-active --quiet "proxy-${PORT}.service" 2>/dev/null; then
                ACTIVE="$ACTIVE $PORT"
            fi
        fi
    done
    if [ -n "$ACTIVE" ]; then
        echo -e "Porta(s) ativa(s):${YELLOW}${ACTIVE}${NC}"
    else
        echo -e "Porta(s) ativa(s):${RED} nenhuma${NC}"
    fi
    echo ""
}

# ============================================
# Abrir Porta (padrão - 80, 8080, etc)
# ============================================
open_port() {
    clear
    show_banner
    echo ""
    echo -e "${CYAN}╔══════════════════════════════════╗${NC}"
    echo -e "${CYAN}║         Abrir Porta               ║${NC}"
    echo -e "${CYAN}╚══════════════════════════════════╝${NC}"
    echo ""

    # Se a porta for 443, redirecionar para xHTTP
    read -p "Porta: " PORT
    if [[ -z "$PORT" ]]; then
        echo -e "${RED}Porta inválida!${NC}"
        read -p "Enter pra continuar..."
        return
    fi

    if [[ "$PORT" == "443" ]]; then
        echo -e "${YELLOW}Para porta 443, use a opção [04] xHTTP SplitHTTP.${NC}"
        read -p "Enter pra continuar..."
        return
    fi

    if [[ ! "$PORT" =~ ^[0-9]+$ ]] || [ "$PORT" -lt 1 ] || [ "$PORT" -gt 65535 ]; then
        echo -e "${RED}Porta inválida!${NC}"
        read -p "Enter pra continuar..."
        return
    fi

    if systemctl is-active --quiet "proxy-${PORT}.service" 2>/dev/null; then
        echo -e "${RED}Porta ${PORT} já está em uso!${NC}"
        read -p "Enter pra continuar..."
        return
    fi

    read -p "Habilitar o HTTPS? (s/n): " HTTPS
    HTTPS=$(echo "$HTTPS" | tr '[:upper:]' '[:lower:]')
    echo ""

    read -p "Status HTTP (Padrão: @SDProxy): " STATUS
    if [[ -z "$STATUS" ]]; then
        STATUS="@SDProxy"
    fi

    read -p "Habilitar somente SSH? (s/n): " SSH_ONLY
    SSH_ONLY=$(echo "$SSH_ONLY" | tr '[:upper:]' '[:lower:]')
    echo ""

    mkdir -p /opt/sdproxy

    if [ ! -f "$SDPROXY" ]; then
        echo -e "${RED}SDProxy não encontrado! Execute o install.sh primeiro.${NC}"
        read -p "Enter pra continuar..."
        return
    fi

    create_service "$PORT" "$HTTPS" "$STATUS" "$SSH_ONLY"

    echo -e "${GREEN}Iniciando proxy na porta ${PORT}...${NC}"
    systemctl daemon-reload
    systemctl enable "proxy-${PORT}.service" 2>/dev/null
    systemctl start "proxy-${PORT}.service" 2>/dev/null

    sleep 2

    if systemctl is-active --quiet "proxy-${PORT}.service" 2>/dev/null; then
        echo -e "${GREEN}Proxy iniciado na porta ${PORT}.${NC}"
    else
        echo -e "${RED}Falha ao iniciar o proxy na porta ${PORT}!${NC}"
        echo -e "${YELLOW}Verifique: journalctl -u proxy-${PORT}.service${NC}"
    fi

    echo ""
    read -p "Enter pra continuar..."
}

# ============================================
# xHTTP SplitHTTP - Opção Exclusiva Porta 443
# ============================================
open_xhttp() {
    clear
    show_banner
    echo ""
    echo -e "${CYAN}╔══════════════════════════════════╗${NC}"
    echo -e "${CYAN}║  xHTTP SplitHTTP - Porta 443     ║${NC}"
    echo -e "${CYAN}║  (SocksRevive-XHTTP-DEMO)       ║${NC}"
    echo -e "${CYAN}╚══════════════════════════════════╝${NC}"
    echo ""
    echo -e "${YELLOW}┌─────────────────────────────────────────────┐${NC}"
    echo -e "${YELLOW}│  Protocolo xHTTP (SplitHTTP)               │${NC}"
    echo -e "${YELLOW}│  - TLS obrigatório na porta 443            │${NC}"
    echo -e "${YELLOW}│  - Compatível SocksRevive-XHTTP-DEMO      │${NC}"
    echo -e "${YELLOW}│  - GET = downlink (streaming)              │${NC}"
    echo -e "${YELLOW}│  - POST = uplink (dados SSH)               │${NC}"
    echo -e "${YELLOW}└─────────────────────────────────────────────┘${NC}"
    echo ""

    PORT="443"

    if systemctl is-active --quiet "proxy-443.service" 2>/dev/null; then
        echo -e "${RED}Porta 443 já está em uso!${NC}"
        read -p "Enter pra continuar..."
        return
    fi

    mkdir -p /opt/sdproxy

    if [ ! -f "$SDPROXY_XHTTP" ]; then
        echo -e "${RED}sdproxy-xhttp não encontrado! Execute o install.sh primeiro.${NC}"
        read -p "Enter pra continuar..."
        return
    fi

    read -p "Status HTTP (Padrão: @SDProxy): " STATUS
    if [[ -z "$STATUS" ]]; then
        STATUS="@SDProxy"
    fi

    # Gerar certificados se não existirem
    echo -e "${GREEN}Verificando certificados TLS...${NC}"
    if [ ! -f "/opt/sdproxy/cert.pem" ] || [ ! -f "/opt/sdproxy/key.pem" ]; then
        echo -e "${YELLOW}Gerando certificado auto-assinado...${NC}"
        openssl req -x509 -newkey rsa:2048 -keyout /opt/sdproxy/key.pem \
            -out /opt/sdproxy/cert.pem -days 365 -nodes \
            -subj "/CN=sdproxy/O=SDProxy/C=BR" 2>/dev/null
        echo -e "${GREEN}Certificados gerados.${NC}"
    else
        echo -e "${GREEN}Certificados TLS existentes.${NC}"
    fi

    echo ""
    echo -e "${GREEN}Configuração xHTTP SplitHTTP:${NC}"
    echo -e "  Porta: ${YELLOW}${PORT}${NC}"
    echo -e "  TLS: ${GREEN}OBRIGATÓRIO (auto-assinado)${NC}"
    echo -e "  SSH Only: ${GREEN}SIM${NC}"
    echo -e "  Status: ${YELLOW}${STATUS}${NC}"
    echo ""

    # Criar serviço xHTTP
    create_xhttp_service "$PORT" "$STATUS"

    echo -e "${GREEN}Iniciando xHTTP SplitHTTP na porta ${PORT}...${NC}"
    systemctl daemon-reload
    systemctl enable "proxy-443.service" 2>/dev/null
    systemctl start "proxy-443.service" 2>/dev/null

    sleep 3

    if systemctl is-active --quiet "proxy-443.service" 2>/dev/null; then
        echo -e "${GREEN}╔══════════════════════════════════════════╗${NC}"
        echo -e "${GREEN}║  xHTTP SplitHTTP ATIVO NA PORTA 443     ║${NC}"
        echo -e "${GREEN}╚══════════════════════════════════════════╝${NC}"
        echo ""
        echo -e "${YELLOW}Configuração SocksRevive-XHTTP-DEMO:${NC}"
        echo -e "  Server: IP deste servidor"
        echo -e "  Port: 443"
        echo -e "  SNI: qualquer domínio (trust-all)"
        echo -e "  XHTTP Path: /ssh"
        echo -e "  XHTTP TLS: HABILITADO"
        echo ""
        echo -e "${YELLOW}Logs: journalctl -u proxy-443.service -f${NC}"
    else
        echo -e "${RED}Falha ao iniciar xHTTP na porta 443!${NC}"
        echo -e "${YELLOW}Logs: journalctl -u proxy-443.service -f${NC}"
    fi

    echo ""
    read -p "Enter pra continuar..."
}

# ============================================
# Criar serviço padrão
# ============================================
create_service() {
    local PORT=$1
    local HTTPS=$2
    local STATUS=$3
    local SSH_ONLY=$4
    local SERVICE_FILE="${SYSTEMD_DIR}/proxy-${PORT}.service"

    EXTRA_ARGS="-p ${PORT}"

    if [[ -n "$STATUS" ]]; then
        EXTRA_ARGS="${EXTRA_ARGS} -s ${STATUS}"
    fi

    if [[ "$HTTPS" == "s" ]]; then
        EXTRA_ARGS="${EXTRA_ARGS} -t"
    fi

    if [[ "$SSH_ONLY" == "s" ]]; then
        EXTRA_ARGS="${EXTRA_ARGS} -ssh"
    fi

    cat > "$SERVICE_FILE" << EOF
[Unit]
Description=SDProxy - Porta ${PORT}
After=network.target

[Service]
Type=simple
ExecStart=${SDPROXY} ${EXTRA_ARGS}
Restart=on-failure
RestartSec=5
User=root
WorkingDirectory=/opt/sdproxy

[Install]
WantedBy=multi-user.target
EOF
}

# ============================================
# Criar serviço xHTTP (SplitHTTP) exclusivo
# ============================================
create_xhttp_service() {
    local PORT=$1
    local STATUS=$2
    local SERVICE_FILE="${SYSTEMD_DIR}/proxy-443.service"

    EXTRA_ARGS="-p 443 -s ${STATUS}"

    cat > "$SERVICE_FILE" << EOF
[Unit]
Description=SDProxy xHTTP SplitHTTP - Porta 443
After=network.target

[Service]
Type=simple
ExecStart=${SDPROXY_XHTTP} ${EXTRA_ARGS}
Restart=on-failure
RestartSec=5
User=root
WorkingDirectory=/opt/sdproxy

[Install]
WantedBy=multi-user.target
EOF
}

# ============================================
# Fechar Porta
# ============================================
close_port() {
    clear
    show_banner
    echo ""
    echo -e "${CYAN}╔══════════════════════════════════╗${NC}"
    echo -e "${CYAN}║         Fechar Porta              ║${NC}"
    echo -e "${CYAN}╚══════════════════════════════════╝${NC}"
    echo ""

    show_active_ports

    read -p "Porta: " PORT
    if [[ -z "$PORT" ]]; then
        echo -e "${RED}Porta inválida!${NC}"
        read -p "Enter pra continuar..."
        return
    fi

    if systemctl is-active --quiet "proxy-${PORT}.service" 2>/dev/null; then
        systemctl stop "proxy-${PORT}.service"
        systemctl disable "proxy-${PORT}.service" 2>/dev/null
        rm -f "${SYSTEMD_DIR}/proxy-${PORT}.service"
        systemctl daemon-reload
        echo -e "${GREEN}Porta ${PORT} fechada com sucesso!${NC}"
    else
        echo -e "${RED}Porta ${PORT} não está ativa!${NC}"
    fi

    echo ""
    read -p "Enter pra continuar..."
}

# ============================================
# Reiniciar Porta
# ============================================
restart_port() {
    clear
    show_banner
    echo ""
    echo -e "${CYAN}╔══════════════════════════════════╗${NC}"
    echo -e "${CYAN}║        Reiniciar Porta            ║${NC}"
    echo -e "${CYAN}╚══════════════════════════════════╝${NC}"
    echo ""

    show_active_ports

    read -p "Porta: " PORT
    if [[ -z "$PORT" ]]; then
        echo -e "${RED}Porta inválida!${NC}"
        read -p "Enter pra continuar..."
        return
    fi

    if systemctl is-active --quiet "proxy-${PORT}.service" 2>/dev/null; then
        echo -e "${YELLOW}Reiniciando proxy na porta ${PORT}...${NC}"
        systemctl restart "proxy-${PORT}.service"
        sleep 2

        if systemctl is-active --quiet "proxy-${PORT}.service" 2>/dev/null; then
            echo -e "${GREEN}Proxy reiniciado na porta ${PORT}!${NC}"
        else
            echo -e "${RED}Falha ao reiniciar proxy na porta ${PORT}!${NC}"
        fi
    else
        echo -e "${RED}Porta ${PORT} não está ativa!${NC}"
    fi

    echo ""
    read -p "Enter pra continuar..."
}

# ============================================
# Loop Principal
# ============================================

while true; do
    show_menu
    show_active_ports
    read OPTION
    case $OPTION in
        01|1) open_port ;;
        02|2) close_port ;;
        03|3) restart_port ;;
        04|4) open_xhttp ;;
        00|0) echo -e "${GREEN}Saindo...${NC}"; exit 0 ;;
        *) echo -e "${RED}Opção inválida!${NC}"; sleep 1 ;;
    esac
done
