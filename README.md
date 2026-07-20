# OpenUXP Installer

一个使用 Tauri 2 构建的轻量级 Adobe UXP 插件安装器，支持 macOS 和 Windows 的 `.ccx` 安装包。

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

## 安装原理

应用调用 Creative Cloud Desktop 自带的 UPIA：

- macOS：`UnifiedPluginInstallerAgent --install /path/to/plugin.ccx`
- Windows：`UnifiedPluginInstallerAgent.exe /install C:\\path\\to\\plugin.ccx`

UPIA 不能单独下载；若应用提示未找到安装服务，请安装或更新 Adobe Creative Cloud Desktop。

## Photoshop 侧载

侧载选项只会在 Adobe 官方安装失败后出现，目前仅支持 macOS 和 Windows 上 manifest 宿主为 `PS` 的插件。应用会将插件解压到当前用户的 `Adobe/UXP/Plugins/External` 目录，并更新 `PluginsInfo/v1/PS.json`。侧载插件不会由 Creative Cloud 管理，使用前请确认安装包来源可信。
