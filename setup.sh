#!/usr/bin/env bash
set -euo pipefail

# ──────────────────────────────────────────────────
# flacoAi setup — one script to rule them all
# ──────────────────────────────────────────────────

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
BOLD='\033[1m'
DIM='\033[2m'
GREEN='\033[32m'
YELLOW='\033[33m'
CYAN='\033[36m'
RESET='\033[0m'

step() { echo -e "\n${CYAN}${BOLD}→ $1${RESET}"; }
ok()   { echo -e "  ${GREEN}✔ $1${RESET}"; }
warn() { echo -e "  ${YELLOW}⚠ $1${RESET}"; }
info() { echo -e "  ${DIM}$1${RESET}"; }

echo -e "${BOLD}"
echo "  __ _                    _    _ "
echo " / _| | __ _  ___ ___   / \  (_)"
echo "| |_| |/ _\` |/ __/ _ \ / _ \ | |"
echo "|  _| | (_| | (_| (_) / ___ \| |"
echo "|_| |_|\__,_|\___\___/_/   \_\_|"
echo -e "${RESET}"
echo -e "${DIM}Local AI coding agent powered by Ollama${RESET}"
echo ""

# ── 1. Rust ──────────────────────────────────────

step "Checking for Rust toolchain"

if command -v cargo &>/dev/null; then
    ok "cargo found: $(cargo --version)"
else
    warn "Rust not found — installing via rustup"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
    ok "Rust installed: $(cargo --version)"
fi

# ── 2. Build flaco binary ────────────────────────

step "Building flaco (release mode)"
info "This may take a few minutes on first build..."

cd "$SCRIPT_DIR/rust"
cargo build --release -p flaco-cli 2>&1 | tail -5

ok "Build complete"

# ── 3. Install to ~/.cargo/bin ───────────────────

step "Installing flaco binary"

cargo install --path crates/flaco-cli --force 2>&1 | tail -3

FLACO_BIN="$(which flaco 2>/dev/null || echo "$HOME/.cargo/bin/flaco")"
ok "Installed to $FLACO_BIN"

# ── 4. Verify it's on PATH ──────────────────────

step "Verifying PATH"

if command -v flaco &>/dev/null; then
    ok "flaco is on your PATH"
else
    # Add cargo bin to current shell profile
    SHELL_NAME="$(basename "$SHELL")"
    case "$SHELL_NAME" in
        zsh)  PROFILE="$HOME/.zshrc" ;;
        bash) PROFILE="$HOME/.bashrc" ;;
        fish) PROFILE="$HOME/.config/fish/config.fish" ;;
        *)    PROFILE="$HOME/.profile" ;;
    esac

    if ! grep -q '.cargo/bin' "$PROFILE" 2>/dev/null; then
        echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> "$PROFILE"
        ok "Added ~/.cargo/bin to $PROFILE"
        info "Run: source $PROFILE  (or restart your terminal)"
    else
        warn "~/.cargo/bin is in $PROFILE but not in current PATH"
        info "Restart your terminal or run: source $PROFILE"
    fi
fi

# ── 5. Python fallback (optional) ───────────────

step "Setting up Python package (optional pip fallback)"

cd "$SCRIPT_DIR"
if command -v pip3 &>/dev/null || command -v pip &>/dev/null; then
    PIP="$(command -v pip3 || command -v pip)"
    $PIP install -e . --quiet 2>/dev/null && ok "Python package installed" || warn "pip install skipped (non-critical)"
else
    info "pip not found — skipping Python package (Rust binary is the primary)"
fi

# ── 6. Ollama check ─────────────────────────────

step "Checking for Ollama"

if command -v ollama &>/dev/null; then
    ok "Ollama found: $(ollama --version 2>/dev/null || echo 'installed')"
    info "Models available:"
    ollama list 2>/dev/null | head -5 || true
else
    warn "Ollama not installed"
    echo ""
    echo -e "  Install it from: ${BOLD}https://ollama.com${RESET}"
    echo -e "  Then pull a model: ${DIM}ollama pull qwen3:30b-a3b${RESET}"
fi

# ── Done ─────────────────────────────────────────

echo ""
echo -e "${GREEN}${BOLD}Setup complete!${RESET}"
echo ""
echo -e "  ${BOLD}Quick start:${RESET}"
echo -e "    ${CYAN}flaco${RESET}                              Start the REPL"
echo -e "    ${CYAN}flaco \"explain this code\"${RESET}           One-shot prompt"
echo -e "    ${CYAN}flaco --model qwen3:30b-a3b${RESET}         Use a specific model"
echo ""
echo -e "  ${BOLD}Set your default model:${RESET}"
echo -e "    ${DIM}export FLACO_MODEL=qwen3:30b-a3b${RESET}"
echo ""
