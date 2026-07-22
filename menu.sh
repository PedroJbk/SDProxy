#!/bin/bash

# ============================================
# SDProxy Menu - Free
# ============================================

SDPROXY="/opt/sdproxy/proxy"
SYSTEMD_DIR="/etc/systemd/system"

# Cores
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
CYAN='\033[0;36m'
WHITE='\033[0;37m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Banner SDPROXY
show_banner() {
    echo -e "\033[0;34m ███████╗██████╗ ██████╗ ██████╗  ██████╗ ██╗  ██╗██╗   ██╗"
    echo -e "\033[0;37m ██╔════╝██╔══██╗██╔══██╗██╔══██╗██╔═══██╗╚██╗██╔╝╚██╗ ██╔╝"
    echo -e "\033[0;34m ███████╗██║  ██║██████╔╝██████╔╝██║   ██║ ╚███╔╝  ╚████╔╝ "
    echo -e "\033[0;37m ╚════██║██║  ██║██╔═══╝ ██╔══██╗██║   ██║ ██╔██╗   ╚██╔╝  "
    echo -e "\033[0;34m ███████║██████╔╝██║     ██║  ██║╚██████╔╝██╔╝ ██╗   ██║   "
    echo -e "\033[0;37m ╚══════╝╚═════╝ ╚═╝     ╚═╝  ╚═╝ ╚═════╝ ╚═╝  ╚═╝   ╚═╝   "
    echo -e "\033[0;34m--------------------------------------------------------------\033[0m"
}

show_menu() {
    clear
    show_banner
    echo ""
    echo -e "${CYAN}╔══════════════════════════════════╗${NC}"
    echo -e "${CYAN}║         SDProxy Menu Free        ║${NC}"
    echo -e "${CYAN}╠══════════════════════════════════╣${NC}"
    echo -e "${CYAN}║                                  ║${NC}"
    echo -e "${CYAN}║ ${WHITE}[01]${NC} - ABRIR PORTA               ${CYAN}║${NC}"
    echo -e "${CYAN}║ ${WHITE}[02]${NC} - FECHAR PORTA              ${CYAN}║${NC}"
    echo -e "${CYAN}║ ${WHITE}[03]${NC} - REINICIAR PORTA           ${CYAN}║${NC}"
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

open_port() {
    clear
    show_banner
    echo ""
    echo -e "${CYAN}╔══════════════════════════════════╗${NC}"
    echo -e "${CYAN}║         Abrir Porta               ║${NC}"
    echo -e "${CYAN}╚══════════════════════════════════╝${NC}"
    echo ""
    
    read -p "Porta: " PORT
    if [[ -z "$PORT" ]]; then
        echo -e "${RED}❌ Porta inválida!${NC}"
        read -p "Enter pra continuar..."
        return
    fi

    if [[ ! "$PORT" =~ ^[0-9]+$ ]] || [ "$PORT" -lt 1 ] || [ "$PORT" -gt 65535 ]; then
        echo -e "${RED}❌ Porta inválida! Use um número entre 1 e 65535.${NC}"
        read -p "Enter pra continuar..."
        return
    fi

    # Verificar se já existe serviço ativo
    if systemctl is-active --quiet "proxy-${PORT}.service" 2>/dev/null; then
        echo -e "${RED}❌ Porta ${PORT} já está em uso!${NC}"
        read -p "Enter pra continuar..."
        return
    fi

    # Perguntar HTTPS
    read -p "Habilitar o HTTPS? (s/n): " HTTPS
    HTTPS=$(echo "$HTTPS" | tr '[:upper:]' '[:lower:]')
    echo ""

    # Perguntar Status HTTP
    read -p "Status HTTP (Padrão: @SDProxy): " STATUS
    if [[ -z "$STATUS" ]]; then
        STATUS="@SDProxy"
    fi

    # Perguntar SSH apenas
    read -p "Habilitar somente SSH? (s/n): " SSH_ONLY
    SSH_ONLY=$(echo "$SSH_ONLY" | tr '[:upper:]' '[:lower:]')
    echo ""

    # Criar diretório se não existir
    mkdir -p /opt/sdproxy

    # Verificar se o binário existe
    if [ ! -f "$SDPROXY" ]; then
        echo -e "${RED}❌ SDProxy não encontrado! Execute o install.sh primeiro.${NC}"
        read -p "Enter pra continuar..."
        return
    fi

    # Criar arquivo de configuração do serviço
    create_service "$PORT" "$HTTPS" "$STATUS" "$SSH_ONLY"

    # Iniciar serviço
    echo -e "${GREEN}Iniciando proxy na porta ${PORT}...${NC}"
    systemctl daemon-reload
    systemctl enable "proxy-${PORT}.service" 2>/dev/null
    systemctl start "proxy-${PORT}.service" 2>/dev/null

    sleep 2

    if systemctl is-active --quiet "proxy-${PORT}.service" 2>/dev/null; then
        echo -e "${GREEN}Proxy iniciado na porta ${PORT}.${NC}"
    else
        echo -e "${RED}❌ Falha ao iniciar o proxy na porta ${PORT}!${NC}"
        echo -e "${YELLOW}Verifique os logs: journalctl -u proxy-${PORT}.service${NC}"
    fi

    echo ""
    read -p "Enter pra continuar..."
}

create_service() {
    local PORT=$1
    local HTTPS=$2
    local STATUS=$3
    local SSH_ONLY=$4
    local SERVICE_FILE="${SYSTEMD_DIR}/proxy-${PORT}.service"

    # Configurar argumentos extras
    EXTRA_ARGS="-p ${PORT}"

    # Adicionar status HTTP
    if [[ -n "$STATUS" ]]; then
        EXTRA_ARGS="${EXTRA_ARGS} -s ${STATUS}"
    fi

    # Configurar HTTPS
    if [[ "$HTTPS" == "s" ]]; then
        EXTRA_ARGS="${EXTRA_ARGS} -t"
    fi

    # Configurar SSH apenas
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
        echo -e "${RED}❌ Porta inválida!${NC}"
        read -p "Enter pra continuar..."
        return
    fi

    if systemctl is-active --quiet "proxy-${PORT}.service" 2>/dev/null; then
        systemctl stop "proxy-${PORT}.service"
        systemctl disable "proxy-${PORT}.service" 2>/dev/null
        rm -f "${SYSTEMD_DIR}/proxy-${PORT}.service"
        systemctl daemon-reload
        echo -e "${GREEN}✅ Porta ${PORT} fechada com sucesso!${NC}"
    else
        echo -e "${RED}❌ Porta ${PORT} não está ativa!${NC}"
    fi

    echo ""
    read -p "Enter pra continuar..."
}

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
        echo -e "${RED}❌ Porta inválida!${NC}"
        read -p "Enter pra continuar..."
        return
    fi

    if systemctl is-active --quiet "proxy-${PORT}.service" 2>/dev/null; then
        echo -e "${YELLOW}Reiniciando proxy na porta ${PORT}...${NC}"
        systemctl restart "proxy-${PORT}.service"
        sleep 2
        
        if systemctl is-active --quiet "proxy-${PORT}.service" 2>/dev/null; then
            echo -e "${GREEN}✅ Proxy reiniciado na porta ${PORT}!${NC}"
        else
            echo -e "${RED}❌ Falha ao reiniciar proxy na porta ${PORT}!${NC}"
        fi
    else
        echo -e "${RED}❌ Porta ${PORT} não está ativa!${NC}"
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
        00|0) echo -e "${GREEN}👋 Saindo...${NC}"; exit 0 ;;
        *) echo -e "${RED}❌ Opção inválida!${NC}"; sleep 1 ;;
    esac
done
