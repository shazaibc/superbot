#!/bin/sh
# Installs the Bots' computer on Ubuntu 24.04+: virtual screens (Xvfb + Openbox),
# agent-browser with Chrome, agent-desktop, and voxl for the live view.
# Run as root: sudo deploy/install-computer.sh <user that runs botd>
# Safe to rerun. botd turns screens on by itself when these tools are on PATH.
set -eu
BOT_USER=${1:?usage: install-computer.sh <user that runs botd>}
AB_VERSION=v0.43.0  # agent-browser
AB_SHA256=8e69c9fb662514c9cf36e4911e94e913742ba066e55102486350e049dd847612
AD_VERSION=v1.3.1   # agent-desktop
AD_SHA256=ca77b6b75034eeb12e537dd8c0a4f2a3424119253f38b7e6bd79ba0d2fef1264
VOXL_VERSION=0.3.5
SHARE=/opt/computer/share
export DEBIAN_FRONTEND=noninteractive

echo "== System packages"
apt-get update -qq
apt-get install -y -qq xvfb openbox x11-utils dbus-x11 at-spi2-core fonts-noto-core fonts-noto-color-emoji \
  git curl build-essential pkg-config clang libx264-dev libyuv-dev libopus-dev esbuild earlyoom >/dev/null

echo "== agent-browser $AB_VERSION"
# Download, verify SHA-256, then rename into place: works while a browser daemon is running the old binary.
curl -fsSL -o /usr/local/bin/.agent-browser.new "https://github.com/vercel-labs/agent-browser/releases/download/$AB_VERSION/agent-browser-linux-x64"
echo "$AB_SHA256  /usr/local/bin/.agent-browser.new" | sha256sum -c -
chmod 755 /usr/local/bin/.agent-browser.new && mv -f /usr/local/bin/.agent-browser.new /usr/local/bin/agent-browser
agent-browser install --with-deps >/dev/null   # Chrome's system libraries
su - "$BOT_USER" -c "agent-browser install" >/dev/null  # Chrome for Testing, for the Bot user
rm -rf /root/.agent-browser
# The release binary ships without its skills; agent-browser reads them from AGENT_BROWSER_SKILLS_DIR.
mkdir -p "$SHARE" && rm -rf /tmp/ab-src "$SHARE/agent-browser-skills"
git -c advice.detachedHead=false clone -q --depth 1 --branch "$AB_VERSION" --filter=blob:none --sparse https://github.com/vercel-labs/agent-browser.git /tmp/ab-src
git -C /tmp/ab-src sparse-checkout set skill-data
cp -r /tmp/ab-src/skill-data "$SHARE/agent-browser-skills" && rm -rf /tmp/ab-src

# Ubuntu 23.10+ blocks Chrome's sandbox for unprivileged users. Allow it for this Bot user's Chrome only.
cat > /etc/apparmor.d/agent-browser-chrome <<AA
abi <abi/4.0>,
include <tunables/global>
profile agent-browser-chrome /home/$BOT_USER/.agent-browser/browsers/*/chrome flags=(unconfined) {
  userns,
  include if exists <local/agent-browser-chrome>
}
AA
apparmor_parser -r /etc/apparmor.d/agent-browser-chrome

echo "== agent-desktop $AD_VERSION"
mkdir -p /tmp/ad && curl -fsSL -o /tmp/ad/ad.tar.gz "https://github.com/vercel-labs/agent-desktop/releases/download/$AD_VERSION/agent-desktop-$AD_VERSION-x86_64-unknown-linux-gnu.tar.gz"
echo "$AD_SHA256  /tmp/ad/ad.tar.gz" | sha256sum -c -
tar -xzf /tmp/ad/ad.tar.gz -C /tmp/ad
mv -f /tmp/ad/agent-desktop /tmp/ad/agent-desktop-linux-clipboard-owner /usr/local/bin/ && rm -rf /tmp/ad

echo "== voxl $VOXL_VERSION (builds from source, a few minutes)"
if ! voxl --version 2>/dev/null | grep -q "$VOXL_VERSION"; then
  command -v cargo >/dev/null || { curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal >/dev/null; . "$HOME/.cargo/env"; }
  cargo install -q --locked --version "$VOXL_VERSION" voxl-server --no-default-features --root /usr/local
fi

echo "== Memory safety net: 4 GB swap, earlyoom kills the biggest process before the server stalls"
if ! swapon --show | grep -q /swapfile; then
  fallocate -l 4G /swapfile && chmod 600 /swapfile && mkswap -q /swapfile && swapon /swapfile
  grep -q /swapfile /etc/fstab || echo "/swapfile none swap sw 0 0" >> /etc/fstab
fi
echo 'EARLYOOM_ARGS="-r 0 -m 5 -s 50 --prefer (^|/)(chrome)$ --avoid (^|/)(botd|sshd|caddy|systemd)$"' > /etc/default/earlyoom
systemctl enable -q --now earlyoom && systemctl restart earlyoom

echo "Done. Add to botd's environment: AGENT_BROWSER_SKILLS_DIR=$SHARE/agent-browser-skills"
echo "Then restart botd. It prints 'Screens: on.'"
