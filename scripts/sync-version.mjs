import { readFile, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import path from "node:path";

const projectDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const packagePath = path.join(projectDir, "package.json");
const packageLockPath = path.join(projectDir, "package-lock.json");
const tauriConfigPath = path.join(projectDir, "src-tauri", "tauri.conf.json");
const cargoManifestPath = path.join(projectDir, "src-tauri", "Cargo.toml");
const cargoLockPath = path.join(projectDir, "src-tauri", "Cargo.lock");
const checkOnly = process.argv.includes("--check");
const semverPattern = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$/;

const packageJson = JSON.parse(await readFile(packagePath, "utf8"));
const packageLock = JSON.parse(await readFile(packageLockPath, "utf8"));
const tauriConfig = JSON.parse(await readFile(tauriConfigPath, "utf8"));
const cargoManifest = await readFile(cargoManifestPath, "utf8");
const cargoLock = await readFile(cargoLockPath, "utf8");
const packageVersion = packageJson.version;

if (typeof packageVersion !== "string" || !semverPattern.test(packageVersion)) {
  console.error(`错误：package.json 中的版本号无效：${String(packageVersion)}`);
  process.exit(1);
}

const lockRoot = packageLock.packages?.[""];
const cargoManifestVersionMatch = cargoManifest.match(
  /^\[package\]\s*\n[\s\S]*?^version\s*=\s*"([^"]+)"/m,
);
const cargoLockVersionMatch = cargoLock.match(
  /(\[\[package\]\]\nname = "openuxp-installer"\nversion = ")([^"]+)(")/,
);

if (!cargoManifestVersionMatch || !cargoLockVersionMatch) {
  console.error("错误：无法读取 OpenUXP Installer 的 Cargo 版本");
  process.exit(1);
}

const tauriMatches = tauriConfig.version === packageVersion;
const lockMatches =
  packageLock.version === packageVersion &&
  (!lockRoot || lockRoot.version === packageVersion);
const cargoManifestMatches = cargoManifestVersionMatch[1] === packageVersion;
const cargoLockMatches = cargoLockVersionMatch[2] === packageVersion;

if (tauriMatches && lockMatches && cargoManifestMatches && cargoLockMatches) {
  console.log(`版本已一致：${packageVersion}`);
  process.exit(0);
}

if (checkOnly) {
  console.error(`错误：项目版本与 package.json（${packageVersion}）不一致：`);
  if (!tauriMatches) {
    console.error(`- src-tauri/tauri.conf.json：${String(tauriConfig.version)}`);
  }
  if (!lockMatches) {
    console.error(`- package-lock.json：${String(packageLock.version)}`);
  }
  if (!cargoManifestMatches) {
    console.error(`- src-tauri/Cargo.toml：${cargoManifestVersionMatch[1]}`);
  }
  if (!cargoLockMatches) {
    console.error(`- src-tauri/Cargo.lock：${cargoLockVersionMatch[2]}`);
  }
  console.error("请运行 npm run version:sync 完成同步。");
  process.exit(1);
}

if (!tauriMatches) {
  const previousVersion = tauriConfig.version;
  tauriConfig.version = packageVersion;
  await writeFile(tauriConfigPath, `${JSON.stringify(tauriConfig, null, 2)}\n`);
  console.log(
    `已将 src-tauri/tauri.conf.json 版本从 ${String(previousVersion)} 同步为 ${packageVersion}`,
  );
}

if (!lockMatches) {
  const previousVersion = packageLock.version;
  packageLock.version = packageVersion;
  if (lockRoot) {
    lockRoot.version = packageVersion;
  }
  await writeFile(packageLockPath, `${JSON.stringify(packageLock, null, 2)}\n`);
  console.log(
    `已将 package-lock.json 版本从 ${String(previousVersion)} 同步为 ${packageVersion}`,
  );
}

if (!cargoManifestMatches) {
  const updatedCargoManifest = cargoManifest.replace(
    cargoManifestVersionMatch[0],
    cargoManifestVersionMatch[0].replace(
      /^version\s*=\s*"[^"]+"/m,
      `version = "${packageVersion}"`,
    ),
  );
  await writeFile(cargoManifestPath, updatedCargoManifest);
  console.log(
    `已将 src-tauri/Cargo.toml 版本从 ${cargoManifestVersionMatch[1]} 同步为 ${packageVersion}`,
  );
}

if (!cargoLockMatches) {
  const updatedCargoLock = cargoLock.replace(
    cargoLockVersionMatch[0],
    `${cargoLockVersionMatch[1]}${packageVersion}${cargoLockVersionMatch[3]}`,
  );
  await writeFile(cargoLockPath, updatedCargoLock);
  console.log(
    `已将 src-tauri/Cargo.lock 版本从 ${cargoLockVersionMatch[2]} 同步为 ${packageVersion}`,
  );
}
