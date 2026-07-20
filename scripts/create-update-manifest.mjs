import { basename } from "node:path";
import { readFileSync, writeFileSync } from "node:fs";

const [version, tag, artifactPath, signaturePath, outputPath, windowsArtifactPath, windowsSignaturePath] = process.argv.slice(2);
if (![version, tag, artifactPath, signaturePath, outputPath].every(Boolean)) {
  console.error("用法：create-update-manifest.mjs <version> <tag> <artifact> <signature> <output>");
  process.exit(1);
}

const repository = "https://github.com/yArna/OpenUPX-Installer";
const artifactName = basename(artifactPath);
const downloadUrl = `${repository}/releases/download/${encodeURIComponent(tag)}/${encodeURIComponent(artifactName)}`;
const signature = readFileSync(signaturePath, "utf8").trim();
const platform = { signature, url: downloadUrl };

const manifest = {
  version,
  notes: `OpenUXP Installer ${version}`,
  pub_date: new Date().toISOString(),
  platforms: {
    "darwin-aarch64": platform,
    "darwin-x86_64": platform,
  },
};

if (windowsArtifactPath || windowsSignaturePath) {
  if (!windowsArtifactPath || !windowsSignaturePath) {
    console.error("Windows 更新包和签名必须同时提供");
    process.exit(1);
  }
  manifest.platforms["windows-x86_64"] = {
    signature: readFileSync(windowsSignaturePath, "utf8").trim(),
    url: `${repository}/releases/download/${encodeURIComponent(tag)}/${encodeURIComponent(basename(windowsArtifactPath))}`,
  };
}

writeFileSync(outputPath, `${JSON.stringify(manifest, null, 2)}\n`, { mode: 0o644 });
console.log(`已生成更新清单：${outputPath}`);
