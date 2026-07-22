#!/bin/bash
# AWProxy Installer
REPO_URL="https://github.com/PedroJbk/AWProxy.git"
REPO_BRANCH="main"
CMD_NAME="awproxy"
TOTAL_STEPS=9
CURRENT_STEP=0

show_progress() {
    PERCENT=$((CURRENT_STEP * 100 / TOTAL_STEPS))
    echo "Progresso: [${PERCENT}%] - $1"
}

error_exit() {
    echo -e "\nErro: $1"
    exit 1
}

increment_step() {
    CURRENT_STEP=$((CURRENT_STEP + 1))
}

if [ "$EUID" -ne 0 ]; then
    error_exit "EXECUTE COMO ROOT"
else
    clear

    # Banner AWPROXY
    echo -e "\033[0;34m   ██████╗ ███████╗██████╗ ██████╗  ██████╗ ██╗  ██╗██╗   ██╗"
    echo -e "\033[0;37m   ██╔══██╗██╔════╝██╔══██╗██╔══██╗██╔═══██╗╚██╗██╔╝╚██╗ ██╔╝"
    echo -e "\033[0;34m   ██████╔╝███████╗██████╔╝██████╔╝██║   ██║ ╚███╔╝  ╚████╔╝ "
    echo -e "\033[0;37m   ██╔══██╗╚════██║██╔═══╝ ██╔══██╗██║   ██║ ██╔██╗   ╚██╔╝  "
    echo -e "\033[0;34m   ██████╔╝███████║██║     ██║  ██║╚██████╔╝██╔╝ ██╗   ██║   "
    echo -e "\033[0;37m   ╚═════╝ ╚══════╝╚═╝     ╚═╝  ╚═╝ ╚═════╝ ╚═╝  ╚═╝   ╚═╝   "
    echo -e "\033[0;34m--------------------------------------------------------------\033[0m"

    show_progress "Atualizando repositorios..."
    export DEBIAN_FRONTEND=noninteractive
    apt update -y > /dev/null 2>&1 || error_exit "Falha ao atualizar os repositorios"
    increment_step

    show_progress "Verificando o sistema..."
    if ! command -v lsb_release &> /dev/null; then
        apt install lsb-release -y > /dev/null 2>&1 || error_exit "Falha ao instalar lsb-release"
    fi
    increment_step

    OS_NAME=$(lsb_release -is)
    VERSION=$(lsb_release -rs)
    case $OS_NAME in
        Ubuntu)
            case $VERSION in
                24.*|22.*|20.*|18.*) show_progress "Sistema Ubuntu suportado, continuando..." ;;
                *) error_exit "Versão do Ubuntu não suportada. Use 18, 20, 22 ou 24." ;;
            esac
            ;;
        Debian)
            case $VERSION in
                12*|11*|10*|9*) show_progress "Sistema Debian suportado, continuando..." ;;
                *) error_exit "Versão do Debian não suportada. Use 9, 10, 11 ou 12." ;;
            esac
            ;;
        *) error_exit "Sistema não suportado. Use Ubuntu ou Debian." ;;
    esac
    increment_step

    show_progress "Atualizando o sistema..."
    apt upgrade -y > /dev/null 2>&1 || error_exit "Falha ao atualizar o sistema"
    apt-get install curl build-essential git -y > /dev/null 2>&1 || error_exit "Falha ao instalar pacotes"
    increment_step

    show_progress "Criando diretorio /opt/awproxy..."
    mkdir -p /opt/awproxy > /dev/null 2>&1
    increment_step

    show_progress "Instalando Rust..."
    if ! command -v rustc &> /dev/null; then
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y > /dev/null 2>&1 || error_exit "Falha ao instalar Rust"
        source "$HOME/.cargo/env"
    fi
    increment_step

    show_progress "Compilando AWProxy, isso pode levar algum tempo..."
    if [ -d "/root/AWProxy" ]; then
        rm -rf /root/AWProxy
    fi
    git clone --branch "$REPO_BRANCH" "$REPO_URL" /root/AWProxy > /dev/null 2>&1 || error_exit "Falha ao clonar AWProxy"

    if [ -f /root/AWProxy/menu.sh ]; then
        cp /root/AWProxy/menu.sh /opt/awproxy/menu
        chmod +x /opt/awproxy/menu
    fi

    cd /root/AWProxy || error_exit "Diretório do AWProxy não encontrado"
    cargo build --release --jobs "$(nproc)" > /dev/null 2>&1 || error_exit "Falha ao compilar AWProxy"

    if [ -f ./target/release/awproxy ]; then
        mv ./target/release/awproxy /opt/awproxy/proxy || error_exit "Binário compilado não encontrado"
        chmod +x /opt/awproxy/proxy
    else
        error_exit "Binário 'awproxy' não encontrado após compilação"
    fi
    increment_step

    show_progress "Configurando permissões..."
    chmod +x /opt/awproxy/proxy
    [ -f /opt/awproxy/menu ] && chmod +x /opt/awproxy/menu

    if [ -f /opt/awproxy/menu ]; then
        cp /opt/awproxy/menu /usr/local/bin/awproxy
    else
        cp /opt/awproxy/proxy /usr/local/bin/awproxy
    fi
    chmod +x /usr/local/bin/awproxy
    increment_step

    show_progress "Limpando diretórios temporários..."
    cd /root/
    rm -rf /root/AWProxy/
    increment_step

    echo ""
    echo -e "\033[0;32m✅ Instalação concluída com sucesso!\033[0m"
    echo ""
    echo "🚀 Digite 'awproxy' para acessar o menu."
    echo "   Ou 'awproxy -p 80' para abrir porta 80 diretamente."
    echo ""
    echo "📡 Protocolos suportados:"
    echo "   - SOCKS5 (byte 0x05)"
    echo "   - TLS/SECURITY (byte 0x16)"
    echo "   - WebSocket (GET / ou HTTP/)"
    echo "   - SECURITY (AUTH ou SECURITY)"
    echo "   - TCP Fallback (qualquer outro)"
    echo ""
fi
