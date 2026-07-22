#!/bin/bash
# SDProxy Installer - Professional Version

REPO_URL="https://github.com/PedroJbk/SDProxy.git"
REPO_BRANCH="main"
CMD_NAME="sdproxy"
TOTAL_STEPS=7 # Adjusted total steps for a cleaner flow
CURRENT_STEP=0

# --- Cores e Estilos ---
GREEN="\033[0;32m"
BLUE="\033[0;34m"
RED="\033[0;31m"
NC="\033[0m" # No Color
BOLD="\033[1m"

# --- Funções de Feedback ---
log_info() {
    echo -e "${BLUE}${BOLD}[INFO]${NC} $1"
}

log_success() {
    echo -e "${GREEN}${BOLD}[SUCESSO]${NC} $1"
}

log_error() {
    echo -e "${RED}${BOLD}[ERRO]${NC} $1"
    exit 1
}

show_progress() {
    CURRENT_STEP=$((CURRENT_STEP + 1))
    PERCENT=$((CURRENT_STEP * 100 / TOTAL_STEPS))
    log_info "${PERCENT}% - $1"
}

# --- Verificação de Root ---
if [ "$EUID" -ne 0 ]; then
    log_error "Este script precisa ser executado como ROOT. Use 'sudo su' ou 'sudo bash install.sh'."
fi

clear
echo -e "${BLUE}${BOLD} ███████╗██████╗ ██████╗ ██████╗  ██████╗ ██╗  ██╗██╗   ██╗"
echo -e "${NC} ██╔════╝██╔══██╗██╔══██╗██╔══██╗██╔═══██╗╚██╗██╔╝╚██╗ ██╔╝"
echo -e "${BLUE}${BOLD} ███████╗██║  ██║██████╔╝██████╔╝██║   ██║ ╚███╔╝  ╚████╔╝ "
echo -e "${NC} ╚════██║██║  ██║██╔═══╝ ██╔══██╗██║   ██║ ██╔██╗   ╚██╔╝  "
echo -e "${BLUE}${BOLD} ███████║██████╔╝██║     ██║  ██║╚██████╔╝██╔╝ ██╗   ██║   "
echo -e "${NC} ╚══════╝╚═════╝ ╚═╝     ╚═╝  ╚═╝ ╚═════╝ ╚═╝  ╚═╝   ╚═╝   "
echo -e "${BLUE}${BOLD}--------------------------------------------------------------${NC}"
log_info "Iniciando instalação do SDProxy..."

# --- Etapa 1: Atualizar e Instalar Dependências ---
show_progress "Atualizando repositórios e instalando dependências essenciais..."
apt update -y > /dev/null 2>&1 || log_error "Falha ao atualizar repositórios."
apt install -y curl build-essential git lsb-release libssl-dev pkg-config > /dev/null 2>&1 || log_error "Falha ao instalar dependências. Verifique sua conexão com a internet."

# --- Etapa 2: Instalar Rust ---
show_progress "Verificando e instalando o Rust (pode levar alguns minutos)..."
if ! command -v cargo &> /dev/null; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y > /dev/null 2>&1
    # Source cargo env to make it available in the current shell
    source "$HOME/.cargo/env" || log_error "Falha ao configurar o ambiente Rust."
else
    log_info "Rust já está instalado. Pulando instalação."
    source "$HOME/.cargo/env" # Ensure env is sourced even if already installed
fi

# --- Etapa 3: Clonar Repositório ---
show_progress "Baixando o código fonte do SDProxy..."
rm -rf /root/SDProxy # Limpa instalações anteriores
git clone --branch "$REPO_BRANCH" "$REPO_URL" /root/SDProxy > /dev/null 2>&1 || log_error "Falha ao clonar o repositório. Verifique a URL ou sua conexão."
cd /root/SDProxy || log_error "Falha ao entrar no diretório do projeto."

# --- Etapa 4: Compilar o Projeto ---
show_progress "Compilando o SDProxy (isso pode levar 2-5 minutos, sem saída detalhada)..."
# Redireciona a saída de compilação para um arquivo temporário e stdout para /dev/null
# Apenas erros críticos serão exibidos
cargo build --release > /tmp/sdproxy_build.log 2>&1
if [ $? -ne 0 ]; then
    log_error "Falha na compilação do SDProxy. Verifique o log em /tmp/sdproxy_build.log para detalhes."
fi

# --- Etapa 5: Instalar Binários ---
show_progress "Instalando binários e configurando o sistema..."
mkdir -p /opt/sdproxy || log_error "Falha ao criar diretório /opt/sdproxy."
cp ./target/release/sdproxy /opt/sdproxy/proxy || log_error "Falha ao copiar binário do proxy."
chmod +x /opt/sdproxy/proxy || log_error "Falha ao dar permissão de execução ao proxy."

# Copia e configura o menu.sh se existir
if [ -f "menu.sh" ]; then
    cp menu.sh /opt/sdproxy/menu || log_error "Falha ao copiar script de menu."
    chmod +x /opt/sdproxy/menu || log_error "Falha ao dar permissão de execução ao menu."
    ln -sf /opt/sdproxy/menu /usr/local/bin/sdproxy || log_error "Falha ao criar link simbólico para o menu."
else
    ln -sf /opt/sdproxy/proxy /usr/local/bin/sdproxy || log_error "Falha ao criar link simbólico para o proxy."
fi

# --- Etapa 6: Limpar Arquivos Temporários ---
show_progress "Limpando arquivos temporários..."
rm -rf /root/SDProxy
rm -f /tmp/sdproxy_build.log

# --- Etapa 7: Finalização ---
log_success "Instalação do SDProxy concluída com sucesso!"
log_info "Para iniciar o proxy, digite: sdproxy"
log_info "Para desinstalar, digite: /opt/sdproxy/uninstall.sh (se existir)"
