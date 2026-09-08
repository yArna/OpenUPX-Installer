#!/usr/bin/env bun

import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const projectRoot = resolve(__dirname, "..");

// 优先通过标准依赖加载 @moonvy/publish，回退至本地 MoonvyRoot vendors 源码路径
async function loadMoonvyPublish() {
  try {
    return await import("@moonvy/publish");
  } catch {
    const fallbackPath = "/Users/yarna/Project/MoonvyRoot/vendors/MoonvyPublish/src/index.ts";
    return await import(fallbackPath);
  }
}

async function main() {
  const { publishDir, refreshCDN } = await loadMoonvyPublish();

  const serverName = "moonvy.com";
  const localDir = resolve(projectRoot, "docs");
  const remotePath = "apps/upx-installer";
  const publicUrl = `https://moonvy.com/${remotePath}/`;

  console.log(`\n🚀 开始上传官网到 ${serverName}/${remotePath}...`);
  console.log(`  📁 本地目录: ${localDir}`);
  console.log(`  🌐 目标地址: ${publicUrl}\n`);

  // 1. 上传 docs 目录到阿里云 OSS
  await publishDir(serverName, localDir, remotePath, {
    skipSourcemapFiles: true,
    skipFileNames: [".DS_Store"],
  });

  // 2. 刷新 CDN 节点缓存
  console.log("\n🔄 正在刷新阿里云 CDN 缓存...");
  try {
    await refreshCDN(
      [
        publicUrl,
        `${publicUrl}index.html`,
        `${publicUrl}styles.css`,
        `${publicUrl}download.js`,
        `${publicUrl}download.json`,
      ],
      "File"
    );
    await refreshCDN(publicUrl, "Dir");
    console.log("  ✓ CDN 缓存刷新完成");
  } catch (err) {
    console.warn(`  ⚠️ CDN 刷新提示: ${err instanceof Error ? err.message : String(err)}`);
  }

  console.log("\n✨ 官网发布成功！");
  console.log(`🔗 访问链接: ${publicUrl}\n`);
}

main().catch((err) => {
  console.error(`\n❌ 官网发布失败: ${err instanceof Error ? err.message : String(err)}`);
  process.exit(1);
});
