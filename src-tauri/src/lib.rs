use serde::Serialize;
use serde_json::{json, Value};
use std::{
    fs::{self, File, OpenOptions},
    io::Read,
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::Duration,
    time::{SystemTime, UNIX_EPOCH},
};
use zip::ZipArchive;

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
    issues: Vec<PermissionIssue>,
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
        if let Some(common) = std::env::var_os("CommonProgramFiles") {
            paths.push(PathBuf::from(common).join(relative));
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
    for _ in 0..20 {
        if creative_cloud_is_running() {
            return Ok(true);
        }
        thread::sleep(Duration::from_millis(500));
    }
    Ok(true)
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
        "PS" | "PHSP" => ("Photoshop".into(), "/apps/photoshop.svg".into()),
        "ID" | "IDSN" => ("InDesign".into(), "/apps/indesign.svg".into()),
        "PR" | "PPRO" => ("Premiere Pro".into(), "/apps/premiere%20pro.svg".into()),
        "XD" => ("Adobe XD".into(), "/apps/xd.svg".into()),
        "AI" | "ILST" => ("Illustrator".into(), "/apps/illustrator.svg".into()),
        "AE" | "AEFT" => ("After Effects".into(), "/apps/after%20effects.svg".into()),
        "AU" | "AUDT" => ("Audition".into(), "/apps/audition.svg".into()),
        "BR" | "KBRG" => ("Bridge".into(), "/apps/bridge.svg".into()),
        "ACROBAT" | "ACRO" => ("Acrobat Pro".into(), "/apps/Acrobat%20Pro.svg".into()),
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

fn read_package(path: &Path) -> Result<PluginPackage, String> {
    if path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.eq_ignore_ascii_case("ccx"))
        != Some(true)
    {
        return Err("文件格式不受支持，请选择扩展名为 .ccx 的安装包。".into());
    }
    if !path.is_file() {
        return Err("找不到所选的 CCX 文件。".into());
    }

    let file = File::open(path).map_err(|error| format!("无法打开安装包：{error}"))?;
    let file_size = file.metadata().map(|metadata| metadata.len()).unwrap_or(0);
    let mut archive = ZipArchive::new(file).map_err(|_| "CCX 安装包无效或已损坏。".to_string())?;
    let manifest_index = (0..archive.len())
        .find(|index| {
            archive
                .by_index(*index)
                .ok()
                .is_some_and(|entry| entry.name().to_ascii_lowercase().ends_with("manifest.json"))
        })
        .ok_or_else(|| "安装包中没有找到 UXP manifest.json。".to_string())?;

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
    let manifest: Value = serde_json::from_str(contents.trim_start_matches('\u{feff}'))
        .map_err(|error| format!("插件清单格式无效：{error}"))?;

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
    })
}

fn supports_photoshop_sideload(package: &PluginPackage) -> bool {
    package
        .hosts
        .iter()
        .any(|host| matches!(host.app.to_ascii_uppercase().as_str(), "PS" | "PHSP"))
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

fn build_sideload_preflight(package: &PluginPackage) -> SideloadPreflight {
    let mut issues = Vec::new();
    let supported = cfg!(any(target_os = "macos", target_os = "windows"))
        && supports_photoshop_sideload(package);
    let base = match sideload_base() {
        Ok(base) => base,
        Err(message) => {
            issues.push(PermissionIssue {
                path: String::new(),
                message,
                hint: "侧载目前仅支持 macOS 和 Windows 上的 Photoshop。".into(),
            });
            return SideloadPreflight {
                supported: false,
                ready: false,
                plugin_directory: String::new(),
                registry_path: String::new(),
                issues,
            };
        }
    };
    let plugin_directory = base.join("Plugins/External");
    let registry_directory = base.join("PluginsInfo/v1");
    let registry_path = registry_directory.join("PS.json");

    if !supports_photoshop_sideload(package) {
        issues.push(PermissionIssue {
            path: String::new(),
            message: "该安装包未声明支持 Photoshop。".into(),
            hint: "第一版侧载仅支持 manifest 中宿主为 PS 的插件。".into(),
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
    if registry_path.exists() {
        if let Err(error) = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&registry_path)
        {
            issues.push(PermissionIssue {
                path: registry_path.to_string_lossy().into_owned(),
                message: format!("Photoshop 注册文件权限不足：{error}"),
                hint: permission_hint(),
            });
        }
    }

    SideloadPreflight {
        supported,
        ready: supported && issues.is_empty(),
        plugin_directory: plugin_directory.to_string_lossy().into_owned(),
        registry_path: registry_path.to_string_lossy().into_owned(),
        issues,
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

fn extract_ccx(path: &Path, destination: &Path) -> Result<(), String> {
    let file = File::open(path).map_err(|error| format!("无法打开 CCX：{error}"))?;
    let mut archive = ZipArchive::new(file).map_err(|_| "CCX 安装包无效或已损坏。".to_string())?;
    if archive.len() > 10_000 {
        return Err("安装包文件数量异常，已停止侧载。".into());
    }

    let mut manifest_path = None;
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|error| format!("无法检查安装包内容：{error}"))?;
        if entry.name().to_ascii_lowercase().ends_with("manifest.json") {
            manifest_path = entry.enclosed_name();
            break;
        }
    }
    let manifest_path =
        manifest_path.ok_or_else(|| "安装包中没有安全的 manifest.json 路径。".to_string())?;
    let manifest_root = manifest_path
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .to_path_buf();
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
    if !destination.join("manifest.json").is_file() {
        return Err("侧载目录中没有找到 manifest.json。".into());
    }
    Ok(())
}

#[tauri::command]
fn check_environment() -> Environment {
    let installer = find_upia();
    #[cfg(target_os = "macos")]
    let creative_cloud_found =
        Path::new("/Applications/Adobe Creative Cloud/ACC/Creative Cloud.app").exists()
            || Path::new("/Applications/Utilities/Adobe Creative Cloud/ACC/Creative Cloud.app")
                .exists();
    #[cfg(target_os = "windows")]
    let creative_cloud_found = std::env::var_os("ProgramFiles").is_some_and(|root| {
        PathBuf::from(root)
            .join("Adobe/Adobe Creative Cloud/ACC/Creative Cloud.exe")
            .exists()
    });
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let creative_cloud_found = false;

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
        .add_filter("Adobe UXP 安装包", &["ccx"])
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
    let can_side_load = supports_photoshop_sideload(&package);
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
    let photoshop_was_running = photoshop_is_running();

    let mut command = Command::new(installer);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
        command.arg("/install");
    }
    #[cfg(not(target_os = "windows"))]
    command.arg("--install");
    let output = match command.arg(&package_path).output() {
        Ok(output) => output,
        Err(error) => {
            return Ok(InstallResult {
                success: false,
                message: format!("无法启动 Adobe 安装服务：{error}"),
                details: None,
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
            message: if photoshop_was_running {
                "插件已安装；请完全退出并重新打开 Photoshop 以启用插件。".into()
            } else {
                "插件已成功安装，打开 Photoshop 即可载入。".into()
            },
            details: (!details.is_empty()).then_some(details),
            can_side_load: false,
            activation_pending: photoshop_was_running,
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

#[tauri::command]
fn sideload_ccx(path: String) -> Result<InstallResult, String> {
    let package_path = PathBuf::from(&path);
    let package = read_package(&package_path)?;
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
    let registry_path = PathBuf::from(&preflight.registry_path);
    let registry_directory = registry_path
        .parent()
        .ok_or_else(|| "Photoshop 注册路径无效。".to_string())?;
    fs::create_dir_all(&plugin_directory)
        .map_err(|error| format!("无法创建 Photoshop 插件目录：{error}"))?;
    fs::create_dir_all(registry_directory)
        .map_err(|error| format!("无法创建 Photoshop 注册目录：{error}"))?;

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

    let old_registry = if registry_path.exists() {
        match fs::read(&registry_path) {
            Ok(contents) => Some(contents),
            Err(error) => {
                let _ = fs::remove_dir_all(&staging);
                return Err(format!("无法读取 Photoshop 注册文件：{error}"));
            }
        }
    } else {
        None
    };
    let mut registry: Value = match old_registry.as_deref() {
        Some(contents) if !contents.is_empty() => match serde_json::from_slice(contents) {
            Ok(registry) => registry,
            Err(error) => {
                let _ = fs::remove_dir_all(&staging);
                return Err(format!("PS.json 格式无效，未执行侧载：{error}"));
            }
        },
        _ => json!({ "plugins": [] }),
    };
    let Some(plugins) = registry.get_mut("plugins").and_then(Value::as_array_mut) else {
        let _ = fs::remove_dir_all(&staging);
        return Err("PS.json 缺少 plugins 数组，未执行侧载。".into());
    };
    plugins.retain(|entry| entry.get("pluginId").and_then(Value::as_str) != Some(plugin_id));
    let host_min_version = package
        .hosts
        .iter()
        .find(|host| matches!(host.app.to_ascii_uppercase().as_str(), "PS" | "PHSP"))
        .and_then(|host| host.min_version.clone())
        .unwrap_or_else(|| "22.0.0".into());
    let registry_relative_path = if cfg!(target_os = "windows") {
        format!("$localPlugins\\External\\{folder_name}")
    } else {
        format!("$localPlugins/External/{folder_name}")
    };
    plugins.push(json!({
        "hostMinVersion": host_min_version,
        "name": package.name,
        "path": registry_relative_path,
        "pluginId": plugin_id,
        "status": "enabled",
        "type": "uxp",
        "versionString": package.version,
    }));
    let registry_contents = serde_json::to_vec_pretty(&registry)
        .map_err(|error| format!("无法生成 Photoshop 注册信息：{error}"))?;

    let target = plugin_directory.join(&folder_name);
    let old_target = plugin_directory.join(format!(".openuxp-previous-{folder_name}-{nonce}"));
    let had_target = target.exists();
    if had_target {
        if let Err(error) = fs::rename(&target, &old_target) {
            let _ = fs::remove_dir_all(&staging);
            return Ok(InstallResult {
                success: false,
                message: "无法替换现有插件，请关闭 Photoshop 后重试。".into(),
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

    let backup_path = old_registry
        .as_ref()
        .map(|_| registry_directory.join(format!("PS.json.openuxp-backup-{nonce}")));
    if let (Some(contents), Some(backup)) = (&old_registry, &backup_path) {
        if let Err(error) = fs::write(backup, contents) {
            let _ = fs::remove_dir_all(&target);
            if had_target {
                let _ = fs::rename(&old_target, &target);
            }
            return Err(format!("无法备份 PS.json，已取消侧载：{error}"));
        }
    }
    if let Err(error) = fs::write(&registry_path, registry_contents) {
        if let Some(contents) = &old_registry {
            let _ = fs::write(&registry_path, contents);
        } else {
            let _ = fs::remove_file(&registry_path);
        }
        let _ = fs::remove_dir_all(&target);
        if had_target {
            let _ = fs::rename(&old_target, &target);
        }
        return Ok(InstallResult {
            success: false,
            message: "写入 Photoshop 注册信息失败，已回滚插件文件。".into(),
            details: Some(error.to_string()),
            can_side_load: true,
            activation_pending: false,
        });
    }
    if had_target {
        let _ = fs::remove_dir_all(&old_target);
    }

    let mut detail_lines = vec![
        format!("安装目录：{}", target.to_string_lossy()),
        format!("注册文件：{}", registry_path.to_string_lossy()),
    ];
    if let Some(backup) = backup_path {
        detail_lines.push(format!("注册文件备份：{}", backup.to_string_lossy()));
    }
    Ok(InstallResult {
        success: true,
        message: "插件已通过侧载安装到 Photoshop，请重启 Photoshop。".into(),
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
}
