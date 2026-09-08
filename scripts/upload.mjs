#!/usr/bin/env node

import { existsSync, readdirSync, readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { resolve, dirname, join, basename } from "node:path";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const projectRoot = resolve(__dirname, "..");

// 优先通过标准依赖加载 qrls，如未安装则自动回退至本地源码/构建产物路径
async function loadQRls() {
  try {
    return await import("qrls");
  } catch {
    const candidates = [
      "/Users/yarna/Project/Qzrzz/Code/QRls/dist/index.js",
      "/Users/yarna/Project/Qzrzz/Code/QRls",
    ];
    for (const path of candidates) {
      try {
        return await import(path);
      } catch {}
    }
    throw new Error(
      "未找到 qrls 模块，请确认 /Users/yarna/Project/Qzrzz/Code/QRls 存在并已构建 (dist/index.js)"
    );
  }
}

function findFile(dir, pattern) {
  if (!existsSync(dir)) return null;
  const files = readdirSync(dir);
  for (const file of files) {
    if (pattern.test(file)) {
      return join(dir, file);
    }
  }
  return null;
}

function printUsage() {
  console.log(`
用法: node scripts/upload.mjs [选项]

选项:
  --dry                空跑模式 (只校验配置与文件指纹，不执行实际网络上传)
  --force              强制重新上传 (忽略 .qrls-state.json 断点记录)
  --draft              创建 GitHub 草稿 Release
  --target <list>      指定上传目标 (逗号分隔，可选: github, lanzou，默认两者均上传)
  --folder, --path     指定蓝奏云文件夹 ID (默认为根目录 "-1")
  --dmg <path>         手动指定 macOS DMG 安装包路径
  --exe <path>         手动指定 Windows EXE 安装包路径
  -h, --help           显示帮助信息
`);
}

async function main() {
  const args = process.argv.slice(2);
  let isDryRun = false;
  let isForce = false;
  let isDraft = false;
  let folderId = "-1";
  let customDmg = null;
  let customExe = null;
  let targetNames = ["github", "lanzou"];

  for (let i = 0; i < args.length; i++) {
    const arg = args[i];
    if (arg === "--dry") {
      isDryRun = true;
    } else if (arg === "--force") {
      isForce = true;
    } else if (arg === "--draft") {
      isDraft = true;
    } else if (arg === "--target") {
      const list = args[++i];
      if (list) {
        targetNames = list.split(",").map((s) => s.trim().toLowerCase()).filter(Boolean);
      }
    } else if (arg === "--folder" || arg === "--path") {
      folderId = args[++i] || "-1";
    } else if (arg === "--dmg") {
      customDmg = args[++i];
    } else if (arg === "--exe") {
      customExe = args[++i];
    } else if (arg === "-h" || arg === "--help") {
      printUsage();
      process.exit(0);
    } else {
      console.warn(`⚠️ 未知参数: ${arg}`);
    }
  }

  // 1. 读取当前项目版本
  const pkgPath = join(projectRoot, "package.json");
  const pkg = JSON.parse(readFileSync(pkgPath, "utf8"));
  const version = pkg.version;
  if (!version) {
    throw new Error("无法从 package.json 读取版本号");
  }

  const tag = `v${version}`;
  console.log(`\n🚀 准备使用 QRls 发布 OpenUXP Installer ${tag}...`);
  console.log(`  🎯 目标渠道: ${targetNames.join(", ")}`);
  if (isDryRun) console.log(`  💡 运行模式: DRY RUN (空跑测试)`);

  // 2. 确定待上传文件
  const macosBundleDir = join(
    projectRoot,
    "src-tauri/target/universal-apple-darwin/release/bundle"
  );
  const macosDmgDir = join(macosBundleDir, "dmg");
  const macosAppDir = join(macosBundleDir, "macos");

  const windowsBundleDir = join(
    projectRoot,
    "src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis"
  );

  const dmgPath =
    customDmg ||
    findFile(macosDmgDir, new RegExp(`_${version}_universal\\.dmg$`, "i")) ||
    findFile(macosDmgDir, /\.dmg$/i);

  const exePath =
    customExe ||
    findFile(windowsBundleDir, new RegExp(`_${version}_.*-setup\\.exe$`, "i")) ||
    findFile(windowsBundleDir, /-setup\\.exe$/i);

  const variants = {};

  if (dmgPath && existsSync(dmgPath)) {
    console.log(`  ✓ 找到 macOS DMG 安装包: ${dmgPath}`);
    const macFiles = [];

    // 检查 macOS 更新包与签名
    const tarGzPath = findFile(macosAppDir, /\.app\.tar\.gz$/i);
    if (tarGzPath && existsSync(tarGzPath)) {
      macFiles.push({ data: tarGzPath, name: basename(tarGzPath) });
      const tarGzSig = `${tarGzPath}.sig`;
      if (existsSync(tarGzSig)) {
        macFiles.push({ data: tarGzSig, name: basename(tarGzSig) });
      }
    }

    variants["macos-universal"] = {
      main: dmgPath,
      files: macFiles,
    };
  } else {
    console.log(`  ℹ 未找到当前版本 macOS DMG 安装包 (路径: ${macosDmgDir})`);
  }

  if (exePath && existsSync(exePath)) {
    console.log(`  ✓ 找到 Windows EXE 安装包: ${exePath}`);
    const winFiles = [];
    const exeSig = `${exePath}.sig`;
    if (existsSync(exeSig)) {
      winFiles.push({ data: exeSig, name: basename(exeSig) });
    }

    variants["windows-x64"] = {
      main: exePath,
      files: winFiles,
    };
  } else {
    console.log(`  ℹ 未找到当前版本 Windows EXE 安装包 (路径: ${windowsBundleDir})`);
  }

  if (Object.keys(variants).length === 0) {
    throw new Error(
      "未找到任何可上传的安装包，请先运行 `npm run release:macos` 或 `npm run release:windows` 进行构建"
    );
  }

  // 3. 配置发布目标
  const targetConfig = {};
  if (targetNames.includes("github")) {
    targetConfig.github = {
      repo: "yArna/OpenUPX-Installer",
      user: "yArna",
      draft: isDraft,
    };
  }
  if (targetNames.includes("lanzou")) {
    targetConfig.lanzou = {
      path: folderId,
    };
  }

  // 4. 加载并执行 QRls
  const { qrls } = await loadQRls();

  const result = await qrls({
    name: "OpenUXP Installer",
    version,
    buildVersion: version,
    variants,
    target: targetConfig,
    dry: isDryRun,
    force: isForce,
    verbose: true,
  });

  // 5. 生成/更新官网 docs/download.json
  const docsDir = join(projectRoot, "docs");
  if (!existsSync(docsDir)) {
    mkdirSync(docsDir, { recursive: true });
  }
  const docsDownloadJsonPath = join(docsDir, "download.json");

  // 组织 download.json 格式（无论 dry-run 与否，均生成对齐格式供官网使用）
  const downloadManifest = {
    name: "OpenUXP Installer",
    version,
    buildVersion: version,
    publishedAt: result.publishedAt || new Date().toISOString(),
    variants: {},
  };

  const variantKeys = Object.keys(variants);
  for (const vKey of variantKeys) {
    const mirrors = [];
    let mainName = "";
    let mainSize = 0;
    let mainSha256 = "";
    let primaryUrl = "";

    // GitHub 镜像
    if (result.github?.variants?.[vKey]?.main) {
      const gMain = result.github.variants[vKey].main;
      mainName = gMain.name;
      mainSize = gMain.size;
      mainSha256 = gMain.sha256;
      primaryUrl = gMain.url;
      mirrors.push({
        target: "github",
        url: gMain.url,
      });
    }

    // 蓝奏云镜像
    if (result.lanzou?.variants?.[vKey]?.main) {
      const lMain = result.lanzou.variants[vKey].main;
      if (!mainName) mainName = lMain.name;
      if (!mainSize) mainSize = lMain.size;
      if (!mainSha256) mainSha256 = lMain.sha256;
      if (!primaryUrl) primaryUrl = lMain.url;
      mirrors.push({
        target: "lanzou",
        url: lMain.url,
      });
    }

    // 如果是 dry-run，填充预估地址
    if (isDryRun && mirrors.length === 0) {
      const fallbackName = basename(variants[vKey].main);
      primaryUrl = `https://github.com/yArna/OpenUPX-Installer/releases/download/${tag}/${encodeURIComponent(fallbackName)}`;
      mirrors.push(
        { target: "github", url: primaryUrl },
        { target: "lanzou", url: `https://ww.lanzou.com/mock_${encodeURIComponent(fallbackName)}` }
      );
    }

    if (mirrors.length > 0) {
      downloadManifest.variants[vKey] = {
        name: mainName || basename(variants[vKey].main),
        size: mainSize || 0,
        sha256: mainSha256 || undefined,
        primaryUrl: primaryUrl || mirrors[0].url,
        mirrors,
      };
    }
  }

  writeFileSync(
    docsDownloadJsonPath,
    `${JSON.stringify(downloadManifest, null, 2)}\n`,
    "utf8"
  );
  console.log(`\n📄 已同步生成官网清单: ${docsDownloadJsonPath}`);

  // 6. 输出总结
  console.log("\n=================== 📦 发布结果汇总 ===================");
  if (result.github?.homepage) {
    console.log(`  🐙 GitHub Release: ${result.github.homepage}`);
  }
  if (result.lanzou?.homepage) {
    console.log(`  ☁️ 蓝奏云页面: ${result.lanzou.homepage}`);
  }

  for (const [vKey, vData] of Object.entries(downloadManifest.variants)) {
    console.log(`\n  变体 [${vKey}]: ${vData.name} (${(vData.size / 1024 / 1024).toFixed(2)} MB)`);
    for (const m of vData.mirrors) {
      console.log(`    ↳ [${m.target}]: ${m.url}`);
    }
  }
  console.log("========================================================\n");

  return result;
}

main().catch((err) => {
  console.error(`\n❌ 发布脚本执行失败: ${err.message || err}`);
  process.exit(1);
});
