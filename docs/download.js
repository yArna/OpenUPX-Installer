/**
 * OpenUXP Installer 官网动态下载驱动
 * 解析由 QRls 生成的 download.json 清单，实现：
 * 1. 自动检测用户系统平台（macOS vs Windows）
 * 2. 动态加载最新安装包版本、文件大小、SHA256
 * 3. 动态配置 GitHub 与 蓝奏云 多镜像高速下载
 */

(function () {
  const FALLBACK_RELEASE_URL = "https://github.com/yArna/OpenUPX-Installer/releases/latest";

  function formatBytes(bytes) {
    if (!bytes || bytes <= 0) return "";
    const mb = bytes / (1024 * 1024);
    if (mb >= 1) return mb.toFixed(1) + " MB";
    const kb = bytes / 1024;
    return Math.round(kb) + " KB";
  }

  function detectOS() {
    const ua = navigator.userAgent.toLowerCase();
    if (ua.includes("mac") || ua.includes("iphone") || ua.includes("ipad")) {
      return "mac";
    }
    if (ua.includes("win")) {
      return "windows";
    }
    if (ua.includes("linux")) {
      return "linux";
    }
    return "unknown";
  }

  function getMirrorUrl(variant, target) {
    if (!variant) return null;
    if (variant.mirrors && variant.mirrors.length > 0) {
      const match = variant.mirrors.find((m) => m.target.toLowerCase() === target.toLowerCase());
      if (match && match.url) return match.url;
    }
    return target === "github" ? variant.primaryUrl : null;
  }

  async function fetchDownloadManifest() {
    // 优先读取同目录下的 download.json（带时间戳避免缓存）
    try {
      const res = await fetch("./download.json?t=" + Date.now());
      if (res.ok) {
        return await res.json();
      }
    } catch (_) {}

    // 备用：从 GitHub Raw 读取
    try {
      const fallbackUrl = "https://raw.githubusercontent.com/yArna/OpenUPX-Installer/main/docs/download.json";
      const res = await fetch(fallbackUrl);
      if (res.ok) {
        return await res.json();
      }
    } catch (_) {}

    return null;
  }

  function renderDownload(manifest) {
    const os = detectOS();
    const macVariant = manifest?.variants?.["macos-universal"] || manifest?.variants?.["macos"];
    const winVariant = manifest?.variants?.["windows-x64"] || manifest?.variants?.["windows"];
    const version = manifest?.version ? `v${manifest.version}` : "";

    const isMac = os === "mac";
    const currentVariant = isMac ? macVariant : winVariant;
    const currentOSName = isMac ? "macOS" : "Windows";
    const otherOSName = isMac ? "Windows" : "macOS";
    const otherVariant = isMac ? winVariant : macVariant;

    // 1. 更新导航栏下载按钮
    const navBtn = document.getElementById("nav-download-btn");
    if (navBtn && currentVariant) {
      const currentUrl = getMirrorUrl(currentVariant, "github") || currentVariant.primaryUrl;
      navBtn.href = currentUrl;
      navBtn.title = `下载 ${manifest.name || "OpenUXP Installer"} (${currentOSName})`;
      if (version) {
        navBtn.textContent = `下载 ${version}`;
      }
    }

    // 2. 更新 Hero 区域主下载按钮
    const heroBtn = document.getElementById("hero-download-btn");
    const heroMeta = document.getElementById("hero-download-meta");
    if (heroBtn && currentVariant) {
      const currentUrl = getMirrorUrl(currentVariant, "github") || currentVariant.primaryUrl;
      const sizeStr = formatBytes(currentVariant.size);
      heroBtn.href = currentUrl;
      heroBtn.setAttribute("download", currentVariant.name || "");
      heroBtn.innerHTML = `
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
          <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
          <polyline points="7 10 12 15 17 10" />
          <line x1="12" x2="12" y1="15" y2="3" />
        </svg>
        <span>下载 ${currentOSName} 版${sizeStr ? ` (${sizeStr})` : ""}</span>
      `;

      if (heroMeta) {
        const lanzouUrl = getMirrorUrl(currentVariant, "lanzou");
        const otherUrl = otherVariant
          ? getMirrorUrl(otherVariant, "github") || otherVariant.primaryUrl
          : null;

        let metaHtml = `<span class="device-pill">已匹配当前系统: ${isMac ? "macOS (通用二进制)" : "Windows 64位"}</span>`;
        if (lanzouUrl) {
          metaHtml += ` <a class="hero-sublink" href="${lanzouUrl}" target="_blank" rel="noopener noreferrer">☁️ 蓝奏云国内高速</a>`;
        }
        if (otherUrl) {
          metaHtml += ` · <a class="hero-sublink" href="#download">切换至 ${otherOSName} 版</a>`;
        }
        heroMeta.innerHTML = metaHtml;
      }
    }

    // 3. 渲染底部双平台卡片列表 (#download-platforms)
    const platformsContainer = document.getElementById("download-platforms");
    if (platformsContainer && (macVariant || winVariant)) {
      let cardsHtml = "";

      // macOS 卡片
      if (macVariant) {
        const macGhUrl = getMirrorUrl(macVariant, "github") || macVariant.primaryUrl;
        const macLanzouUrl = getMirrorUrl(macVariant, "lanzou");
        const macSize = formatBytes(macVariant.size);
        cardsHtml += `
          <div class="platform-card ${isMac ? "is-recommended" : ""}">
            <div class="platform-card-header">
              <div class="platform-title-row">
                <span class="platform-icon">🍎</span>
                <div>
                  <h3>macOS 版</h3>
                  <p class="platform-subtitle">Apple 芯片与 Intel 芯片通用 (Universal DMG)</p>
                </div>
              </div>
              ${isMac ? '<span class="rec-badge">当前设备推荐</span>' : ""}
            </div>
            <div class="platform-file-info">
              <code>${macVariant.name}</code>
              <span class="file-size">${macSize}</span>
            </div>
            <div class="platform-actions">
              <a class="btn btn-primary" href="${macGhUrl}" target="_blank" rel="noopener noreferrer">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" x2="12" y1="15" y2="3"/></svg>
                GitHub 下载
              </a>
              ${
                macLanzouUrl
                  ? `<a class="btn btn-secondary" href="${macLanzouUrl}" target="_blank" rel="noopener noreferrer">
                      ☁️ 蓝奏云国内镜像
                    </a>`
                  : ""
              }
            </div>
            ${
              macVariant.sha256
                ? `<div class="platform-hash" title="点击复制 SHA256" onclick="navigator.clipboard.writeText('${macVariant.sha256}');alert('已复制 SHA256 指纹');">
                    <span>SHA256: </span><code>${macVariant.sha256.slice(0, 16)}...</code>
                   </div>`
                : ""
            }
          </div>
        `;
      }

      // Windows 卡片
      if (winVariant) {
        const winGhUrl = getMirrorUrl(winVariant, "github") || winVariant.primaryUrl;
        const winLanzouUrl = getMirrorUrl(winVariant, "lanzou");
        const winSize = formatBytes(winVariant.size);
        cardsHtml += `
          <div class="platform-card ${!isMac ? "is-recommended" : ""}">
            <div class="platform-card-header">
              <div class="platform-title-row">
                <span class="platform-icon">🪟</span>
                <div>
                  <h3>Windows 版</h3>
                  <p class="platform-subtitle">Windows 10 / 11 (64位安装包)</p>
                </div>
              </div>
              ${!isMac ? '<span class="rec-badge">当前设备推荐</span>' : ""}
            </div>
            <div class="platform-file-info">
              <code>${winVariant.name}</code>
              <span class="file-size">${winSize}</span>
            </div>
            <div class="platform-actions">
              <a class="btn btn-primary" href="${winGhUrl}" target="_blank" rel="noopener noreferrer">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" x2="12" y1="15" y2="3"/></svg>
                GitHub 下载
              </a>
              ${
                winLanzouUrl
                  ? `<a class="btn btn-secondary" href="${winLanzouUrl}" target="_blank" rel="noopener noreferrer">
                      ☁️ 蓝奏云国内镜像
                    </a>`
                  : ""
              }
            </div>
            ${
              winVariant.sha256
                ? `<div class="platform-hash" title="点击复制 SHA256" onclick="navigator.clipboard.writeText('${winVariant.sha256}');alert('已复制 SHA256 指纹');">
                    <span>SHA256: </span><code>${winVariant.sha256.slice(0, 16)}...</code>
                   </div>`
                : ""
            }
          </div>
        `;
      }

      platformsContainer.innerHTML = cardsHtml;
    }
  }

  // 页面加载完成后启动
  document.addEventListener("DOMContentLoaded", async () => {
    const manifest = await fetchDownloadManifest();
    if (manifest) {
      renderDownload(manifest);
    }
  });
})();
