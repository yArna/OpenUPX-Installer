import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { LogicalSize } from "@tauri-apps/api/dpi";
import { currentMonitor, getCurrentWindow } from "@tauri-apps/api/window";
import { openUrl } from "@tauri-apps/plugin-opener";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";
import {
  AlertTriangle,
  Check,
  CheckCircle2,
  ChevronRight,
  Download,
  FolderOpen,
  Github,
  Info,
  LoaderCircle,
  PackageOpen,
  RotateCcw,
  ShieldCheck,
  X,
} from "lucide-react";
import type { Environment, InstallResult, PluginPackage, SideloadPreflight } from "./types";

type Stage = "empty" | "ready" | "installing" | "success" | "error";
type UpdateStage = "hidden" | "available" | "downloading" | "restarting" | "error";

const GITHUB_URL = "https://github.com/yArna/OpenUPX-Installer";
const Moonvy_URL = "https://moonvy.com/?homepage";
let automaticUpdateCheck: Promise<Update | null> | null = null;

const formatBytes = (bytes: number) => {
  if (!bytes) return "—";
  const units = ["B", "KB", "MB", "GB"];
  const index = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  return `${(bytes / 1024 ** index).toFixed(index ? 1 : 0)} ${units[index]}`;
};

const fitWindowToContent = async () => {
  const topbar = document.querySelector<HTMLElement>(".topbar");
  const workspace = document.querySelector<HTMLElement>(".workspace");
  const footer = document.querySelector<HTMLElement>("footer");
  if (!topbar || !workspace || !footer) return;

  const monitor = await currentMonitor();
  const workArea = monitor?.workArea.size.toLogical(monitor.scaleFactor);
  const maxHeight = workArea ? workArea.height - 48 : 900;
  const maxWidth = workArea ? workArea.width - 48 : 720;
  const contentHeight = topbar.offsetHeight + workspace.scrollHeight + footer.offsetHeight + 92;
  const width = Math.min(720, maxWidth);
  const height = Math.min(Math.max(Math.ceil(contentHeight), 450), maxHeight);
  await getCurrentWindow().setSize(new LogicalSize(width, height));
};

export default function App() {
  const [pkg, setPkg] = useState<PluginPackage | null>(null);
  const [environment, setEnvironment] = useState<Environment | null>(null);
  const [stage, setStage] = useState<Stage>("empty");
  const [dragging, setDragging] = useState(false);
  const [message, setMessage] = useState("");
  const [details, setDetails] = useState("");
  const [preflight, setPreflight] = useState<SideloadPreflight | null>(null);
  const [installMode, setInstallMode] = useState<"official" | "sideload">("official");
  const [activationPending, setActivationPending] = useState(false);
  const [updateStage, setUpdateStage] = useState<UpdateStage>("hidden");
  const [updateVersion, setUpdateVersion] = useState("");
  const [updateProgress, setUpdateProgress] = useState(0);
  const [updateMessage, setUpdateMessage] = useState("");
  const pendingUpdate = useRef<Update | null>(null);

  const inspect = async (path?: string) => {
    try {
      setMessage("");
      const next = path
        ? await invoke<PluginPackage>("inspect_ccx", { path })
        : await invoke<PluginPackage | null>("pick_ccx");
      if (!next) return;
      setPkg(next);
      setStage("ready");
      setPreflight(null);
    } catch (error) {
      setPkg(null);
      setStage("error");
      setMessage(String(error));
      setDetails("");
    }
  };

  useEffect(() => {
    invoke<Environment>("check_environment")
      .then(setEnvironment)
      .catch(() => undefined);
    let unlisten: (() => void) | undefined;
    getCurrentWebview()
      .onDragDropEvent((event) => {
        if (event.payload.type === "over") setDragging(true);
        if (event.payload.type === "leave") setDragging(false);
        if (event.payload.type === "drop") {
          setDragging(false);
          const path = event.payload.paths.find((item) => item.toLowerCase().endsWith(".ccx"));
          if (path) void inspect(path);
          else {
            setStage("error");
            setMessage("请选择一个 .ccx 安装包");
          }
        }
      })
      .then((fn) => {
        unlisten = fn;
      });
    return () => unlisten?.();
  }, []);

  useEffect(() => {
    let active = true;
    automaticUpdateCheck ??= check({ timeout: 12_000 });
    automaticUpdateCheck
      .then((update) => {
        if (!active || !update) return;
        pendingUpdate.current = update;
        setUpdateVersion(update.version);
        setUpdateStage("available");
      })
      .catch(() => undefined);
    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    const workspace = document.querySelector<HTMLElement>(".workspace");
    if (!workspace) return;
    let frame = 0;
    const scheduleFit = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        void fitWindowToContent().catch(() => undefined);
      });
    };
    const observer = new ResizeObserver(scheduleFit);
    observer.observe(workspace);
    scheduleFit();
    return () => {
      cancelAnimationFrame(frame);
      observer.disconnect();
    };
  }, []);

  const install = async () => {
    if (!pkg) return;
    setStage("installing");
    setInstallMode("official");
    setMessage("");
    setPreflight(null);
    setActivationPending(false);
    try {
      const result = await invoke<InstallResult>("install_ccx", { path: pkg.path });
      setStage(result.success ? "success" : "error");
      setMessage(result.message);
      setDetails(result.details ?? "");
      setActivationPending(Boolean(result.activationPending));
      if (!result.success && result.canSideLoad) {
        try {
          setPreflight(await invoke<SideloadPreflight>("check_sideload", { path: pkg.path }));
        } catch (error) {
          setPreflight({
            supported: false,
            ready: false,
            pluginDirectory: "",
            registryPath: "",
            issues: [{ path: "", message: String(error), hint: "请检查安装包后重试。" }],
          });
        }
      }
    } catch (error) {
      setStage("error");
      setMessage(String(error));
    }
  };

  const sideload = async () => {
    if (!pkg || !preflight?.ready) return;
    setStage("installing");
    setInstallMode("sideload");
    setMessage("");
    try {
      const result = await invoke<InstallResult>("sideload_ccx", { path: pkg.path });
      setStage(result.success ? "success" : "error");
      setMessage(result.message);
      setDetails(result.details ?? "");
      setActivationPending(Boolean(result.activationPending));
    } catch (error) {
      setStage("error");
      setMessage(String(error));
    }
  };

  const recheckPermissions = async () => {
    if (!pkg) return;
    try {
      setPreflight(await invoke<SideloadPreflight>("check_sideload", { path: pkg.path }));
    } catch (error) {
      setPreflight({
        supported: false,
        ready: false,
        pluginDirectory: "",
        registryPath: "",
        issues: [{ path: "", message: String(error), hint: "请检查安装包后重试。" }],
      });
    }
  };

  const reset = () => {
    setPkg(null);
    setStage("empty");
    setMessage("");
    setDetails("");
    setPreflight(null);
    setInstallMode("official");
    setActivationPending(false);
  };

  const installUpdate = async () => {
    const update = pendingUpdate.current;
    if (!update) return;
    setUpdateStage("downloading");
    setUpdateProgress(0);
    setUpdateMessage("");
    let downloaded = 0;
    let contentLength = 0;
    try {
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") contentLength = event.data.contentLength ?? 0;
        if (event.event === "Progress") {
          downloaded += event.data.chunkLength;
          if (contentLength)
            setUpdateProgress(Math.min(100, Math.round((downloaded / contentLength) * 100)));
        }
        if (event.event === "Finished") setUpdateProgress(100);
      });
      setUpdateStage("restarting");
      await relaunch();
    } catch (error) {
      setUpdateMessage(String(error));
      setUpdateStage("error");
    }
  };

  const status = useMemo(() => {
    if (!environment) return { tone: "muted", text: "正在检测 Adobe 环境…" };
    if (environment.installerFound) return { tone: "good", text: "Adobe 安装服务已就绪" };
    return { tone: "warn", text: "未找到 Adobe 安装服务" };
  }, [environment]);

  return (
    <main className="app-shell">
      <header className="topbar">
        <div className="brand">
          <div className="brand-mark">
            <img src="/icon.png" alt="" />
          </div>
          <div>
            <strong>OpenUXP</strong>
            <span>INSTALLER</span>
          </div>
        </div>
        <div className={`environment ${status.tone}`}>
          <span className="status-dot" />
          {status.text}
        </div>
      </header>

      <section className="workspace">
        {stage === "empty" && (
          <button className={`dropzone ${dragging ? "dragging" : ""}`} onClick={() => inspect()}>
            <div className="package-visual">
              <img src="/ccx-icon.png" alt="Adobe CCX 安装包" />
            </div>
            <h2>{dragging ? "松开以载入安装包" : "拖放 CCX 文件到这里"}</h2>
            <p>或点击从电脑中选择</p>
            <span className="browse">
              <FolderOpen size={17} />
              选择文件
            </span>
            <small>支持 Adobe UXP 的 .ccx 安装包</small>
          </button>
        )}

        {pkg && stage !== "success" && (
          <div className="package-card">
            <div className="package-heading">
              <div className="package-icon">
                <img src="/ccx-icon.png" alt="" />
              </div>
              <div className="package-title">
                <span className="label">准备安装</span>
                <h2>{pkg.name}</h2>
                <p>
                  {pkg.fileName} · {formatBytes(pkg.fileSize)}
                </p>
              </div>
              <button
                className="icon-button"
                onClick={reset}
                disabled={stage === "installing"}
                aria-label="移除文件"
              >
                <X />
              </button>
            </div>

            <div className="metadata">
              <div>
                <span>版本</span>
                <strong>{pkg.version || "未知"}</strong>
              </div>
              <div>
                <span>清单</span>
                <strong>{pkg.manifestVersion ? `Manifest v${pkg.manifestVersion}` : "UXP"}</strong>
              </div>
              <div>
                <span>插件 ID</span>
                <strong title={pkg.pluginId}>{pkg.pluginId || "未提供"}</strong>
              </div>
            </div>

            <div className="hosts">
              <span className="section-label">兼容应用</span>
              <div className="host-list">
                {pkg.hosts.length ? (
                  pkg.hosts.map((host) => (
                    <div className="host" key={`${host.app}-${host.minVersion}`}>
                      <img src={host.icon} alt="" />
                      <span>{host.label}</span>
                      {host.minVersion && <small>≥ {host.minVersion}</small>}
                      <Check size={16} />
                    </div>
                  ))
                ) : (
                  <div className="host unknown">
                    <Info size={18} />
                    安装包未声明宿主应用
                  </div>
                )}
              </div>
            </div>

            {!environment?.installerFound && (
              <div className="notice warning">
                <AlertTriangle />
                <div>
                  <strong>需要 Creative Cloud Desktop</strong>
                  <p>安装前请先安装或更新 Adobe Creative Cloud Desktop。</p>
                </div>
              </div>
            )}

            {stage === "error" && message && (
              <div className="notice error">
                <AlertTriangle />
                <div>
                  <strong>安装未完成</strong>
                  <p>{message}</p>
                  {details && (
                    <details>
                      <summary>查看技术信息</summary>
                      <pre>{details}</pre>
                    </details>
                  )}
                </div>
              </div>
            )}

            {stage === "error" && preflight && (
              <div className={`sideload-panel ${preflight.ready ? "ready" : "blocked"}`}>
                <div className="sideload-heading">
                  <ShieldCheck />
                  <div>
                    <strong>{preflight.ready ? "可尝试侧载安装" : "侧载安装需要文件权限"}</strong>
                    <p>
                      {preflight.ready
                        ? "官方安装失败后，可将插件安装到 Photoshop 的用户级 UXP 目录。"
                        : "OpenUXP Installer 当前无法安全写入以下位置。"}
                    </p>
                  </div>
                </div>
                {!preflight.ready && (
                  <div className="permission-list">
                    {preflight.issues.map((issue, index) => (
                      <div className="permission-issue" key={`${issue.path}-${index}`}>
                        <strong>{issue.message}</strong>
                        {issue.path && <code>{issue.path}</code>}
                        <p>{issue.hint}</p>
                      </div>
                    ))}
                    <button className="permission-recheck" onClick={recheckPermissions}>
                      <RotateCcw />
                      重新检查权限
                    </button>
                  </div>
                )}
                {preflight.ready && (
                  <>
                    <div className="sideload-paths">
                      <span>插件目录</span>
                      <code>{preflight.pluginDirectory}</code>
                      <span>注册文件</span>
                      <code>{preflight.registryPath}</code>
                    </div>
                    <p className="sideload-warning">
                      <AlertTriangle />
                      侧载会绕过 Creative Cloud 的安装确认，且插件不会由 Creative Cloud 管理。
                    </p>
                    <button className="sideload-action" onClick={sideload}>
                      使用侧载安装
                      <ChevronRight />
                    </button>
                  </>
                )}
              </div>
            )}

            <button className="primary-action" onClick={install} disabled={stage === "installing"}>
              {stage === "installing" ? (
                <>
                  <LoaderCircle className="spin" />
                  {installMode === "sideload" ? "正在侧载…" : "正在安装…"}
                </>
              ) : (
                <>
                  立即安装
                  <ChevronRight />
                </>
              )}
            </button>
            <div className="trust">
              <ShieldCheck size={15} />{" "}
              {installMode === "sideload"
                ? "侧载安装会创建可恢复的 Photoshop 注册文件备份"
                : "官方安装由 Adobe Unified Plugin Installer Agent 完成"}
            </div>
          </div>
        )}

        {!pkg && stage === "error" && (
          <div className="error-card">
            <AlertTriangle size={34} />
            <h2>无法读取安装包</h2>
            <p>{message}</p>
            <button className="secondary-action" onClick={reset}>
              <RotateCcw size={16} />
              重新选择
            </button>
          </div>
        )}

        {pkg && stage === "success" && (
          <div className="success-card">
            <div className="success-visual">
              <img src="/icon.png" alt="" />
              <span>
                <CheckCircle2 />
              </span>
            </div>
            <span className="label">{activationPending ? "等待启用" : "安装完成"}</span>
            <h2>{pkg.name}</h2>
            <p>{message || "插件已经可以在兼容的 Adobe 应用中使用。"}</p>
            {details && (
              <details className="success-details">
                <summary>查看安装信息</summary>
                <pre>{details}</pre>
              </details>
            )}
            <div className={`success-tip ${activationPending ? "pending" : ""}`}>
              <Info size={17} />
              {activationPending
                ? "请完全退出并重新打开 Photoshop，插件将在启动时启用。"
                : "插件已就绪，打开 Photoshop 即可使用。"}
            </div>
            <button className="secondary-action" onClick={reset}>
              <PackageOpen size={17} />
              安装另一个插件
            </button>
          </div>
        )}

        {updateStage !== "hidden" && (
          <aside className={`update-panel ${updateStage}`} aria-live="polite">
            <div className="update-icon">
              {updateStage === "downloading" || updateStage === "restarting" ? (
                <LoaderCircle className="spin" />
              ) : (
                <Download />
              )}
            </div>
            <div className="update-copy">
              <strong>
                {updateStage === "error" ? "更新失败" : `发现新版本 ${updateVersion}`}
              </strong>
              <p>
                {updateStage === "available" && "新版本已通过签名验证，可以立即更新。"}
                {updateStage === "downloading" &&
                  `正在下载并安装…${updateProgress ? ` ${updateProgress}%` : ""}`}
                {updateStage === "restarting" && "更新完成，正在重新启动…"}
                {updateStage === "error" && (updateMessage || "暂时无法完成更新，请稍后重试。")}
              </p>
              {updateStage === "downloading" && (
                <span className="update-progress">
                  <i style={{ width: `${updateProgress}%` }} />
                </span>
              )}
            </div>
            {updateStage === "available" && (
              <button className="update-action" onClick={installUpdate}>
                立即更新
              </button>
            )}
            {updateStage === "available" && (
              <button
                className="update-dismiss"
                onClick={() => setUpdateStage("hidden")}
                aria-label="稍后更新"
              >
                <X />
              </button>
            )}
            {updateStage === "error" && (
              <button className="update-action" onClick={() => setUpdateStage("available")}>
                重试
              </button>
            )}
          </aside>
        )}
      </section>

      <footer>
        <span>
          {" "}
          <button
            className="github-link"
            onClick={() => {
              void openUrl(Moonvy_URL);
            }}
            title="打开 Moonvy 主页"
          >
            Moonvy.com
          </button>{" "}
          · v0.1.0
        </span>
        <button
          className="github-link"
          onClick={() => {
            void openUrl(GITHUB_URL);
          }}
          title="打开 OpenUXP Installer GitHub 主页"
        >
          <Github />
          GitHub
        </button>
      </footer>
    </main>
  );
}
