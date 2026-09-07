import { Fragment, useEffect, useMemo, useRef, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
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
import type { Environment, Host, InstallResult, PluginPackage, SideloadPreflight } from "./types";

type Stage = "empty" | "ready" | "installing" | "success" | "error";
type UpdateStage = "hidden" | "available" | "downloading" | "restarting" | "error";

const GITHUB_URL = "https://github.com/yArna/OpenUPX-Installer";
const Moonvy_URL = "https://moonvy.com/?homepage";
let automaticUpdateCheck: Promise<Update | null> | null = null;

const isTauriRuntime = () =>
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

const formatBytes = (bytes: number) => {
  if (!bytes) return "—";
  const units = ["B", "KB", "MB", "GB"];
  const index = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  return `${(bytes / 1024 ** index).toFixed(index ? 1 : 0)} ${units[index]}`;
};

const uniqueHostLabels = (hosts: Host[]) => {
  const labels: string[] = [];
  for (const host of hosts) {
    if (!labels.includes(host.label)) labels.push(host.label);
  }
  return labels;
};

const sideloadHostLabels = (hosts: Host[]) => {
  const labels: string[] = [];
  const seen = new Set<string>();
  for (const host of hosts) {
    const app = host.app.toUpperCase();
    const key =
      app === "PS" || app === "PHSP"
        ? "PS"
        : app === "XD"
          ? "XD"
          : app === "AI" || app === "ILST"
            ? "AI"
            : "";
    if (!key || seen.has(key)) continue;
    seen.add(key);
    labels.push(host.label);
  }
  return labels;
};

const joinLabels = (labels: string[], fallback: string) => {
  if (!labels.length) return fallback;
  if (labels.length === 1) return labels[0];
  if (labels.length === 2) return `${labels[0]} 和 ${labels[1]}`;
  return `${labels.slice(0, -1).join("、")} 和 ${labels[labels.length - 1]}`;
};

const emptyPreflight = (message: string): SideloadPreflight => ({
  supported: false,
  ready: false,
  pluginDirectory: "",
  registryPath: "",
  registryPaths: [],
  hostLabels: [],
  issues: [{ path: "", message, hint: "请检查安装包后重试。" }],
  kind: "uxp",
  debugModeEnabled: false,
  debugModeKeys: [],
});

const fitWindowToContent = async () => {
  if (!isTauriRuntime()) return;
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
  const [installMode, setInstallMode] = useState<"official" | "sideload">("sideload");
  const [installingMessage, setInstallingMessage] = useState("");
  const [activationPending, setActivationPending] = useState(false);
  const [updateStage, setUpdateStage] = useState<UpdateStage>("hidden");
  const [updateVersion, setUpdateVersion] = useState("");
  const [updateProgress, setUpdateProgress] = useState(0);
  const [updateMessage, setUpdateMessage] = useState("");
  const [appVersion, setAppVersion] = useState("");
  const preflightRequest = useRef(0);
  const pendingUpdate = useRef<Update | null>(null);

  const loadSideloadPreflight = async (path: string) => {
    const request = ++preflightRequest.current;
    try {
      const next = await invoke<SideloadPreflight>("check_sideload", { path });
      if (request === preflightRequest.current) setPreflight(next);
    } catch (error) {
      if (request !== preflightRequest.current) return;
      setPreflight(emptyPreflight(String(error)));
    }
  };

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
      if (next.kind === "cep") {
        setInstallMode("sideload");
        void loadSideloadPreflight(next.path);
      } else if (installMode === "sideload") {
        void loadSideloadPreflight(next.path);
      }
    } catch (error) {
      setPkg(null);
      setStage("error");
      setMessage(String(error));
      setDetails("");
    }
  };

  useEffect(() => {
    if (!isTauriRuntime()) return;
    getVersion().then(setAppVersion).catch(() => undefined);
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
          const path = event.payload.paths.find((item) => {
            const lower = item.toLowerCase();
            return lower.endsWith(".ccx") || lower.endsWith(".xdx") || lower.endsWith(".zxp");
          });
          if (path) void inspect(path);
          else {
            setStage("error");
            setMessage("请选择一个 .ccx、.xdx 或 .zxp 安装包");
          }
        }
      })
      .then((fn) => {
        unlisten = fn;
      })
      .catch(() => undefined);
    return () => unlisten?.();
  }, []);

  useEffect(() => {
    if (!isTauriRuntime()) return;
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
    setMessage("");
    setDetails("");
    setPreflight(null);
    setInstallingMessage("正在等待 Adobe 官方安装服务完成，最长可能需要 5 分钟…");
    setActivationPending(false);
    try {
      const result = await invoke<InstallResult>("install_ccx", { path: pkg.path });
      setStage(result.success ? "success" : "error");
      setMessage(result.message);
      setDetails(result.details ?? "");
      setActivationPending(Boolean(result.activationPending));
      setInstallingMessage("");
      if (!result.success && result.canSideLoad) {
        try {
          setPreflight(await invoke<SideloadPreflight>("check_sideload", { path: pkg.path }));
        } catch (error) {
          setPreflight(emptyPreflight(String(error)));
        }
      }
    } catch (error) {
      setStage("error");
      setMessage(String(error));
      setInstallingMessage("");
    }
  };

  const sideload = async () => {
    if (!pkg || !preflight?.ready) return;
    setStage("installing");
    setInstallMode("sideload");
    setMessage("");
    setDetails("");
    setInstallingMessage(
      pkg.kind === "cep"
        ? "正在解压 CEP 扩展并开启调试模式…"
        : `正在安全解压插件并更新 ${joinLabels(
            preflight?.hostLabels?.length ? preflight.hostLabels : sideloadHostLabels(pkg.hosts),
            "Photoshop、Adobe XD 或 Illustrator",
          )} 注册信息…`,
    );
    try {
      const result = await invoke<InstallResult>("sideload_ccx", { path: pkg.path });
      setStage(result.success ? "success" : "error");
      setMessage(result.message);
      setDetails(result.details ?? "");
      setActivationPending(Boolean(result.activationPending));
      setInstallingMessage("");
    } catch (error) {
      setStage("error");
      setMessage(String(error));
      setInstallingMessage("");
    }
  };

  const recheckPermissions = async () => {
    if (!pkg) return;
    void loadSideloadPreflight(pkg.path);
  };

  const chooseInstallMode = (mode: "official" | "sideload") => {
    setInstallMode(mode);
    setMessage("");
    setDetails("");
    if (mode === "official" || !pkg) {
      preflightRequest.current += 1;
      setPreflight(null);
      return;
    }
    void loadSideloadPreflight(pkg.path);
  };

  const reset = () => {
    setPkg(null);
    setStage("empty");
    setMessage("");
    setDetails("");
    setPreflight(null);
    preflightRequest.current += 1;
    setInstallMode("sideload");
    setInstallingMessage("");
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
    if (environment.creativeCloudFound) return { tone: "warn", text: "Creative Cloud 已安装，但未找到安装服务" };
    return { tone: "warn", text: "未找到 Creative Cloud Desktop" };
  }, [environment]);

  const isCep = pkg?.kind === "cep";
  const sideloadSummary = useMemo(
    () =>
      joinLabels(
        preflight?.hostLabels?.length
          ? preflight.hostLabels
          : pkg?.kind === "cep"
            ? uniqueHostLabels(pkg.hosts)
            : pkg
              ? sideloadHostLabels(pkg.hosts)
              : [],
        pkg?.kind === "cep" ? "兼容的 Adobe 应用" : "Photoshop、Adobe XD 或 Illustrator",
      ),
    [pkg, preflight],
  );
  const officialSummary = useMemo(
    () => joinLabels(pkg ? uniqueHostLabels(pkg.hosts) : [], "兼容的 Adobe 应用"),
    [pkg],
  );
  const activeHostSummary = installMode === "sideload" ? sideloadSummary : officialSummary;
  const registryPaths =
    preflight?.registryPaths?.length
      ? preflight.registryPaths
      : preflight?.registryPath
        ? [preflight.registryPath]
        : [];

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
            <h2>{dragging ? "松开以载入安装包" : "拖放 CCX / XDX / ZXP 文件到这里"}</h2>
            <p>或点击从电脑中选择</p>
            <span className="browse">
              <FolderOpen size={17} />
              选择文件
            </span>
            <small>支持 UXP .ccx、Adobe XD .xdx 与 CEP .zxp</small>
          </button>
        )}

        {pkg && stage !== "success" && (
          <div className="package-card" aria-busy={stage === "installing"}>
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
                <strong>
                  {pkg.kind === "cep"
                    ? pkg.manifestVersion
                      ? `CEP v${pkg.manifestVersion}`
                      : "CEP"
                    : pkg.kind === "xdx"
                      ? pkg.manifestVersion
                        ? `XDX · Manifest v${pkg.manifestVersion}`
                        : "XDX"
                      : pkg.manifestVersion
                        ? `UXP · Manifest v${pkg.manifestVersion}`
                        : "UXP"}
                </strong>
              </div>
              <div>
                <span>插件 ID</span>
                <strong title={pkg.pluginId}>{pkg.pluginId || "未提供"}</strong>
              </div>
            </div>

            <div className="install-method">
              <span className="section-label">安装方式</span>
              <div
                className={`install-method-options ${isCep ? "single" : ""}`}
                role="radiogroup"
                aria-label="安装方式"
              >
                <button
                  type="button"
                  role="radio"
                  aria-checked={installMode === "sideload"}
                  className={`install-method-option ${installMode === "sideload" ? "active" : ""}`}
                  onClick={() => chooseInstallMode("sideload")}
                  disabled={stage === "installing"}
                >
                  <strong>{isCep ? "CEP 侧载" : "侧载安装"}</strong>
                  <span>{isCep ? "安装到 CEP 用户扩展目录" : `安装到 ${sideloadSummary} 用户目录`}</span>
                </button>
                {!isCep && (
                  <button
                    type="button"
                    role="radio"
                    aria-checked={installMode === "official"}
                    className={`install-method-option ${installMode === "official" ? "active" : ""}`}
                    onClick={() => chooseInstallMode("official")}
                    disabled={stage === "installing"}
                  >
                    <strong>官方安装</strong>
                    <span>通过 Adobe Creative Cloud 安装</span>
                  </button>
                )}
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

            {installMode === "official" && !environment?.installerFound && (
              <div className="notice warning">
                <AlertTriangle />
                <div>
                  <strong>
                    {environment?.creativeCloudFound
                      ? "未找到 Adobe 安装服务"
                      : "需要 Creative Cloud Desktop"}
                  </strong>
                  <p>
                    {environment?.creativeCloudFound
                      ? "请重新启动或更新 Creative Cloud Desktop 后重试。"
                      : "安装前请先安装或更新 Adobe Creative Cloud Desktop。"}
                  </p>
                </div>
              </div>
            )}

            {installMode === "sideload" && !preflight && stage !== "installing" && (
              <div className="notice warning" aria-live="polite">
                <LoaderCircle className="spin" />
                <div>
                  <strong>正在检查侧载环境</strong>
                  <p>
                    {isCep
                      ? "正在检查 CEP 扩展目录权限，并确认调试模式状态。"
                      : `正在检查插件目录和 ${sideloadSummary} 注册文件的读写权限。`}
                  </p>
                </div>
              </div>
            )}

            {stage === "installing" && (
              <div className="notice warning" aria-live="polite">
                <LoaderCircle className="spin" />
                <div>
                  <strong>{installMode === "sideload" ? "正在侧载插件" : "正在安装插件"}</strong>
                  <p>{installingMessage}</p>
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

            {installMode === "sideload" && stage !== "installing" && preflight && (
              <div className={`sideload-panel ${preflight.ready ? "ready" : "blocked"}`}>
                <div className="sideload-heading">
                  <ShieldCheck />
                  <div>
                    <strong>
                      {preflight.ready
                        ? "侧载环境已准备就绪"
                        : preflight.supported
                          ? "侧载安装需要文件权限"
                          : "当前安装包不支持侧载"}
                    </strong>
                    <p>
                      {preflight.ready
                        ? isCep
                          ? "扩展将安装到用户级 CEP 目录。安装时会自动开启调试模式，未签名扩展才能加载。"
                          : `插件将安装到 ${sideloadSummary} 的用户级 UXP 目录，不经过 Creative Cloud。`
                        : preflight.supported
                          ? "OpenUXP Installer 当前无法安全写入以下位置。"
                          : "侧载目前仅支持 manifest 中声明 Photoshop（PS）、Adobe XD 或 Illustrator（AI）的插件。"}
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
                      <span>{isCep ? "扩展目录" : "插件目录"}</span>
                      <code>{preflight.pluginDirectory}</code>
                      {isCep ? (
                        <>
                          <span>调试模式</span>
                          <code>
                            {preflight.debugModeEnabled
                              ? `已开启 ${preflight.debugModeKeys.join("、")}`
                              : `安装时自动开启 ${preflight.debugModeKeys.join("、")}`}
                          </code>
                        </>
                      ) : (
                        registryPaths.map((path) => (
                          <Fragment key={path}>
                            <span>注册文件</span>
                            <code>{path}</code>
                          </Fragment>
                        ))
                      )}
                    </div>
                    <p className="sideload-warning">
                      <AlertTriangle />
                      {isCep
                        ? "未签名 CEP 扩展需要 PlayerDebugMode。安装会写入当前用户的 CSXS 调试偏好，并在重启 Adobe 应用后生效。"
                        : "侧载会绕过 Creative Cloud 的安装确认，且插件不会由 Creative Cloud 管理。"}
                    </p>
                  </>
                )}
              </div>
            )}

            <button
              className="primary-action"
              onClick={() => void (installMode === "sideload" ? sideload() : install())}
              disabled={stage === "installing" || (installMode === "sideload" && !preflight?.ready)}
            >
              {stage === "installing" ? (
                <>
                  <LoaderCircle className="spin" />
                  {installMode === "sideload" ? "正在侧载…" : "正在安装…"}
                </>
              ) : (
                <>
                  {installMode === "sideload" ? "立即侧载" : "立即安装"}
                  <ChevronRight />
                </>
              )}
            </button>
            <div className="trust">
              <ShieldCheck size={15} />{" "}
              {installMode === "sideload"
                ? isCep
                  ? "安装时会自动开启 CEP 调试模式（PlayerDebugMode）"
                  : `侧载安装会创建可恢复的 ${sideloadSummary} 注册文件备份`
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
                ? `请完全退出并重新打开 ${activeHostSummary}，插件将在启动时启用。`
                : `插件已就绪，打开 ${activeHostSummary} 即可使用。`}
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
          · <span className="app-version">v{appVersion || "—"}</span>
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
