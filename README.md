# OpenUXP Installer

一个使用 Tauri 2 构建的轻量级 Adobe UXP 插件安装器，支持 macOS 和 Windows 的 `.ccx` 安装包。

- 官网：[yarna.github.io/OpenUPX-Installer](https://yarna.github.io/OpenUPX-Installer/)
- 源码：[github.com/yArna/OpenUPX-Installer](https://github.com/yArna/OpenUPX-Installer)

## 功能

- 拖放或选择 `.ccx` 文件
- 在安装前读取并展示 UXP 清单、版本和兼容宿主
- 自动探测 Creative Cloud Desktop 附带的 Unified Plugin Installer Agent（UPIA）
- 使用 Adobe 官方安装服务完成安装，并展示错误代码与诊断输出
- 官方安装失败后，可选择将 Photoshop 插件侧载到用户级 UXP 目录
- 侧载前检查插件目录、注册目录及 `PS.json` 的读写权限
- 安全解压 CCX、备份注册文件、去重更新，并在写入失败时自动回滚

## 开发

需要 Node.js 18+、Rust，以及对应平台的 Tauri 系统依赖。

```bash
npm install
npm run dev
```

如需单独调试 Web 界面，可运行 `npm run dev:web`。

仅检查前端类型：

```bash
npm run check
```

构建桌面应用（会先构建前端）：

```bash
npm run build
```

仅构建前端：

```bash
npm run build:web
```

## macOS 签名与公证

发布版本使用 `Developer ID Application` 证书、Hardened Runtime 和 Apple 公证。发布完全由本机脚本完成，不依赖 GitHub Actions。脚本会生成同时支持 Apple Silicon 与 Intel 的 Universal DMG，分别为 `.app` 和最终 DMG 提交公证并 stapling，然后检查代码签名、Gatekeeper 结果以及两者的公证票据。

先把证书及其私钥安装到登录钥匙串，然后把发布凭据写入 `secret/.env`（推荐）或项目根目录的 `.env`。发布脚本会自动加载：

```dotenv
APPLE_SIGNING_IDENTITY="Developer ID Application: Example Inc (TEAMID)"
APPLE_ID="developer@example.com"
APPLE_PASSWORD="xxxx-xxxx-xxxx-xxxx"
APPLE_TEAM_ID="TEAMID"
```

也可以继续使用当前 Shell 已导出的变量，或通过 `OPENUXP_ENV_FILE=/path/to/file` 指定其他配置文件。

只构建、签名和公证，不上传：

```bash
npm run release:macos
```

发布到 GitHub Release 前，使用 `gh auth login` 登录，并确保当前工作区干净、提交已经推送。下面的命令直接校验并上传已经构建好的 DMG，不会再次编译；默认标签来自 `package.json` 的应用版本：

```bash
npm run release:macos:github
```

如需一次完成构建、签名、公证和上传：

```bash
npm run release:macos:github:build
```

指定标签或先创建草稿：

```bash
npm run release:macos:github -- --tag v0.2.0
npm run release:macos:github -- --tag v0.2.0 --draft
```

脚本兼容 `MACOS_SIGNING_IDENTITY`、`APPLE_APP_SPECIFIC_PASSWORD` 变量别名，也支持用 `APPLE_API_ISSUER`、`APPLE_API_KEY`、`APPLE_API_KEY_PATH` 代替 Apple ID 公证凭据。

## 自动更新

应用启动后会通过 [GitHub Releases](https://github.com/yArna/OpenUPX-Installer/releases) 的 `latest.json` 静默检查新版本。发现更新时会显示版本和下载进度，安装完成后自动重启。更新包必须通过 Tauri 独立签名验证。

本机更新私钥和公钥保存在项目的 `./secret` 目录，发布脚本会自动读取。该目录已被 Git 忽略。请将私钥安全备份；私钥丢失后，已经安装的旧版本将无法验证后续更新。公钥内容已经写入应用配置，可以公开。

`npm run release:macos` 会生成 `.app.tar.gz` 及其 `.sig`；`npm run release:macos:github` 会把更新包、签名、DMG 和自动生成的 `latest.json` 一并上传到 GitHub Release。

## Windows 交叉构建

macOS 可以使用 `cargo-xwin` 交叉构建 Windows x64 NSIS 安装包：

```bash
brew install llvm lld nsis
cargo install --locked cargo-xwin
npm run release:windows
```

产物位于 `src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis/`，包括 `-setup.exe` 和对应的 `.sig`。完成 macOS、Windows 构建后运行 `npm run release:github`，发布脚本会自动把两个平台的产物合并到同一个 GitHub Release 和 `latest.json`。

完整的一键发布命令：

```bash
npm run release
```

`package.json` 是应用版本号的唯一来源。`npm run dev`、`npm run build` 和所有发布脚本都会先将版本同步到 `package-lock.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml` 和 `src-tauri/Cargo.lock`；也可以单独运行 `npm run version:sync`，或使用 `npm run version:check` 只校验而不修改文件。

该命令支持失败后继续：如果当前 `package.json` 应用版本对应的 Windows `-setup.exe` 和 `.sig` 已经存在，会直接复用它们，只重新执行 macOS Universal 签名与公证，最后把两个平台的安装包、更新签名和 `latest.json` 发布到 GitHub Release。版本发生变化时会自动重新构建 Windows，避免复用旧产物。

## 安装原理

应用调用 Creative Cloud Desktop 自带的 UPIA：

- macOS：`UnifiedPluginInstallerAgent --install /path/to/plugin.ccx`
- Windows：`UnifiedPluginInstallerAgent.exe /install C:\\path\\to\\plugin.ccx`

UPIA 不能单独下载；若应用提示未找到安装服务，请安装或更新 Adobe Creative Cloud Desktop。

## Photoshop 侧载

侧载选项只会在 Adobe 官方安装失败后出现，目前仅支持 macOS 和 Windows 上 manifest 宿主为 `PS` 的插件。应用会将插件解压到当前用户的 `Adobe/UXP/Plugins/External` 目录，并更新 `PluginsInfo/v1/PS.json`。侧载插件不会由 Creative Cloud 管理，使用前请确认安装包来源可信。
