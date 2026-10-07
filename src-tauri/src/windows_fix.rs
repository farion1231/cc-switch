//! Windows 专用的任务栏固定图标自愈补丁。
//!
//! ## 背景（上游 issue #5949）
//!
//! MSI 模板（`wix/per-user-main.wxs`）原本给桌面与开始菜单快捷方式写了
//! `Icon="ProductIcon"`。这种写法会让 Windows Installer 把图标提取到
//! `%APPDATA%\Microsoft\Installer\{ProductCode}\ProductIcon`，生成的 `.lnk`
//! 的 IconLocation 指向**这个缓存文件**，而不是 exe 本身。
//!
//! 用户把应用固定到任务栏时，Explorer 会把这份 `.lnk` 复制到
//! `%APPDATA%\...\User Pinned\TaskBar\`，IconLocation（含**当时那个**
//! ProductCode）被一并写死。而模板用 `Product Id="*"`（每次构建换
//! ProductCode）+ 大版本升级先卸载旧产品，卸载会把旧 ProductCode 的图标
//! 缓存目录整个删除；固定项不归 MSI 管理，不会被重写，于是 IconLocation
//! 变成死链。当前会话里 Explorer 图标缓存还留着旧位图，所以升级后当场看
//! 不出问题；等缓存重建（通常是重启）后，任务栏固定按钮就退化成空白文档
//! 图标。
//!
//! ## 本模块解决的残余问题
//!
//! 模板侧已改为不写 `Icon="ProductIcon"`，**新固定**的快捷方式从此不再
//! 失效。但**存量**固定项的坏路径已经写死在用户自己的 `.lnk` 里，模板改动
//! 救不了他们。本模块在启动时做一次性自愈：把「目标就是本进程 exe」且
//! 「图标指向已不存在的 Installer `ProductIcon` 缓存」的固定项，改写为
//! 直接指向 exe 自身（与上游 Tauri 模板的做法一致，exe 路径跨版本稳定）。
//!
//! ## 安全边界
//!
//! - 只扫描 `%APPDATA%\...\User Pinned\TaskBar\*.lnk`，不碰桌面与开始菜单
//!   快捷方式（它们由 MSI 组件管理，升级时会被正确重写）。
//! - 只修改同时满足「目标 = 本进程 exe」与「图标是 `Installer\{GUID}\ProductIcon`
//!   且该文件已不存在」两个条件的项；其余固定项一律不动。
//! - 只调用 `SetIconLocation`，快捷方式的目标、参数、工作目录、AppUserModelID
//!   等属性全部保持不变。
//! - 任何一步失败都只记日志并跳过，绝不影响启动，绝不 panic。
//! - 整个流程跑在独立线程上，调用线程立即返回。

use std::path::{Path, PathBuf};

use windows::core::{Interface, PCWSTR};
use windows::Win32::Foundation::S_OK;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED, STGM_READ,
};
use windows::Win32::UI::Shell::{IShellLinkW, ShellLink, SLGP_RAWPATH};

/// `%APPDATA%\Microsoft\Installer`，Windows Installer 的按用户图标缓存根目录。
fn installer_root() -> Option<PathBuf> {
    Some(
        PathBuf::from(std::env::var_os("APPDATA")?)
            .join("Microsoft")
            .join("Installer"),
    )
}

/// Explorer 存放用户手动固定到任务栏的快捷方式的目录。
fn pinned_taskbar_dir() -> Option<PathBuf> {
    Some(
        PathBuf::from(std::env::var_os("APPDATA")?)
            .join("Microsoft")
            .join("Internet Explorer")
            .join("Quick Launch")
            .join("User Pinned")
            .join("TaskBar"),
    )
}

/// 判断 `icon_path` 是否是 Windows Installer 为某个 ProductCode 提取的
/// `ProductIcon` 缓存文件，即 `...\Microsoft\Installer\{ProductCode}\ProductIcon`。
///
/// 只认这一层结构：必须正好比 `installer_root` 深一层、且文件名恰为
/// `ProductIcon`。这样既能覆盖 issue #5949 的失效形态，又不会误伤
/// `Installer` 目录下的其它文件（例如缓存的 `.msi`）。
///
/// 比较统一按 ASCII 小写进行：`.lnk` 里记录的路径与 `APPDATA` 环境变量的
/// 大小写不保证一致。
fn is_installer_product_icon(icon_path: &Path, installer_root: &Path) -> bool {
    let icon = icon_path.to_string_lossy().to_ascii_lowercase();
    let root = installer_root.to_string_lossy().to_ascii_lowercase();
    let Some(rest) = icon.strip_prefix(&root) else {
        return false;
    };

    let parts: Vec<&str> = rest
        .trim_start_matches(['\\', '/'])
        .split(['\\', '/'])
        .filter(|part| !part.is_empty())
        .collect();

    // 恰好 {ProductCode}\ProductIcon 两层。
    parts.len() == 2 && parts[1] == "producticon"
}

/// 判断两段路径是否指向同一个文件。
///
/// 先按 ASCII 大小写无关做字面比较；失败时退化为规范化比较，以吸收
/// 8.3 短路径、`\\?\` 前缀等差异。
fn is_same_file(target: &str, exe: &Path) -> bool {
    if target.is_empty() {
        return false;
    }

    if Path::new(target)
        .as_os_str()
        .eq_ignore_ascii_case(exe.as_os_str())
    {
        return true;
    }

    match (std::fs::canonicalize(target), std::fs::canonicalize(exe)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// 把路径转成 NUL 结尾的 UTF-16 缓冲。
fn to_wide(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;

    path.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// 从 NUL 结尾的 UTF-16 缓冲里取出字符串。
fn from_wide(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

/// 尝试修复单个固定项，返回是否真的发生了写入。
///
/// # Safety
///
/// 内部使用 COM，但所有指针都来自本函数自己持有的、生命周期覆盖调用的缓冲；
/// 接口指针在函数结束前一直有效。
fn repair_one(lnk: &Path, exe: &Path) -> Result<bool, String> {
    let lnk_wide = to_wide(lnk);
    let exe_wide = to_wide(exe);

    unsafe {
        let link: IShellLinkW = CoCreateInstance(
            &ShellLink,
            None::<&windows::core::IUnknown>,
            CLSCTX_INPROC_SERVER,
        )
        .map_err(|e| format!("CoCreateInstance 失败: {e}"))?;
        let persist: IPersistFile = link
            .cast()
            .map_err(|e| format!("取 IPersistFile 失败: {e}"))?;

        persist
            .Load(PCWSTR::from_raw(lnk_wide.as_ptr()), STGM_READ)
            .map_err(|e| format!("Load 失败: {e}"))?;

        // 只处理目标就是本进程 exe 的固定项。
        let mut target_buf = [0u16; 512];
        link.GetPath(&mut target_buf, std::ptr::null_mut(), SLGP_RAWPATH.0 as u32)
            .map_err(|e| format!("GetPath 失败: {e}"))?;
        if !is_same_file(&from_wide(&target_buf), exe) {
            return Ok(false);
        }

        let mut icon_buf = [0u16; 512];
        let mut icon_index = 0i32;
        link.GetIconLocation(&mut icon_buf, &mut icon_index)
            .map_err(|e| format!("GetIconLocation 失败: {e}"))?;
        let icon = from_wide(&icon_buf);
        if icon.is_empty() {
            // 没有显式图标路径的固定项本来就用 exe 图标，无需处理。
            return Ok(false);
        }

        let Some(root) = installer_root() else {
            return Ok(false);
        };
        let icon_path = PathBuf::from(&icon);
        if !is_installer_product_icon(&icon_path, &root) || icon_path.exists() {
            return Ok(false);
        }

        link.SetIconLocation(PCWSTR::from_raw(exe_wide.as_ptr()), 0)
            .map_err(|e| format!("SetIconLocation 失败: {e}"))?;
        persist
            .Save(PCWSTR::from_raw(lnk_wide.as_ptr()), true)
            .map_err(|e| format!("Save 失败: {e}"))?;

        Ok(true)
    }
}

/// 对指定目录执行一次自愈，返回修复的快捷方式数量。
///
/// `exe` 用于判定某个固定项是否属于本应用；抽成参数是为了让测试能对着临时
/// 目录跑，而不必碰用户真实的固定项。
pub(crate) fn heal_pinned_dir(dir: &Path, exe: &Path) -> Result<usize, String> {
    if !dir.is_dir() {
        return Ok(0);
    }

    let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    if hr.is_err() {
        return Err(format!("CoInitializeEx 失败: 0x{:08X}", hr.0));
    }
    // S_FALSE 表示该线程上 COM 已经初始化过，此时不能再 CoUninitialize。
    let owns_com = hr == S_OK;

    let result = scan_and_repair(dir, exe);

    if owns_com {
        unsafe { CoUninitialize() };
    }

    result
}

fn scan_and_repair(dir: &Path, exe: &Path) -> Result<usize, String> {
    let entries =
        std::fs::read_dir(dir).map_err(|e| format!("读取 {} 失败: {e}", dir.display()))?;

    let mut repaired = 0usize;
    for entry in entries.flatten() {
        let path = entry.path();
        let is_lnk = path
            .extension()
            .map(|ext| ext.eq_ignore_ascii_case("lnk"))
            .unwrap_or(false);
        if !is_lnk {
            continue;
        }

        match repair_one(&path, exe) {
            Ok(true) => {
                log::info!("Windows 固定图标自愈: 已修复 {}", path.display());
                repaired += 1;
            }
            Ok(false) => {}
            Err(e) => {
                log::warn!("Windows 固定图标自愈: 跳过 {} ({e})", path.display());
            }
        }
    }

    Ok(repaired)
}

/// 同步执行一次自愈（针对当前用户真实的固定项目录），返回修复数量。
pub(crate) fn heal_once() -> Result<usize, String> {
    let exe = std::env::current_exe().map_err(|e| format!("取 current_exe 失败: {e}"))?;

    let Some(dir) = pinned_taskbar_dir() else {
        return Ok(0);
    };

    heal_pinned_dir(&dir, &exe)
}

/// 启动时做一次性自愈：把指向已删除的 Installer `ProductIcon` 缓存的
/// 任务栏固定项改写为直接指向本进程 exe。
///
/// fire-and-forget：内部起独立线程，调用线程立即返回，不阻塞启动。
pub(crate) fn repair_stale_taskbar_pin_icons() {
    let spawned = std::thread::Builder::new()
        .name("cc-switch-pin-icon-heal".to_string())
        .spawn(|| match heal_once() {
            Ok(0) => log::debug!("Windows 固定图标自愈: 无需修复"),
            Ok(n) => log::info!("Windows 固定图标自愈: 已修复 {n} 个失效的任务栏固定项"),
            Err(e) => log::warn!("Windows 固定图标自愈失败: {e}"),
        });

    if let Err(e) = spawned {
        log::warn!("Windows 固定图标自愈: 无法启动线程 ({e})");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> PathBuf {
        PathBuf::from(r"C:\Users\demo\AppData\Roaming\Microsoft\Installer")
    }

    #[test]
    fn recognizes_installer_product_icon() {
        let icon = root()
            .join("{09CC399B-909F-47A2-BC3E-0009F440BFB9}")
            .join("ProductIcon");
        assert!(is_installer_product_icon(&icon, &root()));
    }

    #[test]
    fn matches_case_insensitively() {
        let icon =
            PathBuf::from(r"c:\users\demo\appdata\roaming\microsoft\installer\{abc}\producticon");
        assert!(is_installer_product_icon(&icon, &root()));
    }

    #[test]
    fn rejects_wrong_depth() {
        // 直接在根目录下
        assert!(!is_installer_product_icon(
            &root().join("ProductIcon"),
            &root()
        ));
        // 比预期多一层
        assert!(!is_installer_product_icon(
            &root().join("{abc}").join("nested").join("ProductIcon"),
            &root()
        ));
    }

    #[test]
    fn rejects_other_files_and_unrelated_paths() {
        // Installer 目录下的缓存 msi
        assert!(!is_installer_product_icon(
            &root().join("{abc}").join("{abc}.msi"),
            &root()
        ));
        // 文件名近似但不同
        assert!(!is_installer_product_icon(
            &root().join("{abc}").join("ProductIcon2"),
            &root()
        ));
        // 与本机制无关的路径
        assert!(!is_installer_product_icon(
            Path::new(r"D:\CCswith\cc-switch.exe"),
            &root()
        ));
    }
}
