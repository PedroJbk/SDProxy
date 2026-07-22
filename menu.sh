#!/bin/bash

# ============================================
# SDProxy Menu - Free v2.0
# Com suporte exclusivo xHTTP (SplitHTTP) porta 443
# Compatível com SocksRevive-XHTTP-DEMO
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
MAGENTA='\033[0;35m'
NC='\033[0m' # No Color

# ============================================
# Banner SDProxy
# ============================================
show_banner() {
    echo -e "\033[0;34m ███████╗██████╗ ██████╗ ██████╗  ██████╗ ██╗  ██╗██╗   ██╗"
    echo -e "\033[0;37m ██╔════╝██╔══██╗██╔══██╗██╔══██╗██╔═══██╗╚██╗██╔╝╚██╗ ██╔╝"
    echo -e "\033[0;34m ███████╗██║  ██║██████╔╝██████╔╝██║   ██║ ╚███╔╝  ╚████╔╝ "
    echo -e "\033[0;37m ╚════██║██║  ██║██╔═══╝ ██╔══██╗██║   ██║ ██╔██╗   ╚██╔╝  "
    echo -e "\033[0;34m ███████║██████╔╝██║     ██║  ██║╚██████╔╝██╔╝ ██╗   ██║   "
    echo -e "\033[0;37m ╚══════╝╚═════╝ ╚═╝     ╚═╝  ╚═╝ ╚═════╝ ╚═╝  ╚═╝   ╚═╝   "
    echo -e "\033[0;34m--------------------------------------------------------------\033[0m"
}

# ============================================
# Menu Principal
# ============================================
show_menu() {
    clear
    show_banner
    echo ""
    echo -e "${CYAN}╔══════════════════════════════════╗${NC}"
    echo -e "${CYAN}║       SDProxy Menu Free v2.0     ║${NC}"
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

# ============================================
# Mostrar portas ativas
# ============================================
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
# Abrir Porta (modo genérico)
# ============================================
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

    # Perguntar TLS
    echo ""
    echo -e "${YELLOW}NOTA: Para xHTTP/SplitHTTP na porta 443, use a opção [04] deste menu.${NC}"
    echo -e "${YELLOW}Esta opção é para outros protocolos (WebSocket, Socks5, TCP, etc.)${NC}"
    echo ""
    read -p "Habilitar TLS/SSL? (s/n): " HTTPS
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
    create_service "$PORT" "$HTTPS" "$STATUS" "$SSH_ONLY" "normal"

    # Iniciar serviço
    echo -e "${GREEN}Iniciando proxy na porta ${PORT}...${NC}"
    systemctl daemon-reload
    systemctl enable "proxy-${PORT}.service" 2>/dev/null
    systemctl start "proxy-${PORT}.service" 2>/dev/null

    sleep 2

    if systemctl is-active --quiet "proxy-${PORT}.service" 2>/dev/null; then
        echo -e "${GREEN}✅ Proxy iniciado na porta ${PORT}.${NC}"
        if [[ "$HTTPS" == "s" ]]; then
            echo -e "${GREEN}✅ TLS/SSL habilitado.${NC}"
            if [[ "$PORT" == "443" ]]; then
                echo -e "${GREEN}✅ UDP + QUIC ativados automaticamente.${NC}"
            fi
        fi
    else
        echo -e "${RED}❌ Falha ao iniciar o proxy na porta ${PORT}!${NC}"
        echo -e "${YELLOW}Verifique os logs: journalctl -u proxy-${PORT}.service${NC}"
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
    echo -e "${YELLOW}│  Protocolo xHTTP (SplitHTTP) REAL          │${NC}"
    echo -e "${YELLOW}│  - TLS obrigatório na porta 443            │${NC}"
    echo -e "${YELLOW}│  - GET /path/session-id → streaming        │${NC}"
    echo -e "${YELLOW}│  - POST /path/session-id/seq → uplink      │${NC}"
    echo -e "${YELLOW}│  - HTTP/2 com Transfer-Encoding: chunked   │${NC}"
    echo -e "${YELLOW}│  - Compatível SocksRevive-XHTTP-DEMO       │${NC}"
    echo -e "${YELLOW}└─────────────────────────────────────────────┘${NC}"
    echo ""

    # Port fixa 443
    PORT="443"

    # Verificar se já existe serviço ativo na 443
    if systemctl is-active --quiet "proxy-443.service" 2>/dev/null; then
        echo -e "${RED}❌ Porta 443 já está em uso!${NC}"
        echo -e "${YELLOW}Feche a porta existente antes de abrir xHTTP.${NC}"
        read -p "Enter pra continuar..."
        return
    fi

    # Criar diretório
    mkdir -p /opt/sdproxy

    # Verificar binário
    if [ ! -f "$SDPROXY" ]; then
        echo -e "${RED}❌ SDProxy não encontrado! Execute o install.sh primeiro.${NC}"
        read -p "Enter pra continuar..."
        return
    fi

    # Perguntar Status HTTP (cabeçalho HTTP que será enviado ao cliente)
    echo ""
    read -p "Status HTTP (Padrão: @SDProxy): " STATUS
    if [[ -z "$STATUS" ]]; then
        STATUS="@SDProxy"
    fi

    echo ""
    echo -e "${GREEN}Configuração xHTTP SplitHTTP:${NC}"
    echo -e "  Porta: ${YELLOW}${PORT}${NC}"
    echo -e "  TLS: ${GREEN}OBRIGATÓRIO${NC} (auto-ativado)"
    echo -e "  SSH Only: ${GREEN}SIM${NC}"
    echo -e "  UDP+QUIC: ${GREEN}Auto-ativados${NC}"
    echo -e "  Status: ${YELLOW}${STATUS}${NC}"
    echo ""

    # Criar certificado auto-assinado se não existir
    echo -e "${GREEN}Verificando certificados TLS...${NC}"
    if [ ! -f "/opt/sdproxy/cert.pem" ] || [ ! -f "/opt/sdproxy/key.pem" ]; then
        echo -e "${YELLOW}Gerando certificado auto-assinado...${NC}"
        mkdir -p /opt/sdproxy
        openssl req -x509 -newkey rsa:2048 -keyout /opt/sdproxy/key.pem \
            -out /opt/sdproxy/cert.pem -days 365 -nodes \
            -subj "/CN=sdproxy/O=SDProxy/C=BR" 2>/dev/null
        echo -e "${GREEN}✅ Certificados gerados.${NC}"
    else
        echo -e "${GREEN}✅ Certificados TLS existentes.${NC}"
    fi

    # Criar serviço xHTTP
    create_xhttp_service "$PORT" "$STATUS"

    # Iniciar serviço
    echo -e "${GREEN}Iniciando xHTTP SplitHTTP na porta ${PORT}...${NC}"
    systemctl daemon-reload
    systemctl enable "proxy-443.service" 2>/dev/null
    systemctl start "proxy-443.service" 2>/dev/null

    sleep 3

    if systemctl is-active --quiet "proxy-443.service" 2>/dev/null; then
        echo -e ""
        echo -e "${GREEN}╔══════════════════════════════════════════╗${NC}"
        echo -e "${GREEN}║  ✅ xHTTP SplitHTTP ATIVO NA PORTA 443  ║${NC}"
        echo -e "${GREEN}╠══════════════════════════════════════════╣${NC}"
        echo -e "${GREEN}║  Protocolo: xHTTP (SplitHTTP Real)       ║${NC}"
        echo -e "${GREEN}║  TLS/SSL: Habilitado (auto-assinado)     ║${NC}"
        echo -e "${GREEN}║  UDP: Habilitado                         ║${NC}"
        echo -e "${GREEN}║  QUIC: Habilitado                        ║${NC}"
        echo -e "${GREEN}║  Backend: SSH (127.0.0.1:22)            ║${NC}"
        echo -e "${GREEN}║  Fallback: VPN (127.0.0.1:1194)         ║${NC}"
        echo -e "${GREEN}╚══════════════════════════════════════════╝${NC}"
        echo ""
        echo -e "${YELLOW}Configuração para SocksRevive-XHTTP-DEMO:${NC}"
        echo -e "  Server: IP deste servidor"
        echo -e "  Port: 443"
        echo -e "  SNI: ${YELLOW}qualquer domínio (trust-all)${NC}"
        echo -e "  XHTTP Host: (deixe vazio ou IP do servidor)"
        echo -e "  XHTTP Path: /ssh (padrão)"
        echo -e "  XHTTP TLS: ${GREEN}HABILITADO${NC}"
        echo ""
        echo -e "${YELLOW}Para ver logs: journalctl -u proxy-443.service -f${NC}"
    else
        echo -e "${RED}❌ Falha ao iniciar o xHTTP na porta 443!${NC}"
        echo -e "${YELLOW}Verifique os logs: journalctl -u proxy-443.service -f${NC}"
        echo -e "${YELLOW}Verifique se SSH está rodando: systemctl status ssh${NC}"
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
    local MODE=$5
    local SERVICE_FILE="${SYSTEMD_DIR}/proxy-${PORT}.service"

    # Configurar argumentos
    EXTRA_ARGS="-p ${PORT}"

    # Adicionar status HTTP
    if [[ -n "$STATUS" ]]; then
        EXTRA_ARGS="${EXTRA_ARGS} -s ${STATUS}"
    fi

    # Configurar TLS
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

# ============================================
# Criar serviço xHTTP (SplitHTTP) exclusivo
# ============================================
create_xhttp_service() {
    local PORT=$1
    local STATUS=$2
    local SERVICE_FILE="${SYSTEMD_DIR}/proxy-443.service"

    # Garantir certificados
    mkdir -p /opt/sdproxy
    if [ ! -f "/opt/sdproxy/cert.pem" ]; then
        openssl req -x509 -newkey rsa:2048 -keyout /opt/sdproxy/key.pem \
            -out /opt/sdproxy/cert.pem -days 365 -nodes \
            -subj "/CN=sdproxy/O=SDProxy/C=BR" 2>/dev/null
    fi

    # Configuração xHTTP:
    # -p 443          → porta 443
    # -s @SDProxy     → status HTTP
    # -t              → TLS obrigatório (ativa UDP+QUIC automaticamente)
    # -ssh            → SSH only (tunnel SSH)
    EXTRA_ARGS="-p 443 -s ${STATUS} -t -ssh"

    cat > "$SERVICE_FILE" << EOF
[Unit]
Description=SDProxy xHTTP SplitHTTP - Porta 443
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
        echo -e "${RED}❌ Porta inválida!${NC}"
        read -p "Enter pra continuar..."
        return
    fi

    if systemctl is-active --quiet "proxy-${PORT}.service" 2>/dev/null; then
        echo -e "${YELLOW}Reiniciando proxy na porta ${PORT}...${NC}"
        systemctl restart "proxy-${PORT}.service"
        sleep 3
        
        if systemctl is-active --quiet "proxy-${PORT}.service" 2>/dev/null; then
            echo -e "${GREEN}✅ Proxy reiniciado na porta ${PORT}!${NC}"
        else
            echo -e "${RED}❌ Falha ao reiniciar proxy na porta ${PORT}!${NC}"
            echo -e "${YELLOW}Verifique: journalctl -u proxy-${PORT}.service${NC}"
        fi
    else
        echo -e "${RED}❌ Porta ${PORT} não está ativa!${NC}"
    fi

    echo ""
    read -p "Enter pra continuar..."
}

# ============================================
# Verificar estado do SSH
# ============================================
check_ssh() {
    clear
    show_banner
    echo ""
    echo -e "${CYAN}╔══════════════════════════════════╗${NC}"
    echo -e "${CYAN}║      Status SSH Local            ║${NC}"
    echo -e "${CYAN}╚══════════════════════════════════╝${NC}"
    echo ""
    
    if systemctl is-active --quiet ssh 2>/dev/null || systemctl is-active --quiet sshd 2>/dev/null; then
        echo -e "${GREEN}✅ SSH está rodando${NC}"
    else
        echo -e "${RED}❌ SSH NÃO está rodando!${NC}"
        echo -e "${YELLOW}Para xHTTP funcionar, o SSH precisa estar ativo.${NC}"
        echo -e "${YELLOW}Execute: sudo systemctl start ssh${NC}"
    fi

    echo ""
    
    # Mostrar portas ativas
    show_active_ports
    
    # Verificar VPN
    echo -e "${CYAN}Status VPN (OpenVPN):${NC}"
    if systemctl is-active --quiet openvpn@server 2>/dev/null; then
        echo -e "  ${GREEN}OpenVPN rodando (127.0.0.1:1194)${NC}"
    else
        echo -e "  ${YELLOW}OpenVPN não detectado (fallback não disponível)${NC}"
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
        05|5) check_ssh ;;
        00|0) echo -e "${GREEN}👋 Saindo...${NC}"; exit 0 ;;
        *) echo -e "${RED}❌ Opção inválida!${NC}"; sleep 1 ;;
    esac
done
