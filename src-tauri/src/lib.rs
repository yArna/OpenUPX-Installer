use serde::Serialize;
use serde_json::{json, Value};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use zip::ZipArchive;

const CREATIVE_CLOUD_START_TIMEOUT: Duration = Duration::from_secs(30);
const UPIA_TIMEOUT: Duration = Duration::from_secs(5 * 60);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Host {
    app: String,
    label: String,
    min_version: Option<String>,
    icon: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PluginPackage {
    path: String,
    file_name: String,
    file_size: u64,
    name: String,
    version: String,
    plugin_id: Option<String>,
    hosts: Vec<Host>,
    manifest_version: Option<u64>,
    kind: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Environment {
    platform: String,
    installer_found: bool,
    installer_path: Option<String>,
    creative_cloud_found: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InstallResult {
    success: bool,
    message: String,
    details: Option<String>,
    can_side_load: bool,
    activation_pending: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PermissionIssue {
    path: String,
    message: String,
    hint: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SideloadPreflight {
    supported: bool,
    ready: bool,
    plugin_directory: String,
    registry_path: String,
    registry_paths: Vec<String>,
    host_labels: Vec<String>,
    issues: Vec<PermissionIssue>,
    kind: String,
    debug_mode_enabled: bool,
    debug_mode_keys: Vec<String>,
}

fn upia_candidates() -> Vec<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        vec![
            PathBuf::from("/Library/Application Support/Adobe/Adobe Desktop Common/RemoteComponents/UPI/UnifiedPluginInstallerAgent/UnifiedPluginInstallerAgent.app/Contents/MacOS/UnifiedPluginInstallerAgent"),
            PathBuf::from("/Library/Application Support/Adobe/Adobe Desktop Common/RemoteComponents/UPI/UnifiedPluginInstallerAgent/UnifiedPluginInstallerAgent"),
        ]
    }
    #[cfg(target_os = "windows")]
    {
        let relative = Path::new("Adobe/Adobe Desktop Common/RemoteComponents/UPI/UnifiedPluginInstallerAgent/UnifiedPluginInstallerAgent.exe");
        let mut paths = Vec::new();
        for variable in [
            "CommonProgramFiles",
            "CommonProgramW6432",
            "CommonProgramFiles(x86)",
        ] {
            if let Some(common) = std::env::var_os(variable) {
                paths.push(PathBuf::from(common).join(relative));
            }
        }
        paths.push(PathBuf::from(r"C:\Program Files\Common Files").join(relative));
        paths
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Vec::new()
    }
}

fn find_upia() -> Option<PathBuf> {
    upia_candidates().into_iter().find(|path| path.is_file())
}

fn creative_cloud_candidates() -> Vec<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        vec![
            PathBuf::from("/Applications/Adobe Creative Cloud/ACC/Creative Cloud.app"),
            PathBuf::from("/Applications/Utilities/Adobe Creative Cloud/ACC/Creative Cloud.app"),
        ]
    }
    #[cfg(target_os = "windows")]
    {
        let relative = Path::new("Adobe/Adobe Creative Cloud/ACC/Creative Cloud.exe");
        ["ProgramFiles", "ProgramFiles(x86)"]
            .into_iter()
            .filter_map(std::env::var_os)
            .map(PathBuf::from)
            .map(|root| root.join(relative))
            .collect()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Vec::new()
    }
}

fn process_list() -> String {
    #[cfg(target_os = "macos")]
    let output = Command::new("ps").args(["-ax", "-o", "command="]).output();
    #[cfg(target_os = "windows")]
    let output = Command::new("tasklist").args(["/NH"]).output();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    return String::new();

    output
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .unwrap_or_default()
}

fn creative_cloud_is_running() -> bool {
    let processes = process_list().to_ascii_lowercase();
    #[cfg(target_os = "macos")]
    return processes.contains("/creative cloud.app/contents/macos/creative cloud");
    #[cfg(target_os = "windows")]
    return processes
        .lines()
        .any(|line| line.starts_with("creative cloud.exe"));
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    false
}

fn photoshop_is_running() -> bool {
    let processes = process_list().to_ascii_lowercase();
    #[cfg(target_os = "macos")]
    return processes.contains("/adobe photoshop")
        && processes.contains(".app/contents/macos/adobe photoshop");
    #[cfg(target_os = "windows")]
    return processes
        .lines()
        .any(|line| line.starts_with("photoshop.exe"));
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    false
}

fn xd_is_running() -> bool {
    let processes = process_list().to_ascii_lowercase();
    #[cfg(target_os = "macos")]
    return processes.contains("/adobe xd") && processes.contains(".app/contents/macos/adobe xd");
    #[cfg(target_os = "windows")]
    return processes.lines().any(|line| {
        let line = line.trim_start().to_ascii_lowercase();
        line.starts_with("adobe xd.exe")
    });
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    false
}

fn illustrator_is_running() -> bool {
    let processes = process_list().to_ascii_lowercase();
    #[cfg(target_os = "macos")]
    return processes.contains("/adobe illustrator")
        && processes.contains(".app/contents/macos/adobe illustrator");
    #[cfg(target_os = "windows")]
    return processes
        .lines()
        .any(|line| line.trim_start().to_ascii_lowercase().starts_with("illustrator.exe"));
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    false
}

fn ensure_creative_cloud_running() -> Result<bool, String> {
    if creative_cloud_is_running() {
        return Ok(false);
    }
    let Some(application) = creative_cloud_candidates()
        .into_iter()
        .find(|path| path.exists())
    else {
        // Enterprise installations can provide UPIA without the desktop application.
        return Ok(false);
    };
    #[cfg(target_os = "macos")]
    let launch = Command::new("open").arg(&application).spawn();
    #[cfg(target_os = "windows")]
    let launch = Command::new(&application).spawn();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    return Ok(false);

    launch.map_err(|error| format!("无法启动 Creative Cloud Desktop：{error}"))?;
    let deadline = Instant::now() + CREATIVE_CLOUD_START_TIMEOUT;
    while Instant::now() < deadline {
        if creative_cloud_is_running() {
            return Ok(true);
        }
        thread::sleep(Duration::from_millis(500));
    }
    Err("Creative Cloud Desktop 启动超时。请手动启动并登录 Creative Cloud Desktop 后重试。".into())
}

fn run_upia(command: &mut Command) -> Result<Output, String> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|error| format!("无法启动 Adobe 安装服务：{error}"))?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| "无法读取 Adobe 安装服务标准输出。".to_string())?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| "无法读取 Adobe 安装服务错误输出。".to_string())?;
    let stdout_reader = thread::spawn(move || {
        let mut contents = Vec::new();
        let result = stdout.read_to_end(&mut contents);
        (result.map(|_| contents), "stdout")
    });
    let stderr_reader = thread::spawn(move || {
        let mut contents = Vec::new();
        let result = stderr.read_to_end(&mut contents);
        (result.map(|_| contents), "stderr")
    });
    let deadline = Instant::now() + UPIA_TIMEOUT;

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = stdout_reader
                    .join()
                    .map_err(|_| "读取 Adobe 安装服务标准输出失败。".to_string())?
                    .0
                    .map_err(|error| format!("读取 Adobe 安装服务标准输出失败：{error}"))?;
                let stderr = stderr_reader
                    .join()
                    .map_err(|_| "读取 Adobe 安装服务错误输出失败。".to_string())?
                    .0
                    .map_err(|error| format!("读取 Adobe 安装服务错误输出失败：{error}"))?;
                return Ok(Output {
                    status,
                    stdout,
                    stderr,
                });
            }
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(
                    "Adobe 安装服务执行超时（超过 5 分钟）。请确认 Creative Cloud 已登录后重试。"
                        .into(),
                );
            }
            Ok(None) => thread::sleep(Duration::from_millis(250)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("无法等待 Adobe 安装服务：{error}"));
            }
        }
    }
}

fn value_as_text(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(text) => Some(text.clone()),
        Value::Object(map) => map
            .get("default")
            .or_else(|| map.get("en_US"))
            .or_else(|| map.values().next())
            .and_then(Value::as_str)
            .map(str::to_owned),
        _ => None,
    }
}

fn host_metadata(app: &str) -> (String, String) {
    match app.to_ascii_uppercase().as_str() {
        "PS" | "PHSP" | "PHXS" => ("Photoshop".into(), "/apps/photoshop.svg".into()),
        "ID" | "IDSN" => ("InDesign".into(), "/apps/indesign.svg".into()),
        "PR" | "PPRO" => ("Premiere Pro".into(), "/apps/premiere%20pro.svg".into()),
        "XD" => ("Adobe XD".into(), "/apps/xd.svg".into()),
        "AI" | "ILST" => ("Illustrator".into(), "/apps/illustrator.svg".into()),
        "AE" | "AEFT" => ("After Effects".into(), "/apps/after%20effects.svg".into()),
        "AU" | "AUDT" => ("Audition".into(), "/apps/audition.svg".into()),
        "BR" | "KBRG" => ("Bridge".into(), "/apps/bridge.svg".into()),
        "ACROBAT" | "ACRO" => ("Acrobat Pro".into(), "/apps/Acrobat%20Pro.svg".into()),
        "AICY" => ("InCopy".into(), "/apps/incopy.svg".into()),
        "DRWV" => ("Dreamweaver".into(), "/apps/dreamweaver.svg".into()),
        "FLPR" => ("Animate".into(), "/apps/animate.svg".into()),
        _ => (app.to_owned(), "/apps/others.svg".into()),
    }
}

fn parse_hosts(manifest: &Value) -> Vec<Host> {
    let host_value = manifest.get("host").or_else(|| manifest.get("hosts"));
    let values: Vec<&Value> = match host_value {
        Some(Value::Array(items)) => items.iter().collect(),
        Some(value @ Value::Object(_)) => vec![value],
        _ => Vec::new(),
    };

    values
        .into_iter()
        .filter_map(|item| {
            let app = item
                .get("app")
                .or_else(|| item.get("name"))
                .and_then(Value::as_str)?;
            let min_version = item
                .get("minVersion")
                .or_else(|| item.get("min_version"))
                .and_then(Value::as_str)
                .map(str::to_owned);
            let (label, icon) = host_metadata(app);
            Some(Host {
                app: app.into(),
                label,
                min_version,
                icon,
            })
        })
        .collect()
}

fn package_extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
}

fn zip_entry_name(name: &str) -> String {
    name.replace('\\', "/").to_ascii_lowercase()
}

fn xml_attribute(source: &str, name: &str) -> Option<String> {
    for quote in ['"', '\''] {
        let pattern = format!("{name}={quote}");
        let Some(start) = source.find(&pattern) else {
            continue;
        };
        let rest = &source[start + pattern.len()..];
        let end = rest.find(quote)?;
        return Some(rest[..end].to_string());
    }
    None
}

fn parse_cep_min_version(version: &str) -> Option<String> {
    let trimmed = version
        .trim()
        .trim_start_matches(['[', '('])
        .trim_end_matches([']', ')']);
    let min = trimmed.split(',').next()?.trim();
    if min.is_empty() {
        None
    } else {
        Some(min.to_string())
    }
}

fn parse_cep_hosts(manifest: &str) -> Vec<Host> {
    let mut hosts = Vec::new();
    let lower = manifest.to_ascii_lowercase();
    let mut search_from = 0;
    while let Some(relative) = lower[search_from..].find("<host") {
        let start = search_from + relative;
        let slice = &manifest[start..];
        let end = slice.find('>').unwrap_or(slice.len());
        let tag = &slice[..end];
        if let Some(app) = xml_attribute(tag, "Name").or_else(|| xml_attribute(tag, "name")) {
            let min_version = xml_attribute(tag, "Version")
                .or_else(|| xml_attribute(tag, "version"))
                .as_deref()
                .and_then(parse_cep_min_version);
            if !hosts
                .iter()
                .any(|host: &Host| host.app.eq_ignore_ascii_case(&app))
            {
                let (label, icon) = host_metadata(&app);
                hosts.push(Host {
                    app,
                    label,
                    min_version,
                    icon,
                });
            }
        }
        search_from = start + 1;
    }
    hosts
}

fn read_zip_entry_text(path: &Path, suffix: &str) -> Result<(u64, String), String> {
    let file = File::open(path).map_err(|error| format!("无法打开安装包：{error}"))?;
    let file_size = file.metadata().map(|metadata| metadata.len()).unwrap_or(0);
    let mut archive = ZipArchive::new(file).map_err(|_| "安装包无效或已损坏。".to_string())?;
    let manifest_index = (0..archive.len())
        .find(|index| {
            archive.by_index(*index).ok().is_some_and(|entry| {
                zip_entry_name(entry.name()).ends_with(suffix)
            })
        })
        .ok_or_else(|| format!("安装包中没有找到 {suffix}。"))?;
    let mut manifest_file = archive
        .by_index(manifest_index)
        .map_err(|_| "无法读取插件清单。".to_string())?;
    if manifest_file.size() > 2 * 1024 * 1024 {
        return Err("插件清单异常过大，已停止读取。".into());
    }
    let mut contents = String::new();
    manifest_file
        .read_to_string(&mut contents)
        .map_err(|_| "插件清单不是有效的 UTF-8 文本。".to_string())?;
    Ok((
        file_size,
        contents.trim_start_matches('\u{feff}').to_string(),
    ))
}

fn read_uxp_package(path: &Path) -> Result<PluginPackage, String> {
    let (file_size, contents) = read_zip_entry_text(path, "manifest.json")?;
    let manifest: Value =
        serde_json::from_str(&contents).map_err(|error| format!("插件清单格式无效：{error}"))?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("plugin.ccx")
        .to_owned();
    let fallback_name = path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("UXP 插件")
        .to_owned();

    Ok(PluginPackage {
        path: path.to_string_lossy().into_owned(),
        file_name,
        file_size,
        name: value_as_text(manifest.get("name")).unwrap_or(fallback_name),
        version: value_as_text(manifest.get("version")).unwrap_or_else(|| "未知".into()),
        plugin_id: value_as_text(manifest.get("id")),
        hosts: parse_hosts(&manifest),
        manifest_version: manifest.get("manifestVersion").and_then(Value::as_u64),
        kind: "uxp".into(),
    })
}

fn read_cep_package(path: &Path) -> Result<PluginPackage, String> {
    let (file_size, contents) = read_zip_entry_text(path, "csxs/manifest.xml")?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("plugin.zxp")
        .to_owned();
    let fallback_name = path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("CEP 扩展")
        .to_owned();
    let plugin_id = xml_attribute(&contents, "ExtensionBundleId");
    let name = xml_attribute(&contents, "ExtensionBundleName")
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(fallback_name);
    let version = xml_attribute(&contents, "ExtensionBundleVersion")
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "未知".into());
    let manifest_version = xml_attribute(&contents, "Version")
        .as_deref()
        .and_then(|value| value.split('.').next())
        .and_then(|value| value.parse::<u64>().ok());

    Ok(PluginPackage {
        path: path.to_string_lossy().into_owned(),
        file_name,
        file_size,
        name,
        version,
        plugin_id,
        hosts: parse_cep_hosts(&contents),
        manifest_version,
        kind: "cep".into(),
    })
}

fn read_package(path: &Path) -> Result<PluginPackage, String> {
    if !path.is_file() {
        return Err("找不到所选的安装包。".into());
    }
    match package_extension(path).as_deref() {
        Some("ccx") => read_uxp_package(path),
        Some("xdx") => {
            let mut package = read_uxp_package(path)?;
            package.kind = "xdx".into();
            Ok(package)
        }
        Some("zxp") => read_cep_package(path),
        _ => Err("文件格式不受支持，请选择扩展名为 .ccx、.xdx 或 .zxp 的安装包。".into()),
    }
}

fn is_cep_package(package: &PluginPackage) -> bool {
    package.kind == "cep"
}

const CEP_DEBUG_VERSIONS: [u32; 9] = [6, 7, 8, 9, 10, 11, 12, 13, 14];

fn cep_extensions_dir() -> Result<PathBuf, String> {
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| home.join("Library/Application Support/Adobe/CEP/extensions"))
            .ok_or_else(|| "无法确定当前用户的主目录。".into())
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .map(|app_data| app_data.join("Adobe/CEP/extensions"))
            .ok_or_else(|| "无法确定当前用户的 AppData 目录。".into())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Err("CEP 侧载目前仅支持 macOS 和 Windows。".into())
    }
}

#[cfg(target_os = "windows")]
fn hide_windows_console(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    command.creation_flags(0x08000000);
}

fn cep_debug_key(version: u32) -> String {
    format!("CSXS.{version}")
}

fn cep_debug_enabled(version: u32) -> bool {
    #[cfg(target_os = "macos")]
    {
        Command::new("defaults")
            .args([
                "read",
                &format!("com.adobe.CSXS.{version}"),
                "PlayerDebugMode",
            ])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .is_some_and(|value| value.trim() == "1")
    }
    #[cfg(target_os = "windows")]
    {
        let mut command = Command::new("reg");
        command.args([
            "query",
            &format!(r"HKCU\Software\Adobe\CSXS.{version}"),
            "/v",
            "PlayerDebugMode",
        ]);
        hide_windows_console(&mut command);
        command
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .is_some_and(|value| {
                value.lines().any(|line| {
                    let lower = line.to_ascii_lowercase();
                    lower.contains("playerdebugmode")
                        && lower.split_whitespace().last() == Some("1")
                })
            })
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = version;
        false
    }
}

fn enable_cep_debug_version(version: u32) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let status = Command::new("defaults")
            .args([
                "write",
                &format!("com.adobe.CSXS.{version}"),
                "PlayerDebugMode",
                "-string",
                "1",
            ])
            .status()
            .map_err(|error| format!("无法开启 CSXS.{version} 调试模式：{error}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("无法开启 CSXS.{version} 调试模式。"))
        }
    }
    #[cfg(target_os = "windows")]
    {
        let mut command = Command::new("reg");
        command.args([
            "add",
            &format!(r"HKCU\Software\Adobe\CSXS.{version}"),
            "/v",
            "PlayerDebugMode",
            "/t",
            "REG_SZ",
            "/d",
            "1",
            "/f",
        ]);
        hide_windows_console(&mut command);
        let status = command
            .status()
            .map_err(|error| format!("无法开启 CSXS.{version} 调试模式：{error}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("无法开启 CSXS.{version} 调试模式。"))
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = version;
        Err("当前系统不支持开启 CEP 调试模式。".into())
    }
}

fn cep_debug_status() -> (bool, Vec<String>) {
    let keys = CEP_DEBUG_VERSIONS
        .iter()
        .copied()
        .map(cep_debug_key)
        .collect::<Vec<_>>();
    let enabled = CEP_DEBUG_VERSIONS.iter().copied().all(cep_debug_enabled);
    (enabled, keys)
}

fn enable_cep_debug_mode() -> Result<Vec<String>, String> {
    let mut enabled = Vec::new();
    let mut errors = Vec::new();
    for version in CEP_DEBUG_VERSIONS {
        match enable_cep_debug_version(version) {
            Ok(()) => enabled.push(cep_debug_key(version)),
            Err(error) => errors.push(error),
        }
    }
    if enabled.is_empty() {
        Err(if errors.is_empty() {
            "无法开启 CEP 调试模式。".into()
        } else {
            errors.join("\n")
        })
    } else {
        Ok(enabled)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SideloadApp {
    Photoshop,
    Xd,
    Illustrator,
}

const SIDELOAD_APPS: [SideloadApp; 3] = [
    SideloadApp::Photoshop,
    SideloadApp::Xd,
    SideloadApp::Illustrator,
];

impl SideloadApp {
    fn label(self) -> &'static str {
        match self {
            Self::Photoshop => "Photoshop",
            Self::Xd => "Adobe XD",
            Self::Illustrator => "Illustrator",
        }
    }

    fn manifest_code(self) -> &'static str {
        match self {
            Self::Photoshop => "PS",
            Self::Xd => "XD",
            Self::Illustrator => "AI",
        }
    }

    fn registry_file(self) -> &'static str {
        match self {
            Self::Photoshop => "PS.json",
            Self::Xd => "XD.json",
            Self::Illustrator => "AI.json",
        }
    }

    fn default_min_version(self) -> &'static str {
        match self {
            Self::Photoshop => "22.0.0",
            Self::Xd => "36.0.0",
            Self::Illustrator => "26.0.0",
        }
    }

    fn matches(self, app: &str) -> bool {
        match self {
            Self::Photoshop => matches!(app.to_ascii_uppercase().as_str(), "PS" | "PHSP"),
            Self::Xd => app.eq_ignore_ascii_case("XD"),
            Self::Illustrator => matches!(app.to_ascii_uppercase().as_str(), "AI" | "ILST"),
        }
    }

    fn is_running(self) -> bool {
        match self {
            Self::Photoshop => photoshop_is_running(),
            Self::Xd => xd_is_running(),
            Self::Illustrator => illustrator_is_running(),
        }
    }
}

fn sideload_apps(package: &PluginPackage) -> Vec<SideloadApp> {
    SIDELOAD_APPS
        .into_iter()
        .filter(|app| package.hosts.iter().any(|host| app.matches(&host.app)))
        .collect()
}

fn sideload_app_labels() -> Vec<String> {
    SIDELOAD_APPS
        .iter()
        .map(|app| app.label().to_string())
        .collect()
}

fn join_labels_or(labels: &[String]) -> String {
    match labels {
        [] => "兼容的 Adobe 应用".into(),
        [one] => one.clone(),
        [first, second] => format!("{first} 或 {second}"),
        _ => {
            let (last, rest) = labels.split_last().unwrap();
            format!("{} 或 {last}", rest.join("、"))
        }
    }
}

fn sideload_unsupported_message() -> String {
    format!(
        "该安装包未声明支持 {}。",
        join_labels_or(&sideload_app_labels())
    )
}

fn sideload_unsupported_hint() -> String {
    let details = SIDELOAD_APPS
        .iter()
        .map(|app| format!("{}（{}）", app.label(), app.manifest_code()))
        .collect::<Vec<_>>();
    format!(
        "侧载目前仅支持 manifest 中宿主为 {} 的插件。",
        join_labels_or(&details)
    )
}

fn sideload_platform_hint() -> String {
    format!(
        "侧载目前仅支持 macOS 和 Windows 上的 {}。",
        join_labels(&sideload_app_labels())
    )
}

fn supports_sideload(package: &PluginPackage) -> bool {
    !sideload_apps(package).is_empty()
}

fn join_labels(labels: &[String]) -> String {
    match labels {
        [] => "兼容的 Adobe 应用".into(),
        [one] => one.clone(),
        [first, second] => format!("{first} 和 {second}"),
        _ => {
            let (last, rest) = labels.split_last().unwrap();
            format!("{} 和 {last}", rest.join("、"))
        }
    }
}

fn package_host_labels(package: &PluginPackage) -> Vec<String> {
    let mut labels = Vec::new();
    for host in &package.hosts {
        if !labels.contains(&host.label) {
            labels.push(host.label.clone());
        }
    }
    labels
}

fn declared_host_is_running(package: &PluginPackage) -> bool {
    package.hosts.iter().any(|host| {
        SIDELOAD_APPS
            .into_iter()
            .any(|app| app.matches(&host.app) && app.is_running())
    })
}

fn host_min_version(package: &PluginPackage, app: SideloadApp) -> String {
    package
        .hosts
        .iter()
        .find(|host| app.matches(&host.app))
        .and_then(|host| host.min_version.clone())
        .unwrap_or_else(|| app.default_min_version().into())
}

fn sideload_base() -> Result<PathBuf, String> {
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| home.join("Library/Application Support/Adobe/UXP"))
            .ok_or_else(|| "无法确定当前用户的主目录。".into())
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .map(|app_data| app_data.join("Adobe/UXP"))
            .ok_or_else(|| "无法确定当前用户的 AppData 目录。".into())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Err("侧载目前仅支持 macOS 和 Windows。".into())
    }
}

fn permission_hint() -> String {
    #[cfg(target_os = "macos")]
    return "请在访达中打开上级目录，选择“显示简介”→“共享与权限”，为当前用户启用“读与写”，然后重试。".into();
    #[cfg(target_os = "windows")]
    return "请打开文件或文件夹的“属性”→“安全”→“编辑”，为当前用户授予“修改”和“写入”权限，然后重试。".into();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    return "请为当前用户授予该位置的读写权限。".into();
}

fn probe_directory(path: &Path) -> Result<(), String> {
    let mut probe_parent = path;
    while !probe_parent.exists() {
        probe_parent = probe_parent
            .parent()
            .ok_or_else(|| "找不到可用于权限检测的上级目录。".to_string())?;
    }
    if !probe_parent.is_dir() {
        return Err("目标路径的上级位置不是文件夹。".into());
    }
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let probe = probe_parent.join(format!(
        ".openuxp-write-test-{}-{nonce}",
        std::process::id()
    ));
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .map_err(|error| format!("无法写入：{error}"))?;
    fs::remove_file(&probe).map_err(|error| format!("无法清理权限检测文件：{error}"))
}

fn build_cep_preflight(package: &PluginPackage) -> SideloadPreflight {
    let mut issues = Vec::new();
    let host_labels = package_host_labels(package);
    let (debug_mode_enabled, debug_mode_keys) = cep_debug_status();
    let supported = cfg!(any(target_os = "macos", target_os = "windows"));
    let plugin_directory = match cep_extensions_dir() {
        Ok(directory) => directory,
        Err(message) => {
            issues.push(PermissionIssue {
                path: String::new(),
                message,
                hint: "CEP 侧载目前仅支持 macOS 和 Windows。".into(),
            });
            return SideloadPreflight {
                supported: false,
                ready: false,
                plugin_directory: String::new(),
                registry_path: String::new(),
                registry_paths: Vec::new(),
                host_labels,
                issues,
                kind: "cep".into(),
                debug_mode_enabled,
                debug_mode_keys,
            };
        }
    };
    if package.plugin_id.as_deref().is_none_or(str::is_empty) {
        issues.push(PermissionIssue {
            path: String::new(),
            message: "CEP 清单缺少有效的 ExtensionBundleId。".into(),
            hint: "侧载需要 CSXS/manifest.xml 中存在非空 ExtensionBundleId。".into(),
        });
    } else if package
        .plugin_id
        .as_deref()
        .is_some_and(|id| safe_component(id) != id)
    {
        issues.push(PermissionIssue {
            path: String::new(),
            message: "扩展 ID 包含不安全的路径字符。".into(),
            hint: "为避免目录冲突，侧载仅接受字母、数字、点、连字符和下划线组成的扩展 ID。".into(),
        });
    }
    if let Err(error) = probe_directory(&plugin_directory) {
        issues.push(PermissionIssue {
            path: plugin_directory.to_string_lossy().into_owned(),
            message: format!("CEP 扩展目录权限不足：{error}"),
            hint: permission_hint(),
        });
    }
    SideloadPreflight {
        supported,
        ready: supported && issues.is_empty(),
        plugin_directory: plugin_directory.to_string_lossy().into_owned(),
        registry_path: String::new(),
        registry_paths: Vec::new(),
        host_labels,
        issues,
        kind: "cep".into(),
        debug_mode_enabled,
        debug_mode_keys,
    }
}

fn build_sideload_preflight(package: &PluginPackage) -> SideloadPreflight {
    if is_cep_package(package) {
        return build_cep_preflight(package);
    }
    let mut issues = Vec::new();
    let apps = sideload_apps(package);
    let host_labels = apps
        .iter()
        .map(|app| app.label().to_string())
        .collect::<Vec<_>>();
    let supported = cfg!(any(target_os = "macos", target_os = "windows")) && !apps.is_empty();
    let base = match sideload_base() {
        Ok(base) => base,
        Err(message) => {
            issues.push(PermissionIssue {
                path: String::new(),
                message,
                hint: sideload_platform_hint(),
            });
            return SideloadPreflight {
                supported: false,
                ready: false,
                plugin_directory: String::new(),
                registry_path: String::new(),
                registry_paths: Vec::new(),
                host_labels,
                issues,
                kind: "uxp".into(),
                debug_mode_enabled: false,
                debug_mode_keys: Vec::new(),
            };
        }
    };
    let plugin_directory = base.join("Plugins/External");
    let registry_directory = base.join("PluginsInfo/v1");
    let registry_paths = apps
        .iter()
        .map(|app| registry_directory.join(app.registry_file()))
        .collect::<Vec<_>>();

    if apps.is_empty() {
        issues.push(PermissionIssue {
            path: String::new(),
            message: sideload_unsupported_message(),
            hint: sideload_unsupported_hint(),
        });
    }
    if package.plugin_id.as_deref().is_none_or(str::is_empty) {
        issues.push(PermissionIssue {
            path: String::new(),
            message: "插件清单缺少有效的插件 ID。".into(),
            hint: "侧载需要 manifest.json 中存在非空 id。".into(),
        });
    } else if package
        .plugin_id
        .as_deref()
        .is_some_and(|id| safe_component(id) != id)
    {
        issues.push(PermissionIssue {
            path: String::new(),
            message: "插件 ID 包含不安全的路径字符。".into(),
            hint: "为避免目录冲突，侧载仅接受字母、数字、点、连字符和下划线组成的插件 ID。".into(),
        });
    }
    if package.version.is_empty() || package.version == "未知" {
        issues.push(PermissionIssue {
            path: String::new(),
            message: "插件清单缺少有效版本号。".into(),
            hint: "侧载需要 manifest.json 中存在非空 version。".into(),
        });
    }
    if let Err(error) = probe_directory(&plugin_directory) {
        issues.push(PermissionIssue {
            path: plugin_directory.to_string_lossy().into_owned(),
            message: format!("插件目录权限不足：{error}"),
            hint: permission_hint(),
        });
    }
    if let Err(error) = probe_directory(&registry_directory) {
        issues.push(PermissionIssue {
            path: registry_directory.to_string_lossy().into_owned(),
            message: format!("注册目录权限不足：{error}"),
            hint: permission_hint(),
        });
    }
    for registry_path in &registry_paths {
        if registry_path.exists() {
            if let Err(error) = OpenOptions::new()
                .read(true)
                .write(true)
                .open(registry_path)
            {
                let file_name = registry_path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("注册文件");
                issues.push(PermissionIssue {
                    path: registry_path.to_string_lossy().into_owned(),
                    message: format!("{file_name} 权限不足：{error}"),
                    hint: permission_hint(),
                });
            }
        }
    }

    let registry_path_strings = registry_paths
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    SideloadPreflight {
        supported,
        ready: supported && issues.is_empty(),
        plugin_directory: plugin_directory.to_string_lossy().into_owned(),
        registry_path: registry_path_strings.first().cloned().unwrap_or_default(),
        registry_paths: registry_path_strings,
        host_labels,
        issues,
        kind: "uxp".into(),
        debug_mode_enabled: false,
        debug_mode_keys: Vec::new(),
    }
}

fn safe_component(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

fn unique_temp_path(path: &Path, nonce: u128) -> Result<PathBuf, String> {
    let parent = path
        .parent()
        .ok_or_else(|| "目标文件路径无效。".to_string())?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "目标文件名无效。".to_string())?;
    Ok(parent.join(format!(".{file_name}.openuxp-temp-{nonce}")))
}

fn replace_file(temp: &Path, destination: &Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    if destination.exists() {
        fs::remove_file(destination).map_err(|error| format!("无法替换旧文件：{error}"))?;
    }

    fs::rename(temp, destination).map_err(|error| format!("无法启用新文件：{error}"))
}

fn write_file_atomically(path: &Path, contents: &[u8], nonce: u128) -> Result<(), String> {
    let temp = unique_temp_path(path, nonce)?;
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|error| format!("无法创建临时注册文件：{error}"))?;
        file.write_all(contents)
            .map_err(|error| format!("无法写入临时注册文件：{error}"))?;
        file.sync_all()
            .map_err(|error| format!("无法保存临时注册文件：{error}"))?;
        replace_file(&temp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn extract_ccx(path: &Path, destination: &Path) -> Result<(), String> {
    extract_archive(path, destination, "manifest.json", "manifest.json")
}

fn extract_cep(path: &Path, destination: &Path) -> Result<(), String> {
    extract_archive(path, destination, "csxs/manifest.xml", "CSXS/manifest.xml")
}

fn extract_archive(
    path: &Path,
    destination: &Path,
    marker_suffix: &str,
    required_file: &str,
) -> Result<(), String> {
    let file = File::open(path).map_err(|error| format!("无法打开安装包：{error}"))?;
    let mut archive = ZipArchive::new(file).map_err(|_| "安装包无效或已损坏。".to_string())?;
    if archive.len() > 10_000 {
        return Err("安装包文件数量异常，已停止侧载。".into());
    }

    let mut manifest_path = None;
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|error| format!("无法检查安装包内容：{error}"))?;
        if zip_entry_name(entry.name()).ends_with(marker_suffix) {
            manifest_path = entry.enclosed_name();
            break;
        }
    }
    let manifest_path =
        manifest_path.ok_or_else(|| format!("安装包中没有安全的 {marker_suffix} 路径。"))?;
    let mut manifest_root = manifest_path.clone();
    for _ in 0..Path::new(marker_suffix).components().count() {
        manifest_root = manifest_root
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
    }
    let total_size: u64 = (0..archive.len())
        .map(|index| {
            archive
                .by_index(index)
                .map(|entry| entry.size())
                .unwrap_or(u64::MAX)
        })
        .try_fold(0_u64, |total, size| total.checked_add(size))
        .ok_or_else(|| "安装包展开大小异常。".to_string())?;
    if total_size > 1024 * 1024 * 1024 {
        return Err("安装包展开后超过 1 GB，已停止侧载。".into());
    }

    fs::create_dir_all(destination).map_err(|error| format!("无法创建临时插件目录：{error}"))?;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| format!("无法读取安装包条目：{error}"))?;
        let enclosed = entry
            .enclosed_name()
            .ok_or_else(|| format!("安装包包含不安全路径：{}", entry.name()))?
            .to_path_buf();
        let relative = if manifest_root.as_os_str().is_empty() {
            enclosed
        } else if let Ok(relative) = enclosed.strip_prefix(&manifest_root) {
            relative.to_path_buf()
        } else {
            continue;
        };
        if relative.as_os_str().is_empty() {
            continue;
        }
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err("安装包包含符号链接，已停止侧载。".into());
        }
        if entry.size() > 256 * 1024 * 1024 {
            return Err(format!("安装包中的文件过大：{}", entry.name()));
        }
        let output_path = destination.join(relative);
        if entry.is_dir() {
            fs::create_dir_all(&output_path)
                .map_err(|error| format!("无法创建插件文件夹：{error}"))?;
        } else {
            if let Some(parent) = output_path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|error| format!("无法创建插件文件夹：{error}"))?;
            }
            let mut output =
                File::create(&output_path).map_err(|error| format!("无法写入插件文件：{error}"))?;
            std::io::copy(&mut entry, &mut output)
                .map_err(|error| format!("无法解压插件文件：{error}"))?;
        }
    }
    if !destination.join(required_file).is_file() {
        return Err(format!("侧载目录中没有找到 {required_file}。"));
    }
    Ok(())
}

#[tauri::command]
fn check_environment() -> Environment {
    let installer = find_upia();
    let creative_cloud_found = creative_cloud_candidates().iter().any(|path| path.exists());

    Environment {
        platform: std::env::consts::OS.into(),
        installer_found: installer.is_some(),
        installer_path: installer.map(|path| path.to_string_lossy().into_owned()),
        creative_cloud_found,
    }
}

#[tauri::command]
fn pick_ccx() -> Result<Option<PluginPackage>, String> {
    rfd::FileDialog::new()
        .add_filter("Adobe 插件安装包", &["ccx", "xdx", "zxp"])
        .add_filter("Adobe UXP 安装包", &["ccx"])
        .add_filter("Adobe XD 安装包", &["xdx"])
        .add_filter("Adobe CEP 安装包", &["zxp"])
        .pick_file()
        .map(|path| read_package(&path))
        .transpose()
}

#[tauri::command]
fn inspect_ccx(path: String) -> Result<PluginPackage, String> {
    read_package(Path::new(&path))
}

#[tauri::command]
fn check_sideload(path: String) -> Result<SideloadPreflight, String> {
    let package = read_package(Path::new(&path))?;
    Ok(build_sideload_preflight(&package))
}

#[tauri::command]
fn install_ccx(path: String) -> Result<InstallResult, String> {
    let package_path = PathBuf::from(&path);
    let package = read_package(&package_path)?;
    if is_cep_package(&package) {
        return Ok(InstallResult {
            success: false,
            message: "CEP 扩展请使用侧载安装。官方安装服务不用于 .zxp 包。".into(),
            details: Some("侧载会将扩展复制到用户级 CEP 目录，并自动开启调试模式。".into()),
            can_side_load: true,
            activation_pending: false,
        });
    }
    let can_side_load = supports_sideload(&package);
    let Some(installer) = find_upia() else {
        return Ok(InstallResult {
            success: false,
            message:
                "未找到 Adobe Unified Plugin Installer Agent。请先安装或更新 Creative Cloud Desktop。"
                    .into(),
            details: None,
            can_side_load,
            activation_pending: false,
        });
    };
    let creative_cloud_started = match ensure_creative_cloud_running() {
        Ok(started) => started,
        Err(error) => {
            return Ok(InstallResult {
                success: false,
                message: error,
                details: Some("请手动启动并登录 Creative Cloud Desktop 后重试。".into()),
                can_side_load,
                activation_pending: false,
            });
        }
    };
    let host_was_running = declared_host_is_running(&package);
    let host_summary = join_labels(&package_host_labels(&package));

    let mut command = Command::new(installer);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
        command.arg("/install");
    }
    #[cfg(not(target_os = "windows"))]
    command.arg("--install");
    let output = match run_upia(command.arg(&package_path)) {
        Ok(output) => output,
        Err(error) => {
            return Ok(InstallResult {
                success: false,
                message: error,
                details: Some("请确认 Creative Cloud Desktop 已安装、已登录，并保持运行。".into()),
                can_side_load,
                activation_pending: false,
            });
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    let mut detail_parts = [stdout, stderr]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if creative_cloud_started {
        detail_parts.push("已自动启动 Creative Cloud Desktop。".into());
    }
    let details = detail_parts.join("\n");

    if output.status.success() {
        Ok(InstallResult {
            success: true,
            message: if host_was_running {
                format!("插件已安装；请完全退出并重新打开 {host_summary} 以启用插件。")
            } else {
                format!("插件已成功安装，打开 {host_summary} 即可载入。")
            },
            details: (!details.is_empty()).then_some(details),
            can_side_load: false,
            activation_pending: host_was_running,
        })
    } else {
        Ok(InstallResult {
            success: false,
            message: output
                .status
                .code()
                .map(|code| format!("Adobe 安装服务返回错误代码 {code}。"))
                .unwrap_or_else(|| "Adobe 安装服务意外终止。".into()),
            details: (!details.is_empty()).then_some(details),
            can_side_load,
            activation_pending: false,
        })
    }
}

struct RegistryUpdate {
    path: PathBuf,
    file_name: String,
    old_contents: Option<Vec<u8>>,
    new_contents: Vec<u8>,
    backup_path: Option<PathBuf>,
}

fn prepare_registry_update(
    registry_path: &Path,
    plugin_id: &str,
    entry: Value,
) -> Result<RegistryUpdate, String> {
    let file_name = registry_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("registry.json")
        .to_owned();
    let old_contents = if registry_path.exists() {
        Some(
            fs::read(registry_path)
                .map_err(|error| format!("无法读取 {file_name}：{error}"))?,
        )
    } else {
        None
    };
    let mut registry: Value = match old_contents.as_deref() {
        Some(contents) if !contents.is_empty() => serde_json::from_slice(contents)
            .map_err(|error| format!("{file_name} 格式无效，未执行侧载：{error}"))?,
        _ => json!({ "plugins": [] }),
    };
    let Some(plugins) = registry.get_mut("plugins").and_then(Value::as_array_mut) else {
        return Err(format!("{file_name} 缺少 plugins 数组，未执行侧载。"));
    };
    plugins.retain(|item| item.get("pluginId").and_then(Value::as_str) != Some(plugin_id));
    plugins.push(entry);
    let new_contents = serde_json::to_vec_pretty(&registry)
        .map_err(|error| format!("无法生成 {file_name} 注册信息：{error}"))?;
    Ok(RegistryUpdate {
        path: registry_path.to_path_buf(),
        file_name,
        old_contents,
        new_contents,
        backup_path: None,
    })
}

fn restore_registry(update: &RegistryUpdate, nonce: u128) {
    if let Some(contents) = &update.old_contents {
        let _ = write_file_atomically(&update.path, contents, nonce);
    } else {
        let _ = fs::remove_file(&update.path);
    }
}

fn sideload_cep(package_path: &Path, package: &PluginPackage) -> Result<InstallResult, String> {
    let host_summary = join_labels(&package_host_labels(package));
    let preflight = build_cep_preflight(package);
    if !preflight.ready {
        let details = preflight
            .issues
            .iter()
            .map(|issue| {
                if issue.path.is_empty() {
                    issue.message.clone()
                } else {
                    format!("{}\n{}", issue.message, issue.path)
                }
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        return Ok(InstallResult {
            success: false,
            message: "侧载前权限检查未通过，请修改权限后重试。".into(),
            details: Some(details),
            can_side_load: true,
            activation_pending: false,
        });
    }
    let plugin_id = package
        .plugin_id
        .as_deref()
        .filter(|id| !id.is_empty())
        .ok_or_else(|| "CEP 清单缺少有效的 ExtensionBundleId。".to_string())?;
    let folder_name = safe_component(plugin_id);
    let plugin_directory = PathBuf::from(&preflight.plugin_directory);
    fs::create_dir_all(&plugin_directory)
        .map_err(|error| format!("无法创建 CEP 扩展目录：{error}"))?;

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let staging =
        plugin_directory.join(format!(".openuxp-staging-{}-{}", std::process::id(), nonce));
    if let Err(error) = extract_cep(package_path, &staging) {
        let _ = fs::remove_dir_all(&staging);
        return Ok(InstallResult {
            success: false,
            message: "无法安全解压 CEP 安装包。".into(),
            details: Some(error),
            can_side_load: true,
            activation_pending: false,
        });
    }
    let extracted_manifest = match fs::read_to_string(staging.join("CSXS/manifest.xml")) {
        Ok(contents) => contents,
        Err(error) => {
            let _ = fs::remove_dir_all(&staging);
            return Err(format!("无法复核解压后的 CEP 清单：{error}"));
        }
    };
    if xml_attribute(&extracted_manifest, "ExtensionBundleId").as_deref() != Some(plugin_id) {
        let _ = fs::remove_dir_all(&staging);
        return Err("安装包在读取过程中发生变化，已停止侧载。".into());
    }

    let target = plugin_directory.join(&folder_name);
    let old_target = plugin_directory.join(format!(".openuxp-previous-{folder_name}-{nonce}"));
    let had_target = target.exists();
    if had_target {
        if let Err(error) = fs::rename(&target, &old_target) {
            let _ = fs::remove_dir_all(&staging);
            return Ok(InstallResult {
                success: false,
                message: format!("无法替换现有扩展，请关闭 {host_summary} 后重试。"),
                details: Some(error.to_string()),
                can_side_load: true,
                activation_pending: false,
            });
        }
    }
    if let Err(error) = fs::rename(&staging, &target) {
        if had_target {
            let _ = fs::rename(&old_target, &target);
        }
        let _ = fs::remove_dir_all(&staging);
        return Err(format!("无法启用已解压的 CEP 扩展：{error}"));
    }
    if had_target {
        let _ = fs::remove_dir_all(&old_target);
    }

    let mut detail_lines = vec![format!("安装目录：{}", target.to_string_lossy())];
    let debug_result = enable_cep_debug_mode();
    match &debug_result {
        Ok(keys) => {
            detail_lines.push(format!("已开启 CEP 调试模式：{}", keys.join("、")));
        }
        Err(error) => {
            detail_lines.push(format!("扩展文件已安装，但未能完全开启 CEP 调试模式：{error}"));
        }
    }

    Ok(InstallResult {
        success: true,
        message: if debug_result.is_ok() {
            format!("CEP 扩展已安装到用户目录，并已开启调试模式。请重启 {host_summary}。")
        } else {
            format!("CEP 扩展已安装，但调试模式未能完全开启。未签名扩展可能无法加载，请重启 {host_summary} 后再试。")
        },
        details: Some(detail_lines.join("\n")),
        can_side_load: false,
        activation_pending: true,
    })
}

#[tauri::command]
fn sideload_ccx(path: String) -> Result<InstallResult, String> {
    let package_path = PathBuf::from(&path);
    let package = read_package(&package_path)?;
    if is_cep_package(&package) {
        return sideload_cep(&package_path, &package);
    }
    let apps = sideload_apps(&package);
    let host_summary = join_labels(
        &apps
            .iter()
            .map(|app| app.label().to_string())
            .collect::<Vec<_>>(),
    );
    let preflight = build_sideload_preflight(&package);
    if !preflight.ready {
        let details = preflight
            .issues
            .iter()
            .map(|issue| {
                if issue.path.is_empty() {
                    issue.message.clone()
                } else {
                    format!("{}\n{}", issue.message, issue.path)
                }
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        return Ok(InstallResult {
            success: false,
            message: "侧载前权限检查未通过，请修改权限后重试。".into(),
            details: Some(details),
            can_side_load: true,
            activation_pending: false,
        });
    }

    let plugin_id = package
        .plugin_id
        .as_deref()
        .filter(|id| !id.is_empty())
        .ok_or_else(|| "插件清单缺少有效的插件 ID。".to_string())?;
    let folder_name = format!(
        "{}_{}",
        safe_component(plugin_id),
        safe_component(&package.version)
    );
    let plugin_directory = PathBuf::from(&preflight.plugin_directory);
    if preflight.registry_paths.is_empty() {
        return Err("没有可用于侧载的注册文件。".into());
    }
    let registry_directory = PathBuf::from(&preflight.registry_paths[0])
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "插件注册路径无效。".to_string())?;
    fs::create_dir_all(&plugin_directory)
        .map_err(|error| format!("无法创建插件目录：{error}"))?;
    fs::create_dir_all(&registry_directory)
        .map_err(|error| format!("无法创建注册目录：{error}"))?;

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let staging =
        plugin_directory.join(format!(".openuxp-staging-{}-{}", std::process::id(), nonce));
    if let Err(error) = extract_ccx(&package_path, &staging) {
        let _ = fs::remove_dir_all(&staging);
        return Ok(InstallResult {
            success: false,
            message: "无法安全解压 CCX 安装包。".into(),
            details: Some(error),
            can_side_load: true,
            activation_pending: false,
        });
    }
    let extracted_manifest = match fs::read_to_string(staging.join("manifest.json")) {
        Ok(contents) => contents,
        Err(error) => {
            let _ = fs::remove_dir_all(&staging);
            return Err(format!("无法复核解压后的插件清单：{error}"));
        }
    };
    let extracted_manifest: Value =
        match serde_json::from_str(extracted_manifest.trim_start_matches('\u{feff}')) {
            Ok(manifest) => manifest,
            Err(error) => {
                let _ = fs::remove_dir_all(&staging);
                return Err(format!("解压后的插件清单无效：{error}"));
            }
        };
    if value_as_text(extracted_manifest.get("id")).as_deref() != Some(plugin_id)
        || value_as_text(extracted_manifest.get("version")).as_deref()
            != Some(package.version.as_str())
    {
        let _ = fs::remove_dir_all(&staging);
        return Err("安装包在读取过程中发生变化，已停止侧载。".into());
    }

    let registry_relative_path = if cfg!(target_os = "windows") {
        format!("$localPlugins\\External\\{folder_name}")
    } else {
        format!("$localPlugins/External/{folder_name}")
    };
    let mut updates = Vec::new();
    for app in &apps {
        let registry_path = registry_directory.join(app.registry_file());
        let entry = json!({
            "hostMinVersion": host_min_version(&package, *app),
            "name": package.name,
            "path": registry_relative_path,
            "pluginId": plugin_id,
            "status": "enabled",
            "type": "uxp",
            "versionString": package.version,
        });
        match prepare_registry_update(&registry_path, plugin_id, entry) {
            Ok(mut update) => {
                if update.old_contents.is_some() {
                    update.backup_path = Some(registry_directory.join(format!(
                        "{}.openuxp-backup-{nonce}",
                        update.file_name
                    )));
                }
                updates.push(update);
            }
            Err(error) => {
                let _ = fs::remove_dir_all(&staging);
                return Err(error);
            }
        }
    }

    let target = plugin_directory.join(&folder_name);
    let old_target = plugin_directory.join(format!(".openuxp-previous-{folder_name}-{nonce}"));
    let had_target = target.exists();
    let restore_plugin = || {
        let _ = fs::remove_dir_all(&target);
        if had_target {
            let _ = fs::rename(&old_target, &target);
        }
    };
    if had_target {
        if let Err(error) = fs::rename(&target, &old_target) {
            let _ = fs::remove_dir_all(&staging);
            return Ok(InstallResult {
                success: false,
                message: format!("无法替换现有插件，请关闭 {host_summary} 后重试。"),
                details: Some(error.to_string()),
                can_side_load: true,
                activation_pending: false,
            });
        }
    }
    if let Err(error) = fs::rename(&staging, &target) {
        if had_target {
            let _ = fs::rename(&old_target, &target);
        }
        let _ = fs::remove_dir_all(&staging);
        return Err(format!("无法启用已解压的插件：{error}"));
    }

    for (index, update) in updates.iter().enumerate() {
        if let (Some(contents), Some(backup)) = (&update.old_contents, &update.backup_path) {
            if let Err(error) =
                write_file_atomically(backup, contents, nonce.wrapping_add((index as u128) * 4))
            {
                restore_plugin();
                return Err(format!(
                    "无法备份 {}，已取消侧载：{error}",
                    update.file_name
                ));
            }
        }
    }
    for (index, update) in updates.iter().enumerate() {
        if let Err(error) = write_file_atomically(
            &update.path,
            &update.new_contents,
            nonce.wrapping_add((index as u128) * 4 + 1),
        ) {
            for (previous_index, previous) in updates.iter().take(index + 1).enumerate() {
                restore_registry(previous, nonce.wrapping_add((previous_index as u128) * 4 + 2));
            }
            restore_plugin();
            return Ok(InstallResult {
                success: false,
                message: format!("写入 {} 失败，已回滚插件文件。", update.file_name),
                details: Some(error.to_string()),
                can_side_load: true,
                activation_pending: false,
            });
        }
    }
    if had_target {
        let _ = fs::remove_dir_all(&old_target);
    }

    let mut detail_lines = vec![format!("安装目录：{}", target.to_string_lossy())];
    for update in &updates {
        detail_lines.push(format!("注册文件：{}", update.path.to_string_lossy()));
        if let Some(backup) = &update.backup_path {
            detail_lines.push(format!("注册文件备份：{}", backup.to_string_lossy()));
        }
    }
    Ok(InstallResult {
        success: true,
        message: format!("插件已通过侧载安装到 {host_summary}，请重启 {host_summary}。"),
        details: Some(detail_lines.join("\n")),
        can_side_load: false,
        activation_pending: true,
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            #[cfg(desktop)]
            app.handle()
                .plugin(tauri_plugin_updater::Builder::new().build())?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            check_environment,
            pick_ccx,
            inspect_ccx,
            install_ccx,
            check_sideload,
            sideload_ccx
        ])
        .run(tauri::generate_context!())
        .expect("error while running OpenUXP Installer");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::{write::SimpleFileOptions, ZipWriter};

    fn test_directory(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "openuxp-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ))
    }

    #[test]
    fn extracts_only_the_plugin_root() {
        let root = test_directory("extract");
        fs::create_dir_all(&root).unwrap();
        let archive_path = root.join("sample.ccx");
        let file = File::create(&archive_path).unwrap();
        let mut archive = ZipWriter::new(file);
        archive
            .start_file("wrapper/manifest.json", SimpleFileOptions::default())
            .unwrap();
        archive
            .write_all(br#"{"id":"com.example.test","version":"1.0.0"}"#)
            .unwrap();
        archive
            .start_file("wrapper/index.js", SimpleFileOptions::default())
            .unwrap();
        archive.write_all(b"console.log('ok')").unwrap();
        archive
            .start_file("package-metadata.txt", SimpleFileOptions::default())
            .unwrap();
        archive.write_all(b"ignored").unwrap();
        archive.finish().unwrap();

        let output = root.join("output");
        extract_ccx(&archive_path, &output).unwrap();
        assert!(output.join("manifest.json").is_file());
        assert!(output.join("index.js").is_file());
        assert!(!output.join("package-metadata.txt").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn sanitizes_directory_components() {
        assert_eq!(
            safe_component("com.example_plugin-1"),
            "com.example_plugin-1"
        );
        assert_eq!(safe_component("../../escape"), ".._.._escape");
    }

    #[test]
    fn writes_files_atomically() {
        let root = test_directory("atomic-write");
        fs::create_dir_all(&root).unwrap();
        let target = root.join("PS.json");
        fs::write(&target, br#"{"plugins":[]}"#).unwrap();

        write_file_atomically(&target, br#"{"plugins":[{"pluginId":"com.example"}]}"#, 1).unwrap();

        assert_eq!(
            fs::read_to_string(&target).unwrap(),
            r#"{"plugins":[{"pluginId":"com.example"}]}"#
        );
        assert!(!root.join(".PS.json.openuxp-temp-1").exists());
        fs::remove_dir_all(root).unwrap();
    }

    fn sample_package(apps: &[&str]) -> PluginPackage {
        PluginPackage {
            path: "test.ccx".into(),
            file_name: "test.ccx".into(),
            file_size: 1,
            name: "Test".into(),
            version: "1.0.0".into(),
            plugin_id: Some("com.example.test".into()),
            hosts: apps
                .iter()
                .map(|app| {
                    let (label, icon) = host_metadata(app);
                    Host {
                        app: (*app).into(),
                        label,
                        min_version: None,
                        icon,
                    }
                })
                .collect(),
            manifest_version: Some(4),
            kind: "uxp".into(),
        }
    }

    #[test]
    fn sideload_detects_photoshop_xd_and_illustrator_hosts() {
        assert_eq!(
            sideload_apps(&sample_package(&["PS"])),
            vec![SideloadApp::Photoshop]
        );
        assert_eq!(
            sideload_apps(&sample_package(&["XD"])),
            vec![SideloadApp::Xd]
        );
        assert_eq!(
            sideload_apps(&sample_package(&["AI"])),
            vec![SideloadApp::Illustrator]
        );
        assert_eq!(
            sideload_apps(&sample_package(&["ILST"])),
            vec![SideloadApp::Illustrator]
        );
        assert_eq!(
            sideload_apps(&sample_package(&["XD", "PHSP", "AI"])),
            vec![
                SideloadApp::Photoshop,
                SideloadApp::Xd,
                SideloadApp::Illustrator
            ]
        );
        assert!(sideload_apps(&sample_package(&["PR", "ID"])).is_empty());
        assert!(supports_sideload(&sample_package(&["XD"])));
        assert!(supports_sideload(&sample_package(&["ILST"])));
        assert!(!supports_sideload(&sample_package(&["PPRO"])));
        assert_eq!(SideloadApp::Xd.registry_file(), "XD.json");
        assert_eq!(SideloadApp::Photoshop.registry_file(), "PS.json");
        assert_eq!(SideloadApp::Illustrator.registry_file(), "AI.json");
    }

    #[test]
    fn joins_host_labels() {
        assert_eq!(join_labels(&[]), "兼容的 Adobe 应用");
        assert_eq!(join_labels(&["Adobe XD".into()]), "Adobe XD");
        assert_eq!(
            join_labels(&["Photoshop".into(), "Adobe XD".into()]),
            "Photoshop 和 Adobe XD"
        );
        assert_eq!(
            join_labels_or(&["Photoshop".into(), "Adobe XD".into(), "Illustrator".into()]),
            "Photoshop、Adobe XD 或 Illustrator"
        );
    }

    #[test]
    fn upserts_xd_registry_entries() {
        let root = test_directory("xd-registry");
        fs::create_dir_all(&root).unwrap();
        let registry = root.join("XD.json");
        fs::write(&registry, br#"{"plugins":[{"pluginId":"com.old","name":"Old"}]}"#).unwrap();

        let update = prepare_registry_update(
            &registry,
            "com.example.test",
            json!({
                "pluginId": "com.example.test",
                "name": "Test",
                "status": "enabled"
            }),
        )
        .unwrap();
        write_file_atomically(&update.path, &update.new_contents, 7).unwrap();

        let parsed: Value = serde_json::from_slice(&fs::read(&registry).unwrap()).unwrap();
        let plugins = parsed.get("plugins").and_then(Value::as_array).unwrap();
        assert_eq!(plugins.len(), 2);
        assert_eq!(
            plugins[1].get("pluginId").and_then(Value::as_str),
            Some("com.example.test")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn parses_cep_manifest_hosts_and_bundle_id() {
        let manifest = r#"<?xml version="1.0" encoding="UTF-8"?>
<ExtensionManifest Version="11.0" ExtensionBundleId="com.example.cep"
                   ExtensionBundleVersion="2.1.0"
                   ExtensionBundleName="Example CEP">
  <ExecutionEnvironment>
    <HostList>
      <Host Name="PHSP" Version="[23.0,99.9]"/>
      <Host Name="ILST" Version="[26.0,99.9]"/>
      <Host Name="PPRO" Version="[22.0,99.9]"/>
    </HostList>
  </ExecutionEnvironment>
</ExtensionManifest>"#;
        assert_eq!(
            xml_attribute(manifest, "ExtensionBundleId").as_deref(),
            Some("com.example.cep")
        );
        assert_eq!(
            xml_attribute(manifest, "ExtensionBundleName").as_deref(),
            Some("Example CEP")
        );
        assert_eq!(parse_cep_min_version("[23.0,99.9]").as_deref(), Some("23.0"));
        let hosts = parse_cep_hosts(manifest);
        assert_eq!(hosts.len(), 3);
        assert_eq!(hosts[0].app, "PHSP");
        assert_eq!(hosts[0].label, "Photoshop");
        assert_eq!(hosts[0].min_version.as_deref(), Some("23.0"));
        assert_eq!(hosts[1].app, "ILST");
        assert_eq!(hosts[2].app, "PPRO");
        assert_eq!(hosts[2].label, "Premiere Pro");
    }

    #[test]
    fn extracts_cep_plugin_root() {
        let root = test_directory("cep-extract");
        fs::create_dir_all(&root).unwrap();
        let archive_path = root.join("sample.zxp");
        let file = File::create(&archive_path).unwrap();
        let mut archive = ZipWriter::new(file);
        archive
            .start_file("CSXS/manifest.xml", SimpleFileOptions::default())
            .unwrap();
        archive
            .write_all(br#"<ExtensionManifest ExtensionBundleId="com.example.cep" ExtensionBundleVersion="1.0.0" Version="11.0"/>"#)
            .unwrap();
        archive
            .start_file("index.html", SimpleFileOptions::default())
            .unwrap();
        archive.write_all(b"<html></html>").unwrap();
        archive
            .start_file("META-INF/signatures.xml", SimpleFileOptions::default())
            .unwrap();
        archive.write_all(b"<signatures/>").unwrap();
        archive.finish().unwrap();

        let output = root.join("output");
        extract_cep(&archive_path, &output).unwrap();
        assert!(output.join("CSXS/manifest.xml").is_file());
        assert!(output.join("index.html").is_file());
        fs::remove_dir_all(root).unwrap();
    }
}
