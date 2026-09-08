#!/usr/bin/env bash

set -euo pipefail

OPENUXP_SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OPENUXP_PROJECT_DIR="$(cd "$OPENUXP_SCRIPT_DIR/.." && pwd)"
OPENUXP_WINDOWS_BUNDLE_DIR="$OPENUXP_PROJECT_DIR/src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis"

cd "$OPENUXP_PROJECT_DIR"
npm run version:sync
OPENUXP_VERSION="$(node -p "require('./package.json').version")"
[[ -n "$OPENUXP_VERSION" ]] || { printf '错误：无法读取 package.json 版本\n' >&2; exit 1; }

OPENUXP_WINDOWS_INSTALLER=""
if [[ -d "$OPENUXP_WINDOWS_BUNDLE_DIR" ]]; then
  OPENUXP_WINDOWS_INSTALLER="$(find "$OPENUXP_WINDOWS_BUNDLE_DIR" -maxdepth 1 -type f -name "*_${OPENUXP_VERSION}_*-setup.exe" -print -quit)"
fi

if [[ -n "$OPENUXP_WINDOWS_INSTALLER" && -s "$OPENUXP_WINDOWS_INSTALLER.sig" ]]; then
  printf '复用 Windows %s 构建产物：\n%s\n\n' "$OPENUXP_VERSION" "$OPENUXP_WINDOWS_INSTALLER"
else
  printf '未找到完整的 Windows %s 构建产物，开始构建。\n\n' "$OPENUXP_VERSION"
  npm run release:windows
fi

npm run release:macos
npm run release:upload
