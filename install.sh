#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────────
#  SoundTime — One-Click Installer
#  https://github.com/CICCADA-CORP/SoundTime
#
#  Usage:
#    curl -fsSL https://raw.githubusercontent.com/CICCADA-CORP/SoundTime/main/install.sh | bash
#    or:
#    wget -qO- https://raw.githubusercontent.com/CICCADA-CORP/SoundTime/main/install.sh | bash
#
#  What it does:
#    1. Checks prerequisites (Docker, Docker Compose, git)
#    2. Clones the repository (or updates if already present)
#    3. Generates a secure .env with random secrets
#    4. Pulls multi-arch Docker images (amd64 / arm64)
#    5. Starts all services
# ──────────────────────────────────────────────────────────────────
set -euo pipefail

# ── Colors & helpers ─────────────────────────────────────────────
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m' # No Color

info()    { printf "${BLUE}[INFO]${NC}    %s\n" "$*"; }
success() { printf "${GREEN}[OK]${NC}      %s\n" "$*"; }
warn()    { printf "${YELLOW}[WARN]${NC}    %s\n" "$*"; }
error()   { printf "${RED}[ERROR]${NC}   %s\n" "$*" >&2; }
fatal()   { error "$*"; exit 1; }

# ── Banner ───────────────────────────────────────────────────────
banner() {
  printf "\n${CYAN}${BOLD}"
  cat << 'EOF'
   ____                        _ _____ _
  / ___|  ___  _   _ _ __   __| |_   _(_)_ __ ___   ___
  \___ \ / _ \| | | | '_ \ / _` | | | | | '_ ` _ \ / _ \
   ___) | (_) | |_| | | | | (_| | | | | | | | | | |  __/
  |____/ \___/ \__,_|_| |_|\__,_| |_| |_|_| |_| |_|\___|

EOF
  printf "${NC}"
  printf "  ${BOLD}Self-hosted music streaming with P2P sharing${NC}\n"
  printf "  ${CYAN}https://github.com/CICCADA-CORP/SoundTime${NC}\n\n"
}

# ── Prerequisite checks ─────────────────────────────────────────
check_command() {
  if ! command -v "$1" &>/dev/null; then
    return 1
  fi
  return 0
}

check_prerequisites() {
  info "Checking prerequisites..."

  local missing=()

  # Docker
  if check_command docker; then
    local docker_version
    docker_version=$(docker --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+' | head -1)
    success "Docker ${docker_version} found"
  else
    missing+=("docker")
  fi

  # Docker Compose (v2 plugin or standalone)
  if docker compose version &>/dev/null 2>&1; then
    local compose_version
    compose_version=$(docker compose version --short 2>/dev/null || echo "v2+")
    success "Docker Compose ${compose_version} found"
  elif check_command docker-compose; then
    warn "Legacy docker-compose found — consider upgrading to Docker Compose V2"
    success "Docker Compose (legacy) found"
  else
    missing+=("docker-compose")
  fi

  # Git
  if check_command git; then
    success "git found"
  else
    missing+=("git")
  fi

  # curl or wget (for potential future use)
  if check_command curl || check_command wget; then
    success "HTTP client found"
  fi

  if [ ${#missing[@]} -gt 0 ]; then
    echo ""
    error "Missing required tools: ${missing[*]}"
    echo ""
    printf "  Install them first:\n"
    case "$(uname -s)" in
      Linux*)
        printf "    ${BOLD}Ubuntu/Debian:${NC}  sudo apt update && sudo apt install -y docker.io docker-compose-v2 git\n"
        printf "    ${BOLD}Fedora/RHEL:${NC}    sudo dnf install -y docker docker-compose git\n"
        printf "    ${BOLD}Arch:${NC}           sudo pacman -S docker docker-compose git\n"
        ;;
      Darwin*)
        printf "    ${BOLD}macOS:${NC}          brew install --cask docker && brew install git\n"
        printf "                  (or install Docker Desktop from https://docker.com/products/docker-desktop)\n"
        ;;
      *)
        printf "    See https://docs.docker.com/get-docker/ for installation instructions.\n"
        ;;
    esac
    echo ""
    fatal "Please install the missing tools and re-run this script."
  fi

  # Docker daemon running?
  if ! docker info &>/dev/null 2>&1; then
    echo ""
    error "Docker daemon is not running."
    case "$(uname -s)" in
      Linux*)  printf "  Start it with: ${BOLD}sudo systemctl start docker${NC}\n" ;;
      Darwin*) printf "  Open ${BOLD}Docker Desktop${NC} and wait for it to start.\n" ;;
    esac
    fatal "Start Docker and re-run this script."
  fi
  success "Docker daemon is running"
}

# ── Install directory ────────────────────────────────────────────
INSTALL_DIR="${SOUNDTIME_INSTALL_DIR:-$HOME/soundtime}"
REPO_URL="https://github.com/CICCADA-CORP/SoundTime.git"

setup_directory() {
  if [ -d "$INSTALL_DIR/.git" ]; then
    info "Existing installation found at ${INSTALL_DIR}"
    info "Pulling latest changes..."
    cd "$INSTALL_DIR"
    git pull --ff-only origin main 2>/dev/null || warn "Could not fast-forward — using existing version"
    success "Repository updated"
  else
    info "Cloning SoundTime to ${INSTALL_DIR}..."
    git clone "$REPO_URL" "$INSTALL_DIR"
    success "Repository cloned"
  fi
  cd "$INSTALL_DIR"
}

# ── Generate secure .env ─────────────────────────────────────────
generate_secret() {
  # Generate a random 32-byte base64 secret
  if check_command openssl; then
    openssl rand -base64 32
  elif [ -r /dev/urandom ]; then
    head -c 32 /dev/urandom | base64 | tr -d '\n'
  else
    # Fallback: use date + PID (less secure, but works everywhere)
    echo "soundtime-$(date +%s)-$$-$(od -An -tx4 -N8 /dev/random 2>/dev/null | tr -d ' ')" | head -c 44
  fi
}

setup_env() {
  if [ -f .env ]; then
    info "Existing .env file found — keeping your configuration"
    success ".env preserved"
    return
  fi

  info "Generating secure .env configuration..."

  local jwt_secret
  jwt_secret=$(generate_secret)
  local postgres_password
  postgres_password=$(generate_secret | tr -dc 'a-zA-Z0-9' | head -c 24)

  cat > .env << ENVFILE
# ──────────────────────────────────────────────────────────
#  SoundTime — Configuration
#  Generated by install.sh on $(date -u +"%Y-%m-%d %H:%M:%S UTC")
# ──────────────────────────────────────────────────────────

# ─── Database ───
POSTGRES_USER=soundtime
POSTGRES_PASSWORD=${postgres_password}
POSTGRES_DB=soundtime

# ─── Security ───
# Auto-generated secret — do NOT share this
JWT_SECRET=${jwt_secret}

# ─── Application ───
SOUNDTIME_DOMAIN=localhost
RUST_LOG=info

# ─── Storage ───
STORAGE_BACKEND=local
AUDIO_STORAGE_PATH=/data/music

# ─── Networking ───
NGINX_PORT=8880
P2P_ENABLED=true
P2P_BIND_PORT=11204
P2P_LOCAL_DISCOVERY=true

# ─── Optional: S3 storage ───
# STORAGE_BACKEND=s3
# S3_ENDPOINT=
# S3_REGION=us-east-1
# S3_ACCESS_KEY=
# S3_SECRET_KEY=
# S3_BUCKET=
ENVFILE

  success ".env generated with secure random secrets"
}

# ── Pull & start ─────────────────────────────────────────────────
start_services() {
  info "Pulling Docker images (this may take a few minutes on first run)..."
  docker compose pull 2>/dev/null || docker-compose pull
  success "Images pulled"

  info "Starting SoundTime..."
  docker compose up -d 2>/dev/null || docker-compose up -d
  success "All services started"
}

# ── Wait for healthy ─────────────────────────────────────────────
wait_for_healthy() {
  info "Waiting for services to become healthy..."

  local max_wait=60
  local waited=0

  while [ $waited -lt $max_wait ]; do
    if curl -sf http://localhost:8080/api/health &>/dev/null 2>&1; then
      success "Backend is healthy"
      return
    fi
    sleep 2
    waited=$((waited + 2))
    printf "."
  done
  echo ""
  warn "Backend did not respond within ${max_wait}s — it may still be starting"
  warn "Check logs with: docker compose logs -f backend"
}

# ── Detect accessible URL ────────────────────────────────────────
detect_url() {
  local port
  port=$(grep -E '^NGINX_PORT=' .env 2>/dev/null | cut -d= -f2 || echo "8880")
  port="${port:-8880}"

  # Try to detect LAN IP for easier access from other devices
  local ip="localhost"
  case "$(uname -s)" in
    Linux*)
      ip=$(hostname -I 2>/dev/null | awk '{print $1}')
      ip="${ip:-localhost}"
      ;;
    Darwin*)
      ip=$(ipconfig getifaddr en0 2>/dev/null || echo "localhost")
      ;;
  esac

  echo ""
  printf "${GREEN}${BOLD}╔══════════════════════════════════════════════════════════╗${NC}\n"
  printf "${GREEN}${BOLD}║                                                          ║${NC}\n"
  printf "${GREEN}${BOLD}║   🎵  SoundTime is running!                              ║${NC}\n"
  printf "${GREEN}${BOLD}║                                                          ║${NC}\n"
  printf "${GREEN}${BOLD}╚══════════════════════════════════════════════════════════╝${NC}\n"
  echo ""
  printf "  ${BOLD}Open in your browser:${NC}\n"
  echo ""
  printf "    ${CYAN}➜  Local:${NC}     http://localhost:${port}\n"
  if [ "$ip" != "localhost" ]; then
    printf "    ${CYAN}➜  Network:${NC}   http://${ip}:${port}\n"
  fi
  echo ""
  printf "  ${BOLD}What's next:${NC}\n"
  printf "    1. Open the URL above and create your account\n"
  printf "    2. The first user automatically becomes ${BOLD}admin${NC}\n"
  printf "    3. Upload your music and enjoy! 🎶\n"
  echo ""
  printf "  ${BOLD}Useful commands:${NC}\n"
  printf "    ${CYAN}cd ${INSTALL_DIR}${NC}\n"
  printf "    docker compose logs -f          ${YELLOW}# View logs${NC}\n"
  printf "    docker compose stop             ${YELLOW}# Stop services${NC}\n"
  printf "    docker compose up -d            ${YELLOW}# Start services${NC}\n"
  printf "    docker compose pull && docker compose up -d  ${YELLOW}# Update${NC}\n"
  echo ""
}

# ── Uninstall hint ───────────────────────────────────────────────
uninstall_hint() {
  printf "  ${BOLD}To uninstall:${NC}\n"
  printf "    cd ${INSTALL_DIR} && docker compose down -v && cd ~ && rm -rf ${INSTALL_DIR}\n"
  echo ""
}

# ── Main ─────────────────────────────────────────────────────────
main() {
  banner
  check_prerequisites
  echo ""
  setup_directory
  echo ""
  setup_env
  echo ""
  start_services
  echo ""
  wait_for_healthy
  detect_url
  uninstall_hint
}

main "$@"
