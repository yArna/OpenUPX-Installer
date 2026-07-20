# OpenUXP Installer

一个使用 Tauri 2 构建的轻量级 Adobe UXP 插件安装器，支持 macOS 和 Windows 的 `.ccx` 安装包。

项目主页：[github.com/yArna/OpenUPX-Installer](https://github.com/yArna/OpenUPX-Installer)

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

仅检查前端类型和构建：

```bash
npm run check
npm run build
```

构建桌面安装包：

```bash
npm run tauri build
```

## macOS 签名与公证

发布版本使用 `Developer ID Application` 证书、Hardened Runtime 和 Apple 公证。发布完全由本机脚本完成，不依赖 GitHub Actions。脚本会生成同时支持 Apple Silicon 与 Intel 的 Universal DMG，分别为 `.app` 和最终 DMG 提交公证并 stapling，然后检查代码签名、Gatekeeper 结果以及两者的公证票据。

先把证书及其私钥安装到登录钥匙串，然后设置以下环境变量：

```bash
export APPLE_SIGNING_IDENTITY="Developer ID Application: Example Inc (TEAMID)"
export APPLE_ID="developer@example.com"
export APPLE_PASSWORD="xxxx-xxxx-xxxx-xxxx"
export APPLE_TEAM_ID="TEAMID"
```

只构建、签名和公证，不上传：

```bash
npm run release:macos
```

发布到 GitHub Release 前，使用 `gh auth login` 登录，并确保当前工作区干净、提交已经推送。下面的命令直接校验并上传已经构建好的 DMG，不会再次编译；默认标签来自 `package.json` 的版本：

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

## 安装原理

应用调用 Creative Cloud Desktop 自带的 UPIA：

- macOS：`UnifiedPluginInstallerAgent --install /path/to/plugin.ccx`
- Windows：`UnifiedPluginInstallerAgent.exe /install C:\\path\\to\\plugin.ccx`

UPIA 不能单独下载；若应用提示未找到安装服务，请安装或更新 Adobe Creative Cloud Desktop。

## Photoshop 侧载

侧载选项只会在 Adobe 官方安装失败后出现，目前仅支持 macOS 和 Windows 上 manifest 宿主为 `PS` 的插件。应用会将插件解压到当前用户的 `Adobe/UXP/Plugins/External` 目录，并更新 `PluginsInfo/v1/PS.json`。侧载插件不会由 Creative Cloud 管理，使用前请确认安装包来源可信。
