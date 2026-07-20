#!/usr/bin/env bash

set -euo pipefail

OPENUXP_SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OPENUXP_PROJECT_DIR="$(cd "$OPENUXP_SCRIPT_DIR/.." && pwd)"
OPENUXP_TARGET="x86_64-pc-windows-msvc"
OPENUXP_BUNDLE_DIR="$OPENUXP_PROJECT_DIR/src-tauri/target/$OPENUXP_TARGET/release/bundle/nsis"
OPENUXP_UPDATER_KEY="$OPENUXP_PROJECT_DIR/secret/openuxp-installer.key"

fail() {
  printf '错误：%s\n' "$1" >&2
  exit 1
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || fail "未找到 $1"
}

require_command npm
require_command rustup
require_command cargo-xwin
require_command makensis

for OPENUXP_LLVM_BIN in \
  /opt/homebrew/opt/llvm/bin \
  /opt/homebrew/opt/lld/bin \
  /usr/local/opt/llvm/bin \
  /usr/local/opt/lld/bin; do
  if [[ -d "$OPENUXP_LLVM_BIN" ]]; then
    export PATH="$OPENUXP_LLVM_BIN:$PATH"
  fi
done
require_command llvm-rc
require_command lld-link

if [[ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" ]]; then
  [[ -f "$OPENUXP_UPDATER_KEY" ]] || fail "未找到 Tauri 更新私钥：$OPENUXP_UPDATER_KEY"
  export TAURI_SIGNING_PRIVATE_KEY="$OPENUXP_UPDATER_KEY"
fi
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}"

cd "$OPENUXP_PROJECT_DIR"
npm run version:sync
rustup target add "$OPENUXP_TARGET"
npm ci
npm run check
cargo test --manifest-path src-tauri/Cargo.toml
npm exec tauri -- build \
  --runner cargo-xwin \
  --target "$OPENUXP_TARGET" \
  --bundles nsis

OPENUXP_INSTALLER="$(find "$OPENUXP_BUNDLE_DIR" -maxdepth 1 -type f -name '*-setup.exe' -print -quit)"
[[ -n "$OPENUXP_INSTALLER" ]] || fail "未找到 Windows NSIS 安装包"
[[ -f "$OPENUXP_INSTALLER.sig" ]] || fail "未找到 Windows updater 签名：$OPENUXP_INSTALLER.sig"

printf '\n已完成 Windows x64 交叉构建：\n%s\n%s\n' "$OPENUXP_INSTALLER" "$OPENUXP_INSTALLER.sig"
