#!/usr/bin/env bash

set -euo pipefail

OPENUXP_SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OPENUXP_PROJECT_DIR="$(cd "$OPENUXP_SCRIPT_DIR/.." && pwd)"
OPENUXP_BUNDLE_DIR="$OPENUXP_PROJECT_DIR/src-tauri/target/universal-apple-darwin/release/bundle"
OPENUXP_DEFAULT_UPDATER_KEY="$OPENUXP_PROJECT_DIR/secret/openuxp-installer.key"
OPENUXP_BUILD=true
OPENUXP_PUBLISH=false
OPENUXP_DRAFT=false
OPENUXP_RELEASE_TAG=""

fail() {
  printf '错误：%s\n' "$1" >&2
  exit 1
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || fail "未找到 $1"
}

usage() {
  cat <<'EOF'
用法：release-macos.sh [选项]

  --publish       构建完成后创建 GitHub Release 并上传 DMG
  --publish-existing
                  跳过构建，发布已有且已公证的 DMG
  --draft         创建草稿 Release（同时启用 --publish）
  --tag <tag>     指定 Release 标签，默认使用 package.json 中的 v<version>
  -h, --help      显示帮助
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --publish)
      OPENUXP_PUBLISH=true
      shift
      ;;
    --publish-existing)
      OPENUXP_BUILD=false
      OPENUXP_PUBLISH=true
      shift
      ;;
    --draft)
      OPENUXP_PUBLISH=true
      OPENUXP_DRAFT=true
      shift
      ;;
    --tag)
      [[ $# -ge 2 && -n "$2" ]] || fail "--tag 需要一个标签"
      OPENUXP_RELEASE_TAG="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      fail "未知选项：$1"
      ;;
  esac
done

# 兼容现有项目 .env 中使用的变量名。
APPLE_SIGNING_IDENTITY="${APPLE_SIGNING_IDENTITY:-${MACOS_SIGNING_IDENTITY:-}}"
APPLE_PASSWORD="${APPLE_PASSWORD:-${APPLE_APP_SPECIFIC_PASSWORD:-}}"
export APPLE_SIGNING_IDENTITY APPLE_PASSWORD

[[ "$(uname -s)" == "Darwin" ]] || fail "macOS 发布只能在 macOS 上执行"

require_command node
require_command codesign
require_command spctl
require_command xcrun
xcrun --find stapler >/dev/null 2>&1 || fail "未找到 stapler，请安装完整版本的 Xcode"

cd "$OPENUXP_PROJECT_DIR"

OPENUXP_VERSION="$(node -e "const fs=require('node:fs'); console.log(JSON.parse(fs.readFileSync('package.json', 'utf8')).version)")"
[[ -n "$OPENUXP_VERSION" ]] || fail "无法读取 package.json 版本"
if [[ -z "$OPENUXP_RELEASE_TAG" ]]; then
  OPENUXP_RELEASE_TAG="v$OPENUXP_VERSION"
fi

if [[ "$OPENUXP_PUBLISH" == true ]]; then
  require_command gh
  require_command git
  gh auth status >/dev/null 2>&1 || fail "GitHub CLI 尚未登录，请先运行 gh auth login"
  [[ -z "$(git status --porcelain)" ]] || fail "Git 工作区存在未提交变更，请提交后再发布"

  OPENUXP_BRANCH="$(git branch --show-current)"
  [[ -n "$OPENUXP_BRANCH" ]] || fail "当前处于 detached HEAD，无法确认远程发布提交"
  OPENUXP_COMMIT="$(git rev-parse HEAD)"
  OPENUXP_REMOTE_COMMIT="$(git ls-remote --exit-code origin "refs/heads/$OPENUXP_BRANCH" | awk '{print $1}')" \
    || fail "无法读取 origin/$OPENUXP_BRANCH"
  [[ "$OPENUXP_COMMIT" == "$OPENUXP_REMOTE_COMMIT" ]] \
    || fail "当前提交尚未推送到 origin/$OPENUXP_BRANCH"
  if gh release view "$OPENUXP_RELEASE_TAG" >/dev/null 2>&1; then
    fail "GitHub Release $OPENUXP_RELEASE_TAG 已存在，请更新版本或使用 --tag 指定新标签"
  fi
fi

if [[ "$OPENUXP_BUILD" == true ]]; then
  require_command npm
  require_command rustup
  require_command security
  xcrun --find notarytool >/dev/null 2>&1 || fail "未找到 notarytool，请安装完整版本的 Xcode"

  [[ -n "${APPLE_SIGNING_IDENTITY:-}" ]] || fail "请设置 APPLE_SIGNING_IDENTITY"
  if [[ -n "${APPLE_ID:-}" && -n "${APPLE_PASSWORD:-}" && -n "${APPLE_TEAM_ID:-}" ]]; then
    :
  elif [[ -n "${APPLE_API_ISSUER:-}" && -n "${APPLE_API_KEY:-}" && -n "${APPLE_API_KEY_PATH:-}" ]]; then
    [[ -f "$APPLE_API_KEY_PATH" ]] || fail "APPLE_API_KEY_PATH 指向的私钥不存在"
  else
    fail "请设置 APPLE_ID、APPLE_PASSWORD、APPLE_TEAM_ID，或 App Store Connect API 三个变量"
  fi

  security find-identity -v -p codesigning | grep -F "$APPLE_SIGNING_IDENTITY" >/dev/null \
    || fail "当前钥匙串中找不到 APPLE_SIGNING_IDENTITY 指定的签名证书"

  if [[ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" ]]; then
    OPENUXP_UPDATER_KEY="${TAURI_SIGNING_PRIVATE_KEY_PATH:-$OPENUXP_DEFAULT_UPDATER_KEY}"
    [[ -f "$OPENUXP_UPDATER_KEY" ]] \
      || fail "未找到 Tauri 更新私钥：$OPENUXP_UPDATER_KEY"
    export TAURI_SIGNING_PRIVATE_KEY="$OPENUXP_UPDATER_KEY"
  fi
  export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}"

  rustup target add aarch64-apple-darwin x86_64-apple-darwin
  npm ci
  npm run check
  cargo test --manifest-path src-tauri/Cargo.toml
  npm run tauri build -- --target universal-apple-darwin --bundles app,dmg
fi

OPENUXP_APP_PATH=""
if [[ -d "$OPENUXP_BUNDLE_DIR/macos" ]]; then
  OPENUXP_APP_PATH="$(find "$OPENUXP_BUNDLE_DIR/macos" -maxdepth 1 -type d -name '*.app' -print -quit)"
fi
[[ -d "$OPENUXP_BUNDLE_DIR/dmg" ]] || fail "未找到 DMG 构建目录，请先运行 npm run release:macos"
OPENUXP_DMG_PATH="$(find "$OPENUXP_BUNDLE_DIR/dmg" -maxdepth 1 -type f -name '*.dmg' -print -quit)"
OPENUXP_UPDATE_PATH=""
if [[ -d "$OPENUXP_BUNDLE_DIR/macos" ]]; then
  OPENUXP_UPDATE_PATH="$(find "$OPENUXP_BUNDLE_DIR/macos" -maxdepth 1 -type f -name '*.app.tar.gz' -print -quit)"
fi

[[ -n "$OPENUXP_DMG_PATH" ]] || fail "未找到构建后的 .dmg"
if [[ "$OPENUXP_PUBLISH" == true ]]; then
  [[ -n "$OPENUXP_UPDATE_PATH" ]] || fail "未找到 Tauri 更新包，请先重新运行 npm run release:macos"
  [[ -f "$OPENUXP_UPDATE_PATH.sig" ]] || fail "未找到 Tauri 更新签名：$OPENUXP_UPDATE_PATH.sig"
fi

if [[ "$OPENUXP_BUILD" == true ]]; then
  [[ -n "$OPENUXP_APP_PATH" ]] || fail "未找到构建后的 .app"
  if [[ -n "${APPLE_ID:-}" ]]; then
    xcrun notarytool submit "$OPENUXP_DMG_PATH" \
      --apple-id "$APPLE_ID" \
      --password "$APPLE_PASSWORD" \
      --team-id "$APPLE_TEAM_ID" \
      --wait \
      --timeout 30m
  else
    xcrun notarytool submit "$OPENUXP_DMG_PATH" \
      --issuer "$APPLE_API_ISSUER" \
      --key-id "$APPLE_API_KEY" \
      --key "$APPLE_API_KEY_PATH" \
      --wait \
      --timeout 30m
  fi
  xcrun stapler staple "$OPENUXP_DMG_PATH"
fi

codesign --verify --strict --verbose=2 "$OPENUXP_DMG_PATH"
xcrun stapler validate "$OPENUXP_DMG_PATH"
if [[ -n "$OPENUXP_APP_PATH" ]]; then
  codesign --verify --deep --strict --verbose=2 "$OPENUXP_APP_PATH"
  spctl --assess --type execute --verbose=4 "$OPENUXP_APP_PATH"
  xcrun stapler validate "$OPENUXP_APP_PATH"
fi

printf '\n已确认发布产物签名和公证有效：\n%s\n' "$OPENUXP_DMG_PATH"

if [[ "$OPENUXP_PUBLISH" == true ]]; then
  OPENUXP_UPDATE_MANIFEST="$OPENUXP_BUNDLE_DIR/latest.json"
  node scripts/create-update-manifest.mjs \
    "$OPENUXP_VERSION" \
    "$OPENUXP_RELEASE_TAG" \
    "$OPENUXP_UPDATE_PATH" \
    "$OPENUXP_UPDATE_PATH.sig" \
    "$OPENUXP_UPDATE_MANIFEST"

  OPENUXP_RELEASE_ARGS=(
    release create "$OPENUXP_RELEASE_TAG"
    "$OPENUXP_DMG_PATH"
    "$OPENUXP_UPDATE_PATH"
    "$OPENUXP_UPDATE_PATH.sig"
    "$OPENUXP_UPDATE_MANIFEST"
    --title "OpenUXP Installer $OPENUXP_RELEASE_TAG"
    --generate-notes
    --target "$OPENUXP_COMMIT"
  )
  if [[ "$OPENUXP_DRAFT" == true ]]; then
    OPENUXP_RELEASE_ARGS+=(--draft)
  fi

  gh "${OPENUXP_RELEASE_ARGS[@]}"
  printf '\n已发布 GitHub Release：\n'
  gh release view "$OPENUXP_RELEASE_TAG" --json url --jq .url
fi
