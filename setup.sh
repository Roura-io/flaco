#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────
# flacoAi installer — interactive setup for macOS & Linux
# Supports: fresh install, update, and full reinstall
# ─────────────────────────────────────────────────────────────

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
VERSION="0.1.0"
FLACO_STATE_DIR="$HOME/.flaco"
FLACO_STATE_FILE="$FLACO_STATE_DIR/install.state"

# ── Dev mode detection ─────────────────────────────────────
# Run with --dev to install as flacoai-dev (for developers)
# Otherwise installs as flacoai (stable user build)

DEV_MODE=false
BIN_NAME="flacoai"

for arg in "$@"; do
    case "$arg" in
        --dev) DEV_MODE=true; BIN_NAME="flacoai-dev" ;;
    esac
done

# ── Colors & formatting ────────────────────────────────────

BOLD='\033[1m'
DIM='\033[2m'
UNDERLINE='\033[4m'
RED='\033[31m'
GREEN='\033[32m'
YELLOW='\033[33m'
BLUE='\033[34m'
MAGENTA='\033[35m'
CYAN='\033[36m'
WHITE='\033[37m'
RESET='\033[0m'

# ── Helpers ─────────────────────────────────────────────────

banner() {
    echo ""
    echo -e "${MAGENTA}${BOLD}"
    echo "███████╗██╗      █████╗  ██████╗ ██████╗      █████╗ ██╗"
    echo "██╔════╝██║     ██╔══██╗██╔════╝██╔═══██╗    ██╔══██╗██║"
    echo "█████╗  ██║     ███████║██║     ██║   ██║    ███████║██║"
    echo "██╔══╝  ██║     ██╔══██║██║     ██║   ██║    ██╔══██║██║"
    echo "██║     ███████╗██║  ██║╚██████╗╚██████╔╝    ██║  ██║██║"
    echo "╚═╝     ╚══════╝╚═╝  ╚═╝ ╚═════╝ ╚═════╝     ╚═╝  ╚═╝╚═╝"
    echo -e "${RESET}"
    if $DEV_MODE; then
        echo -e "${DIM}Local AI coding agent powered by Roura.io${RESET}  ${CYAN}v${VERSION}${RESET}  ${YELLOW}${BOLD}[DEV]${RESET}"
    else
        echo -e "${DIM}Local AI coding agent powered by Roura.io${RESET}  ${CYAN}v${VERSION}${RESET}"
    fi
    echo ""
}

step()  { echo -e "\n${BLUE}${BOLD}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${RESET}"; echo -e "${CYAN}${BOLD}📦 $1${RESET}"; echo -e "${BLUE}${BOLD}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${RESET}"; }
ok()    { echo -e "${GREEN}✅ $1${RESET}"; }
warn()  { echo -e "${YELLOW}⚠️  $1${RESET}"; }
fail()  { echo -e "${RED}❌ $1${RESET}"; }
info()  { echo -e "${DIM}$1${RESET}"; }
hint()  { echo -e "${MAGENTA}💡 $1${RESET}"; }

ask_yn() {
    local prompt="$1"
    local default="${2:-y}"
    local yn_hint
    if [[ "$default" == "y" ]]; then yn_hint="[Y/n]"; else yn_hint="[y/N]"; fi
    echo -ne "${YELLOW}❓ ${prompt} ${DIM}${yn_hint}${RESET} "
    read -r answer
    answer="${answer:-$default}"
    case "$answer" in
        [Yy]*) return 0 ;;
        *)     return 1 ;;
    esac
}

ask_input() {
    local prompt="$1"
    local default="$2"
    echo -ne "${YELLOW}✏️  ${prompt} ${DIM}[${default}]${RESET} "
    read -r answer
    echo "${answer:-$default}"
}

ask_choice() {
    local prompt="$1"
    shift
    local options=("$@")
    echo ""
    echo -e "${WHITE}${BOLD}${prompt}${RESET}"
    echo ""
    for i in "${!options[@]}"; do
        echo -e "  ${CYAN}$((i+1)).${RESET} ${options[$i]}"
    done
    echo ""
    echo -ne "${YELLOW}👉 Enter choice ${DIM}[1-${#options[@]}]${RESET} "
    read -r choice
    echo "${choice:-1}"
}

detect_shell_profile() {
    local shell_name
    shell_name="$(basename "${SHELL:-/bin/bash}")"
    case "$shell_name" in
        zsh)  echo "$HOME/.zshrc" ;;
        bash)
            if [[ -f "$HOME/.bashrc" ]]; then echo "$HOME/.bashrc"
            else echo "$HOME/.bash_profile"; fi
            ;;
        fish) echo "$HOME/.config/fish/config.fish" ;;
        *)    echo "$HOME/.profile" ;;
    esac
}

detect_os() {
    case "$(uname -s)" in
        Darwin) echo "macos" ;;
        Linux)  echo "linux" ;;
        *)      echo "unknown" ;;
    esac
}

save_state() {
    mkdir -p "$FLACO_STATE_DIR"
    cat > "$FLACO_STATE_FILE" <<STATEEOF
FLACO_INSTALLED_VERSION=${VERSION}
FLACO_INSTALLED_DATE=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
FLACO_INSTALLED_MODEL=${CHOSEN_MODEL:-}
FLACO_INSTALL_MODE=${INSTALL_MODE}
STATEEOF
}

load_state() {
    if [[ -f "$FLACO_STATE_FILE" ]]; then
        # shellcheck source=/dev/null
        source "$FLACO_STATE_FILE"
        return 0
    fi
    return 1
}

# ── Track what happened for the summary ─────────────────────

RUST_STATUS="skipped"
OLLAMA_STATUS="skipped"
BUILD_STATUS="skipped"
MODEL_STATUS="skipped"
ENV_STATUS="skipped"
SMOKE_STATUS="skipped"
CHOSEN_MODEL=""
SHELL_PROFILE=""
INSTALL_MODE="fresh"  # fresh | update | reinstall

# ── Detect install state ────────────────────────────────────

IS_EXISTING_INSTALL=false
PREV_VERSION=""
PREV_MODEL=""

if load_state 2>/dev/null; then
    IS_EXISTING_INSTALL=true
    PREV_VERSION="${FLACO_INSTALLED_VERSION:-unknown}"
    PREV_MODEL="${FLACO_INSTALLED_MODEL:-}"
fi

# ── Start ───────────────────────────────────────────────────

clear 2>/dev/null || true
banner

OS="$(detect_os)"
SHELL_PROFILE="$(detect_shell_profile)"

if [[ "$IS_EXISTING_INSTALL" == true ]]; then
    # ── Returning user ──────────────────────────────────
    echo -e "${GREEN}${BOLD}Welcome back!${RESET}  ${DIM}flacoAi ${PREV_VERSION} was previously installed.${RESET}"
    if [[ -n "$PREV_MODEL" ]]; then
        echo -e "${DIM}Current model: ${PREV_MODEL}${RESET}"
    fi
    echo ""

    MODE_CHOICE=$(ask_choice "What would you like to do?" \
        "🔄  Update — rebuild flacoAi & pull latest model (keeps config)" \
        "🧹  Fresh Install — full setup from scratch (like first time)" \
        "🛠️   Customize — pick which steps to run" \
        "❌  Cancel")

    case "$MODE_CHOICE" in
        1) INSTALL_MODE="update" ;;
        2) INSTALL_MODE="fresh" ;;
        3) INSTALL_MODE="custom" ;;
        *)
            echo ""
            echo -e "${DIM}No changes made. See you next time! 👋${RESET}"
            echo ""
            exit 0
            ;;
    esac
else
    # ── First time ──────────────────────────────────────
    echo -e "${WHITE}${BOLD}Welcome to the flacoAi installer!${RESET}"
    echo ""
    echo -e "This script will set up everything you need:"
    echo ""
    echo -e "  ${CYAN}1.${RESET} 🦀 Install Rust (if needed)"
    echo -e "  ${CYAN}2.${RESET} 🦙 Install Ollama (if needed)"
    echo -e "  ${CYAN}3.${RESET} 🔨 Build & install the ${BOLD}${BIN_NAME}${RESET} CLI"
    echo -e "  ${CYAN}4.${RESET} 🧠 Pull an AI model for local inference"
    echo -e "  ${CYAN}5.${RESET} ⚙️  Configure your shell environment"
    echo ""
    echo -e "${DIM}Detected: ${OS} · shell: $(basename "${SHELL:-bash}") · profile: ${SHELL_PROFILE}${RESET}"
    echo ""

    if ! ask_yn "Ready to proceed with installation?" "y"; then
        echo ""
        echo -e "${DIM}No worries! Run this script again when you're ready.${RESET}"
        echo -e "${DIM}  ./setup.sh${RESET}"
        echo ""
        exit 0
    fi

    INSTALL_MODE="fresh"
fi

# ── Decide which steps to run ───────────────────────────────

DO_RUST=true
DO_OLLAMA=true
DO_BUILD=true
DO_MODEL=true
DO_SHELLCFG=true

if [[ "$INSTALL_MODE" == "update" ]]; then
    DO_RUST=false       # assume already installed
    DO_OLLAMA=false     # assume already installed
    DO_BUILD=true       # always rebuild
    DO_MODEL=true       # offer to update model
    DO_SHELLCFG=false   # already configured
elif [[ "$INSTALL_MODE" == "custom" ]]; then
    echo ""
    echo -e "${WHITE}${BOLD}Select which steps to run:${RESET}"
    echo ""
    ask_yn "  🦀 Check / install Rust?" "n"   && DO_RUST=true   || DO_RUST=false
    ask_yn "  🦙 Check / install Ollama?" "n"  && DO_OLLAMA=true || DO_OLLAMA=false
    ask_yn "  🔨 Rebuild & install ${BIN_NAME}?" "y" && DO_BUILD=true  || DO_BUILD=false
    ask_yn "  🧠 Pull / change AI model?" "n"  && DO_MODEL=true  || DO_MODEL=false
    ask_yn "  ⚙️  Configure shell profile?" "n" && DO_SHELLCFG=true || DO_SHELLCFG=false
fi

TOTAL_STEPS=0
CURRENT_STEP=0
$DO_RUST     && ((TOTAL_STEPS++)) || true
$DO_OLLAMA   && ((TOTAL_STEPS++)) || true
$DO_BUILD    && ((TOTAL_STEPS++)) || true
$DO_MODEL    && ((TOTAL_STEPS++)) || true
$DO_SHELLCFG && ((TOTAL_STEPS++)) || true

next_step() { ((CURRENT_STEP++)) || true; }

# ─────────────────────────────────────────────────────────────
# Step: Rust
# ─────────────────────────────────────────────────────────────

if $DO_RUST; then
    next_step
    step "Step ${CURRENT_STEP}/${TOTAL_STEPS} — 🦀 Rust Toolchain"

    if command -v cargo &>/dev/null; then
        ok "Rust is already installed: $(cargo --version)"
        RUST_STATUS="already installed"
    else
        warn "Rust is not installed on this system."
        echo ""
        if ask_yn "Install Rust via rustup? (required to build flacoAi)" "y"; then
            echo ""
            info "Downloading and running rustup installer..."
            echo ""
            if curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y 2>&1; then
                if [[ -f "$HOME/.cargo/env" ]]; then
                    # shellcheck source=/dev/null
                    source "$HOME/.cargo/env"
                fi
                ok "Rust installed successfully: $(cargo --version)"
                RUST_STATUS="installed"
            else
                fail "Rust installation failed."
                echo ""
                hint "Try installing manually:"
                echo -e "  ${CYAN}curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh${RESET}"
                echo -e "${DIM}Then re-run this script.${RESET}"
                exit 1
            fi
        else
            fail "Rust is required to build flacoAi. Cannot continue without it."
            echo ""
            hint "Install Rust manually when you're ready:"
            echo -e "  ${CYAN}curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh${RESET}"
            exit 1
        fi
    fi
else
    RUST_STATUS="skipped"
fi

# ─────────────────────────────────────────────────────────────
# Step: Ollama
# ─────────────────────────────────────────────────────────────

if $DO_OLLAMA; then
    next_step
    step "Step ${CURRENT_STEP}/${TOTAL_STEPS} — 🦙 Ollama Runtime"

    if command -v ollama &>/dev/null; then
        ok "Ollama is already installed: $(ollama --version 2>/dev/null || echo 'found')"
        OLLAMA_STATUS="already installed"
    else
        warn "Ollama is not installed on this system."
        echo ""
        if ask_yn "Install Ollama now?" "y"; then
            echo ""
            install_ollama_curl() {
                info "Installing Ollama via install script..."
                if curl -fsSL https://ollama.com/install.sh | sh 2>&1; then
                    ok "Ollama installed"
                    OLLAMA_STATUS="installed"
                    return 0
                fi
                return 1
            }

            if [[ "$OS" == "macos" ]] && command -v brew &>/dev/null; then
                info "Installing Ollama via Homebrew..."
                if brew install ollama 2>&1; then
                    ok "Ollama installed via Homebrew"
                    OLLAMA_STATUS="installed (brew)"
                else
                    warn "Homebrew install failed, trying curl..."
                    if ! install_ollama_curl; then
                        fail "Ollama installation failed."
                        hint "Install manually from: ${UNDERLINE}https://ollama.com${RESET}"
                        OLLAMA_STATUS="failed"
                    fi
                fi
            else
                if ! install_ollama_curl; then
                    fail "Ollama installation failed."
                    hint "Download manually from: ${UNDERLINE}https://ollama.com${RESET}"
                    OLLAMA_STATUS="failed"
                fi
            fi
        else
            echo ""
            warn "Skipping Ollama installation."
            hint "You'll need Ollama to run models locally."
            echo -e "  ${CYAN}${UNDERLINE}https://ollama.com${RESET}"
            OLLAMA_STATUS="skipped"
        fi
    fi
else
    OLLAMA_STATUS="skipped"
fi

# ─────────────────────────────────────────────────────────────
# Step: Build & Install flaco
# ─────────────────────────────────────────────────────────────

if $DO_BUILD; then
    next_step
    step "Step ${CURRENT_STEP}/${TOTAL_STEPS} — 🔨 Build & Install ${BIN_NAME}"

    if [[ "$INSTALL_MODE" == "update" ]]; then
        info "Updating ${BIN_NAME} to v${VERSION}..."
    else
        info "Building in release mode — this may take a few minutes on first build..."
    fi
    echo ""

    cd "$SCRIPT_DIR/rust"

    if cargo build --release -p flaco-cli 2>&1; then
        echo ""
        ok "Build succeeded"

        info "Installing ${BIN_NAME} to ~/.cargo/bin..."
        if cargo install --path crates/flaco-cli --bin "$BIN_NAME" --force 2>&1; then
            FLACO_BIN="${HOME}/.cargo/bin/${BIN_NAME}"
            ok "Installed to ${FLACO_BIN}"
            BUILD_STATUS="installed (v${VERSION})"
        else
            fail "cargo install failed."
            hint "The binary was built. You can copy it manually:"
            echo -e "  ${CYAN}cp ${SCRIPT_DIR}/rust/target/release/${BIN_NAME} ~/.cargo/bin/${RESET}"
            BUILD_STATUS="build ok, install failed"
        fi
    else
        fail "Build failed!"
        echo ""
        hint "Common fixes:"
        echo -e "  ${DIM}• macOS: ${CYAN}xcode-select --install${RESET}"
        echo -e "  ${DIM}• Linux: ${CYAN}sudo apt install build-essential${RESET}"
        echo -e "  ${DIM}• Update Rust: ${CYAN}rustup update${RESET}"
        echo ""
        echo -e "${DIM}Then re-run this script.${RESET}"
        BUILD_STATUS="failed"
        exit 1
    fi

    cd "$SCRIPT_DIR"

    # Ensure ~/.cargo/bin is on PATH for remaining steps
    if ! command -v "$BIN_NAME" &>/dev/null; then
        export PATH="$HOME/.cargo/bin:$PATH"
    fi
else
    BUILD_STATUS="skipped"
fi

# ─────────────────────────────────────────────────────────────
# Step: Pull AI model
# ─────────────────────────────────────────────────────────────

if $DO_MODEL; then
    next_step
    step "Step ${CURRENT_STEP}/${TOTAL_STEPS} — 🧠 AI Model"

    if ! command -v ollama &>/dev/null; then
        warn "Ollama is not available — skipping model pull."
        hint "After installing Ollama, run: ${CYAN}ollama pull qwen3:30b-a3b${RESET}"
        MODEL_STATUS="skipped (no ollama)"
    else
        echo ""
        echo -e "${WHITE}Popular models for flacoAi:${RESET}"
        echo ""
        echo -e "  ${BOLD}qwen3:30b-a3b${RESET}     ${DIM}— 30B MoE, fast & great for coding ${GREEN}(recommended)${RESET}"
        echo -e "  ${BOLD}qwen3:8b${RESET}          ${DIM}— 8B params, lighter weight${RESET}"
        echo -e "  ${BOLD}deepseek-coder-v2${RESET} ${DIM}— specialized for code${RESET}"
        echo -e "  ${BOLD}llama3.1:8b${RESET}       ${DIM}— Meta's versatile model${RESET}"
        echo -e "  ${BOLD}codellama:13b${RESET}     ${DIM}— Meta's code-focused model${RESET}"
        echo ""

        DEFAULT_MODEL="${PREV_MODEL:-qwen3:30b-a3b}"
        CHOSEN_MODEL="$(ask_input "Which model?" "$DEFAULT_MODEL")"
        echo ""

        # Start ollama serve if not already running
        if ! curl -sf http://localhost:11434/api/tags &>/dev/null; then
            info "Starting Ollama server..."
            ollama serve &>/dev/null &
            sleep 2
        fi

        info "Pulling ${CHOSEN_MODEL} — this may take a while depending on your connection..."
        echo ""

        if ollama pull "$CHOSEN_MODEL" 2>&1; then
            echo ""
            ok "Model ${CHOSEN_MODEL} is ready"
            MODEL_STATUS="pulled"
        else
            echo ""
            fail "Failed to pull ${CHOSEN_MODEL}."
            hint "You can pull it manually later:"
            echo -e "  ${CYAN}ollama pull ${CHOSEN_MODEL}${RESET}"
            MODEL_STATUS="failed"
        fi
    fi
else
    MODEL_STATUS="skipped"
fi

# ─────────────────────────────────────────────────────────────
# Step: Shell configuration
# ─────────────────────────────────────────────────────────────

if $DO_SHELLCFG; then
    next_step
    step "Step ${CURRENT_STEP}/${TOTAL_STEPS} — ⚙️  Shell Configuration"

    # PATH
    if ! grep -q '.cargo/bin' "$SHELL_PROFILE" 2>/dev/null; then
        if ask_yn "Add ~/.cargo/bin to your PATH in ${SHELL_PROFILE}?" "y"; then
            echo '' >> "$SHELL_PROFILE"
            echo '# Added by flacoAi installer' >> "$SHELL_PROFILE"
            echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> "$SHELL_PROFILE"
            ok "Added ~/.cargo/bin to PATH in ${SHELL_PROFILE}"
            ENV_STATUS="configured"
        else
            warn "Skipped PATH configuration."
            hint "Make sure ~/.cargo/bin is in your PATH to use ${BIN_NAME}."
            ENV_STATUS="skipped"
        fi
    else
        ok "~/.cargo/bin is already in your PATH"
        ENV_STATUS="already configured"
    fi

    # FLACO_MODEL
    if [[ -n "$CHOSEN_MODEL" ]]; then
        echo ""
        if ask_yn "Set FLACO_MODEL=${CHOSEN_MODEL} as default in ${SHELL_PROFILE}?" "y"; then
            if grep -q 'FLACO_MODEL' "$SHELL_PROFILE" 2>/dev/null; then
                grep -v 'FLACO_MODEL' "$SHELL_PROFILE" > "${SHELL_PROFILE}.tmp" && mv "${SHELL_PROFILE}.tmp" "$SHELL_PROFILE"
            fi
            echo "export FLACO_MODEL=\"${CHOSEN_MODEL}\"" >> "$SHELL_PROFILE"
            ok "Set FLACO_MODEL=${CHOSEN_MODEL} in ${SHELL_PROFILE}"
            ENV_STATUS="configured"
        else
            info "Skipped. You can set it later:"
            echo -e "  ${CYAN}export FLACO_MODEL=\"${CHOSEN_MODEL}\"${RESET}"
        fi
    fi
else
    ENV_STATUS="skipped"
fi

# ─────────────────────────────────────────────────────────────
# Smoke test
# ─────────────────────────────────────────────────────────────

echo ""
echo -e "${BLUE}${BOLD}── Smoke test ──${RESET}"
echo ""

if command -v "$BIN_NAME" &>/dev/null; then
    FLACO_VER="$("$BIN_NAME" --version 2>&1 || echo 'unknown')"
    ok "${BIN_NAME} --version → ${FLACO_VER}"
    SMOKE_STATUS="passed"
else
    if [[ -f "$HOME/.cargo/bin/${BIN_NAME}" ]]; then
        FLACO_VER="$("$HOME/.cargo/bin/${BIN_NAME}" --version 2>&1 || echo 'unknown')"
        ok "${BIN_NAME} --version → ${FLACO_VER}  ${DIM}(needs PATH reload)${RESET}"
        SMOKE_STATUS="passed (PATH reload needed)"
    else
        fail "${BIN_NAME} not found"
        hint "Try: ${CYAN}source ${SHELL_PROFILE}${RESET} or restart your terminal"
        SMOKE_STATUS="not on PATH"
    fi
fi

if command -v ollama &>/dev/null; then
    echo ""
    info "Installed Ollama models:"
    ollama list 2>/dev/null | head -10 || warn "Could not list models"
fi

# ─────────────────────────────────────────────────────────────
# Save state for future runs
# ─────────────────────────────────────────────────────────────

save_state

# ─────────────────────────────────────────────────────────────
# Summary
# ─────────────────────────────────────────────────────────────

echo ""
echo ""
if [[ "$INSTALL_MODE" == "update" ]]; then
    echo -e "${GREEN}${BOLD}╔═══════════════════════════════════════════════════╗${RESET}"
    echo -e "${GREEN}${BOLD}║          🔄  Update complete!                    ║${RESET}"
    echo -e "${GREEN}${BOLD}╚═══════════════════════════════════════════════════╝${RESET}"
else
    echo -e "${GREEN}${BOLD}╔═══════════════════════════════════════════════════╗${RESET}"
    echo -e "${GREEN}${BOLD}║          🎉  Setup complete!                     ║${RESET}"
    echo -e "${GREEN}${BOLD}╚═══════════════════════════════════════════════════╝${RESET}"
fi
echo ""
echo -e "${WHITE}${BOLD}Results:${RESET}"
echo -e "  Rust ........... ${CYAN}${RUST_STATUS}${RESET}"
echo -e "  Ollama ......... ${CYAN}${OLLAMA_STATUS}${RESET}"
echo -e "  ${BIN_NAME} ...... ${CYAN}${BUILD_STATUS}${RESET}"
echo -e "  Model .......... ${CYAN}${MODEL_STATUS}${RESET}${CHOSEN_MODEL:+ (${BOLD}${CHOSEN_MODEL}${RESET})}"
echo -e "  Shell config ... ${CYAN}${ENV_STATUS}${RESET}"
echo -e "  Smoke test ..... ${CYAN}${SMOKE_STATUS}${RESET}"
echo ""
echo -e "${WHITE}${BOLD}🚀 Quick start:${RESET}"
echo ""
echo -e "  ${CYAN}${BIN_NAME}${RESET}                            Start the interactive REPL"
echo -e "  ${CYAN}${BIN_NAME} \"explain this function\"${RESET}     One-shot prompt"
if [[ -n "$CHOSEN_MODEL" ]]; then
    echo -e "  ${CYAN}${BIN_NAME} --model ${CHOSEN_MODEL}${RESET}   Use your selected model"
else
    echo -e "  ${CYAN}${BIN_NAME} --model qwen3:30b-a3b${RESET}   Use a specific model"
fi
echo ""

if [[ "$ENV_STATUS" == "configured" ]]; then
    echo -e "${YELLOW}${BOLD}⚡ Important:${RESET} Apply your new shell config:"
    echo -e "  ${CYAN}source ${SHELL_PROFILE}${RESET}"
    echo ""
fi

echo -e "${DIM}Run ${CYAN}./setup.sh${DIM} again anytime to update or reconfigure.${RESET}"
echo -e "${DIM}Happy hacking! 🤙${RESET}"
echo ""
