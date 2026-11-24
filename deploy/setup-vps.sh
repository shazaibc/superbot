#!/bin/sh
# Sets up SuperBot with MiniMax + Caddy + systemd on an Ubuntu 24.04+ VPS.
# Run as root: sudo deploy/setup-vps.sh
set -eu

REPO_DIR=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
BOT_USER=${BOT_USER:-superbot}

echo "== Creating dedicated non-root user: $BOT_USER"
if ! id "$BOT_USER" >/dev/null 2>&1; then
  useradd -m -s /bin/bash "$BOT_USER"
fi

echo "== Installing base packages, Node.js 22, and Caddy"
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
apt-get install -y -qq git curl build-essential pkg-config ca-certificates gnupg openssl \
  debian-keyring debian-archive-keyring apt-transport-https >/dev/null

if ! command -v node >/dev/null 2>&1; then
  curl -fsSL https://deb.nodesource.com/setup_22.x | bash - >/dev/null
  apt-get install -y -qq nodejs >/dev/null
fi

if ! command -v caddy >/dev/null 2>&1; then
  curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/gpg.key' | gpg --dearmor -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg
  curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt' | tee /etc/apt/sources.list.d/caddy-stable.list >/dev/null
  apt-get update -qq && apt-get install -y -qq caddy >/dev/null
fi

echo "== Installing Claude Code CLI (used by the minimax harness)"
npm install -g @anthropic-ai/claude-code >/dev/null

APP_DIR="/home/$BOT_USER/superbot"
if [ "$REPO_DIR" != "$APP_DIR" ]; then
  if [ -d "$APP_DIR" ]; then
    echo "== Updating existing $APP_DIR (preserving .env, data/, workspace/)"
    tar -C "$REPO_DIR" --exclude='.env' --exclude='data' --exclude='workspace' --exclude='target' -cf - . | tar -C "$APP_DIR" -xf -
  else
    cp -a "$REPO_DIR" "$APP_DIR"
  fi
fi
chown -R "$BOT_USER:$BOT_USER" "$APP_DIR"

echo "== Installing Rust and building botd + botctl as $BOT_USER"
su - "$BOT_USER" <<'EOF'
set -eu
if ! command -v cargo >/dev/null 2>&1; then
  curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal >/dev/null
  . "$HOME/.cargo/env"
fi
cd "$HOME/superbot"
cargo build --release
if [ ! -f "$HOME/.claude.json" ]; then
  printf '%s\n' '{"hasCompletedOnboarding":true}' > "$HOME/.claude.json"
  chmod 600 "$HOME/.claude.json"
fi
if [ ! -f .env ]; then
  cp .env.example .env
  PASS=$(openssl rand -hex 18)
  sed -i "s/^AUTH_PASSWORD=.*/AUTH_PASSWORD=$PASS/" .env
  chmod 600 .env
fi
EOF

echo "== Installing Bot computers (Xvfb, Openbox, agent-browser, agent-desktop, voxl)"
"$APP_DIR/deploy/install-computer.sh" "$BOT_USER"

echo "== Installing systemd service"
sed "s|superbot|$BOT_USER|g" "$APP_DIR/deploy/botd.service.example" > /etc/systemd/system/botd.service
systemctl daemon-reload
systemctl enable botd >/dev/null

echo
echo "Setup complete."
echo "1) Edit /home/$BOT_USER/superbot/.env and set MINIMAX_API_KEY and USER_NAME."
echo "   View generated login password: sudo grep '^AUTH_PASSWORD=' /home/$BOT_USER/superbot/.env"
echo "2) Configure /etc/caddy/Caddyfile from $APP_DIR/deploy/Caddyfile.example (replace <your-domain> with your HTTPS domain)."
echo "3) Run: sudo systemctl restart botd caddy"
