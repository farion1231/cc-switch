//! Linux 专用的 GTK 输入法模块缓存兼容补丁。
//!
//! linuxdeploy-plugin-gtk 生成的 AppRun 会**无条件**导出两个变量：
//!
//! ```text
//! export GTK_EXE_PREFIX="$APPDIR/<exec_prefix>"
//! export GTK_IM_MODULE_FILE="$APPDIR/<libdir>/gtk-3.0/3.0.0/immodules.cache"
//! ```
//!
//! `<libdir>` 的两种真实形态：扁平布局 `usr/lib`（Arch / Gentoo）与
//! multiarch 布局 `usr/lib/x86_64-linux-gnu`（Debian / Ubuntu——
//! linuxdeploy-plugin-gtk 在 x86_64 Ubuntu 上拼出的就是这一种）。本模块的
//! 主机缓存探测列表与镜像候选都覆盖这两类形态。
//!
//! 而该 bundle 缓存里只有 GTK 自带的少数输入模块（am-et、broadway、cedilla、
//! cyrillic-translit、ipa、multipress、thai、ti-er、ti-et、viqr、wayland、
//! waylandgtk、xim），**没有任何 fcitx / ibus / scim 条目**，AppDir 里也没有
//! `immodules/` 目录可以补齐。用户配了 `GTK_IM_MODULE=fcitx` 时，GTK 在这个
//! 缓存里查无此模块，会按当前 locale 回退到 XIM（`im-xim.so`）；编辑页的文本框
//! 一获得焦点，WebKitGTK 就走进了 XIM 路径，整个应用卡死，只能 `SIGKILL`
//! （issue #7746）。
//!
//! # 为什么不能只把变量摘掉
//!
//! 同一个钩子还导出了 `GTK_EXE_PREFIX`，而 GTK 自己推导默认缓存路径时读的
//! 正是它（gtkrc.c 的 `gtk_rc_make_default_dir()`：`GTK_EXE_PREFIX` +
//! `lib/gtk-3.0/<版本>/immodules.cache`；Chromium 的
//! `GetGtk3ImModulesCacheFile()` 复刻了同一套查找顺序）。所以：
//!
//! - **扁平布局**（`exec_prefix=/usr`）：摘掉变量后 GTK 读到的正是同一份
//!   bundle 缓存 → 什么都没变，卡死依旧；
//! - **multiarch 布局**（`libdir=/usr/lib/x86_64-linux-gnu`）：该路径在
//!   AppDir 里根本不存在 → GTK 一个输入法模块都加载不到，退回内置 simple。
//!   不崩了，但那是"碰巧不崩"：用户自己的输入法一个也用不上。
//!
//! 正解是**主动把变量指向主机上真正列出用户输入法模块的缓存**：那份缓存在
//! AppDir 之外，GTK 从它加载 `im-fcitx5.so` / `im-ibus.so`，完全不碰 XIM。
//! 一条都找不到时**什么都不做**——随机指一个路径只会让情况更糟。
//!
//! # 修好之后用户会得到什么
//!
//! 卡死一定消失（XIM 回退这条路径被绕开了），但输入法本身是否可用取决于主机
//! 的 `im-fcitx5.so` / `im-ibus.so` 能否在 AppImage 自带的 GTK 版本下加载：
//! 加载不了（GTK 微版本不同等常见情形）时，GTK 会退回内置的 `simple` 上下文
//! ——不崩，但输入法仍然不可用。#7746 报告者自己的机器就是这种结局，所以这是
//! **预期结果而不是回归**：修之前是卡死，修之后最差也是"能退出 + 输入法没有"。
//! 需要保留钩子原行为时，`CC_SWITCH_GTK_IM_MODULE_FILE=keep` 是显式退路。
//!
//! # 设计哲学（与 `main.rs` 里 `CC_SWITCH_GDK_BACKEND` 一致）
//!
//! - 只针对「确实命中该缺陷」的组合动手，默认行为零回归；
//! - 只看 `GTK_IM_MODULE`：GTK 挑输入法模块时只读它与 XSETTINGS，**不读
//!   `XMODIFIERS`**（gtkimmodule.c 的 `_gtk_im_module_get_default_context_id()`）。
//!   所以只配了 `XMODIFIERS=@im=fcitx` 的机器不在本补丁覆盖范围内——那种配置下
//!   GTK 会直接走 locale 兜底命中 `xim`，改写成哪个缓存文件都改变不了这一点；
//! - 钩子不会触碰一个钩子外的显式逃生开关 `CC_SWITCH_GTK_IM_MODULE_FILE`；
//! - 决策逻辑全部放在 [`resolve_im_module_file_action`]（纯函数、可单测），
//!   读环境变量、读缓存文件的副作用隔离在 `apply_gtk_im_module_file_fix()`
//!   （仅 Linux 编译，故此处用普通行内代码而非可跳转链接）。

#[cfg(target_os = "linux")]
use std::env;

/// 显式逃生开关（钩子不会触碰的变量名）。
///
/// 取值（两端空白会被忽略，`keep` 大小写不敏感）：
/// - `keep`：完全相信钩子写入的 `GTK_IM_MODULE_FILE`，什么也不改；
/// - 其它非空值：强制把 `GTK_IM_MODULE_FILE` 改写成该路径；
/// - 未设置 / 空串：走自动判定（默认，零回归）。
const IM_MODULE_FILE_OVERRIDE_ENV: &str = "CC_SWITCH_GTK_IM_MODULE_FILE";

/// `GTK_IM_MODULE` 里这些 id 不需要外部缓存就能解析：GTK 3.24 的
/// `lookup_immodule()` 认的是 `gtk-im-context-simple` / `gtk-im-context-none`
/// 这两个内置 id，`xim` 则由 bundle 缓存自己列出。用户显式选它们时，我们不动
/// 环境变量。
///
/// 注意这里**不是**裸的 `simple` / `none`：GTK 并不认这两个短名，`GTK_IM_MODULE=simple`
/// 会被当作查无此模块，随后在 CJK locale 下被 locale 兜底解析成 `xim`（卡死路径）。
/// 这类请求任何主机缓存都不可能列出，结局同样是保守保持；把它们算作"已满足"只是
/// 为了让诊断说得到位（`Keep` 而不是 `KeepWithoutHostCache`）。
const NO_OP_MODULES: [&str; 3] = ["gtk-im-context-simple", "gtk-im-context-none", "xim"];

/// 主机系统里可能的 GTK 输入法缓存位置（全部在 AppDir 之外）。
///
/// 每个候选都必须可读、且列出用户请求的模块才会被选中。顺序的影响不止于
/// 「多个有效缓存时用哪个」：先列出 `GTK_IM_MODULE` 冒号列表里**第一个**
/// 模块的候选会胜出，只列出靠后模块的候选只会作为兜底——所以顺序会影响用户
/// 最终拿到哪个输入法。覆盖三种已知布局：`/usr/lib`（Arch / Gentoo 等扁平
/// 布局）、`/usr/lib64`（Fedora / openSUSE）、`*-linux-gnu`（Debian 系
/// multiarch，x86_64 与 arm64），外加本机编译安装的 `/usr/local`。
const HOST_IM_MODULE_CACHES: [&str; 6] = [
    "/usr/lib/gtk-3.0/3.0.0/immodules.cache",
    "/usr/lib64/gtk-3.0/3.0.0/immodules.cache",
    "/usr/lib/x86_64-linux-gnu/gtk-3.0/3.0.0/immodules.cache",
    "/usr/lib/aarch64-linux-gnu/gtk-3.0/3.0.0/immodules.cache",
    "/usr/local/lib/gtk-3.0/3.0.0/immodules.cache",
    "/usr/local/lib64/gtk-3.0/3.0.0/immodules.cache",
];

/// 对进程环境里 `GTK_IM_MODULE_FILE` 的处置决定。
#[derive(Debug, Clone, PartialEq, Eq)]
enum ImModuleFileAction {
    /// 保持现状：不摘除、不改写。
    Keep,
    /// 把 `GTK_IM_MODULE_FILE` 指向给定路径（自动判定时是主机缓存）。
    Set(String),
    /// bundle 缓存服务不了用户请求的模块，主机上也找不到能服务的缓存。
    /// 行为等同 [`ImModuleFileAction::Keep`]，只是把原因带给调用方去记录——
    /// 启动阶段还没有日志可用，一条明确的说明比沉默便宜得多。
    KeepWithoutHostCache(String),
}

/// 纯决策函数：给定逃生开关、当前缓存路径、AppDir、用户输入法配置，以及两个
/// 查询回调，决定怎么处理 `GTK_IM_MODULE_FILE`。不接触进程环境和文件系统。
///
/// - `bundle_lists`：bundle 缓存是否列出某模块。三值——`Some(true)` 列出、
///   `Some(false)` 缓存可读但没有该模块、`None` 缓存读不到。`None` 一律按
///   「不干预」处理：我们只在有正面证据时才改用户环境。
/// - `host_cache_for`：按候选顺序找主机缓存。返回 `None` 表示一台都没有。
fn resolve_im_module_file_action(
    override_value: Option<&str>,
    gtk_im_module: Option<&str>,
    current: Option<&str>,
    appdir: Option<&str>,
    bundle_lists: impl Fn(&str) -> Option<bool>,
    host_cache_for: impl Fn(&[&str]) -> Option<String>,
) -> ImModuleFileAction {
    // 1) 显式逃生开关优先，与 CC_SWITCH_GDK_BACKEND 同一风格：先 trim，
    //    `keep` 大小写不敏感，空串/纯空白按未设置处理（继续走自动判定）。
    let override_value = override_value.map(str::trim).unwrap_or_default();
    if override_value.eq_ignore_ascii_case("keep") {
        return ImModuleFileAction::Keep;
    }
    if !override_value.is_empty() {
        return ImModuleFileAction::Set(override_value.to_string());
    }

    // 2) 钩子没设 GTK_IM_MODULE_FILE（非 AppImage 启动）时无事可做。
    let current = match current.map(str::trim).filter(|c| !c.is_empty()) {
        Some(current) => current,
        None => return ImModuleFileAction::Keep,
    };

    // 3) 只处理指向 AppDir 内 bundle 缓存的路径：APPDIR 由 AppRun 导出，
    //    缺失或路径不在其下说明不是 AppImage 启动（deb / rpm / 自行解包），
    //    直接不碰。
    let appdir = match appdir.map(str::trim).filter(|d| !d.is_empty()) {
        Some(appdir) => appdir,
        None => return ImModuleFileAction::Keep,
    };
    if !is_appdir_relative(current, appdir) {
        return ImModuleFileAction::Keep;
    }

    // 4) 用户请求的模块列表，见 requested_im_modules()。
    let wanted = match requested_im_modules(gtk_im_module) {
        Some(modules) => modules,
        None => return ImModuleFileAction::Keep,
    };

    // 5) GTK_IM_MODULE 是冒号分隔的有序列表：任一项能被解析，GTK 就不会走到
    //    「查无此模块」的分支，所以只要我们不用介入。"能被解析"包括两类——
    //    内置 id / bundle 自带（gtk-im-context-simple / gtk-im-context-none /
    //    xim，无需外部缓存）与 bundle 缓存里确实列出的项。缓存读不到（None）时
    //    没有正面证据，保守保持。
    let mut served = false;
    for module in wanted.iter().copied() {
        let listed = if NO_OP_MODULES.contains(&module) {
            Some(true)
        } else {
            bundle_lists(module)
        };
        match listed {
            Some(true) => served = true,
            Some(false) => {}
            None => return ImModuleFileAction::Keep,
        }
    }
    if served {
        return ImModuleFileAction::Keep;
    }

    // 6) 剩下每一项都依赖外部缓存：主动指向第一个能服务它们的主机缓存；
    //    一台都找不到就保持现状——猜一个路径只会让情况更糟。
    let requested = wanted.join(":");
    match host_cache_for(&wanted) {
        Some(path) => ImModuleFileAction::Set(path),
        None => ImModuleFileAction::KeepWithoutHostCache(requested),
    }
}

/// 判断 `current` 是否指向 `appdir` 内的文件。
///
/// 就是「去掉 `appdir` 尾部斜杠后做一次字符串前缀比较」：钩子写出的是
/// `$APPDIR/usr/...`，容许多写一个 `/` 的形态，也容忍 APPDIR 带尾斜杠。
/// 不做路径规范化（不解析 `..`、不展开软链），判定不了就当作"不是 bundle
/// 缓存"，走保守分支。
fn is_appdir_relative(current: &str, appdir: &str) -> bool {
    let appdir = appdir.trim_end_matches('/');
    if appdir.is_empty() {
        return false;
    }
    let with_sep = format!("{appdir}/");
    current.starts_with(&with_sep)
}

/// 解析用户请求的输入法模块列表。
///
/// `GTK_IM_MODULE` 是冒号分隔的有序列表（GTK 用 `g_strsplit(env, ":")` 逐项
/// 解析，解析不了的项跳过），所以这里也按冒号切分。
///
/// **不 trim**：GTK 用 `g_strcmp0` 精确匹配，`" fcitx "` 在 GTK 眼里不是一个
/// 能解析的模块名。我们若按 trim 后的值去猜"用户想用 fcitx"，只会改写出一个对
/// GTK 毫无帮助的环境变量——它照样解析失败、照样落到 XIM。所以宁可原样带着这些
/// 空格：主机缓存同样匹配不上，最终保守保持，不白改用户环境。
///
/// **但空项必须丢掉**（连续冒号 / 首尾冒号产生的那种）。真实缓存的 locale 字段
/// 里有大量空串 `""`，若把空项也送进缓存判定，它会被当成"已被列出"→ 误判为已
/// 满足 → 该修的也没修。GTK 自己跳过这些空项（lookup 必然失败），我们同样跳过。
///
/// 只有在**整体**未配置（未设置 / 空串 / 纯空白）时返回 `None`。
fn requested_im_modules(gtk_im_module: Option<&str>) -> Option<Vec<&str>> {
    let raw = gtk_im_module.unwrap_or_default();
    if raw.trim().is_empty() {
        return None;
    }
    let listed: Vec<&str> = raw.split(':').filter(|module| !module.is_empty()).collect();
    if listed.is_empty() {
        return None;
    }
    Some(listed)
}

/// 缓存是否已列出名为 `module` 的输入模块（启发式判定）。
///
/// 真实缓存（`gtk-query-immodules` 输出，GTK 3 的 `gtk_im_module_initialize()`
/// 解析）里，**每个模块占一个块**：第一行只有 `.so` 路径（路径后不能再有任何
/// 内容——`gtk_skip_space()` 必须失败，否则该行被当作垃圾丢掉），第二行是恰好
/// 五个带引号字段（id / 显示名 / domain / locale 目录 / locale 列表），块与块
/// 之间以空行结束并分隔：
///
/// ```text
/// "/usr/lib/x86_64-linux-gnu/gtk-3.0/3.0.0/immodules/im-fcitx5.so"
/// "fcitx5" "Fcitx5" "gtk30" "/usr/share/locale" "@fcitx5"
///
/// "/usr/lib/x86_64-linux-gnu/gtk-3.0/3.0.0/immodules/im-xim.so"
/// "xim" "X Input Method" "gtk30" "/usr/share/locale" "@xim"
/// ```
///
/// 判定方式是在整份文本里查**带引号的整名** `"fcitx"`，而不是裸子串：裸子串会
/// 让缓存里的 `"fcitx5"` 命中用户要的 `fcitx`，从而漏掉本该修的情况（症状照旧，
/// 而我们以为已经修掉）。带引号匹配的方向相反——最多误判"没有"，结果只是保持
/// 现状，不会把坏缓存当成好缓存。它也天然不会把显示名里的 `(fcitx)` 当成模块名
/// （括号前的字符不是引号）。
///
/// 已知启发式局限：这是"整份文本的子串"判定，不是字段级解析。模块名若恰好落
/// 在**非 id 字段**里（例如某个模块的 locale 列表写成 `"fcitx"`），这里会误判
/// 成"已列出"——方向上仍是漏修而不是错改，所以可接受；真要字段级判定就得引入
/// 真正的缓存解析器，对这个补丁来说不值得。
///
/// 调用方需保证只在缓存文件读到了之后才调用（读不到应传 `None`）。
fn cache_lists_module(content: &str, module: &str) -> bool {
    content.contains(&format!("\"{module}\""))
}

/// 组装主机缓存候选：先试由 bundle 路径反推的同布局路径，再试静态列表。
/// 纯函数，便于单测。
///
/// 镜像候选来自打包机的布局——linuxdeploy 按打包机的 libdir 拼 bundle 路径，
/// 而多数用户与打包机同为 Debian/Ubuntu 或同为 Arch/Fedora，所以它通常就是
/// 用户机器上正确的那个。猜错也不过是一次 `read` 失败（文件不存在）。
fn host_cache_candidates(current: &str, appdir: &str) -> Vec<String> {
    let mut candidates: Vec<String> = Vec::with_capacity(HOST_IM_MODULE_CACHES.len() + 1);
    if let Some(mirrored) = mirrored_host_cache(current, appdir) {
        candidates.push(mirrored);
    }
    for candidate in HOST_IM_MODULE_CACHES {
        if !candidates.iter().any(|existing| existing == candidate) {
            candidates.push(candidate.to_string());
        }
    }
    candidates
}

/// 把 bundle 内的缓存路径映射回主机：剥掉 APPDIR 前缀后剩下的就是打包机上的
/// 绝对路径（如 `/usr/lib/x86_64-linux-gnu/gtk-3.0/3.0.0/immodules.cache`）。
///
/// 只接受 `/usr` 前缀（含 `/usr/local`）：既覆盖所有已知布局，也顺带挡掉
/// "剥完还在 AppDir 里"的退化形态（APPDIR 本身以 `/usr` 开头时）。别的
/// 前缀（`/opt` 之类）交给静态候选列表。
fn mirrored_host_cache(current: &str, appdir: &str) -> Option<String> {
    let appdir = appdir.trim_end_matches('/');
    if appdir.is_empty() {
        return None;
    }
    let remainder = current.strip_prefix(appdir)?;
    let mirrored = format!("/{}", remainder.trim_start_matches('/'));
    if !mirrored.starts_with("/usr/") || mirrored.starts_with(appdir) {
        return None;
    }
    Some(mirrored)
}

/// 在 GTK 初始化前修正 `GTK_IM_MODULE_FILE`：钩子写入的 bundle 缓存服务不了
/// 用户请求的输入法模块时，把它改指向主机上真正列出该模块的缓存。
/// 只在 Linux 编译。
#[cfg(target_os = "linux")]
pub fn apply_gtk_im_module_file_fix() {
    let override_value = env::var(IM_MODULE_FILE_OVERRIDE_ENV).ok();
    let current = env::var("GTK_IM_MODULE_FILE").ok();
    let appdir = env::var("APPDIR").ok();
    let gtk_im_module = env::var("GTK_IM_MODULE").ok();

    // 先用纯字符串判断把「不可能需要文件」的组合挡掉：非 AppImage 启动
    // （没有 APPDIR / 变量没设 / 变量指向 AppDir 之外）、显式用了逃生开关、
    // 或者压根没配非内置输入法模块时，连一次 read 都不做。判定条件与下面
    // resolve 的前四步一致，只是为了把 IO 挡在最前面。
    let override_value = override_value.as_deref().map(str::trim).unwrap_or_default();
    let current_path = current.as_deref().map(str::trim).filter(|p| !p.is_empty());
    let appdir = appdir.as_deref().map(str::trim).filter(|d| !d.is_empty());
    let bundle_path = match (current_path, appdir) {
        (Some(path), Some(dir)) if is_appdir_relative(path, dir) => Some(path),
        _ => None,
    };
    let modules = requested_im_modules(gtk_im_module.as_deref());
    let wants_host_cache = modules
        .as_deref()
        .unwrap_or_default()
        .iter()
        .any(|module| !NO_OP_MODULES.contains(module));
    let probe_files = override_value.is_empty() && bundle_path.is_some() && wants_host_cache;

    // bundle 缓存与主机候选都是惰性的：上面的前置判断不通过时，前者保持
    // None（resolve 据此判断"没有正面证据"），探到主机缓存时才会逐个读。
    let bundle_content = if probe_files {
        bundle_path.and_then(read_cache_content)
    } else {
        None
    };
    let host_candidates = if probe_files {
        match (bundle_path, appdir) {
            (Some(path), Some(dir)) => host_cache_candidates(path, dir),
            _ => Vec::new(),
        }
    } else {
        Vec::new()
    };

    let action = resolve_im_module_file_action(
        Some(override_value),
        gtk_im_module.as_deref(),
        current.as_deref(),
        appdir,
        |module| bundle_lists(bundle_content.as_deref(), module),
        |list| find_host_cache(&host_candidates, list),
    );

    // tauri 的日志插件在这个时点还没注册：它在 lib.rs 的 .setup() 里初始化，
    // 而 setup 发生在 builder.build()，晚于当前这次调用。所以诊断只能写
    // stderr——用 log::info! 会被静默丢掉（main.rs 里既有几行也不打日志）。
    match action {
        ImModuleFileAction::Keep => {}
        ImModuleFileAction::Set(path) => {
            eprintln!("[cc-switch] GTK_IM_MODULE_FILE 已指向 {path}");
            env::set_var("GTK_IM_MODULE_FILE", path);
        }
        ImModuleFileAction::KeepWithoutHostCache(modules) => {
            eprintln!("[cc-switch] 主机 GTK 缓存没有列出 {modules}，保持默认行为");
        }
    }
}

/// 把「bundle 缓存读到了 / 没读到」折叠成 resolve 要的三值结果。
#[cfg(target_os = "linux")]
fn bundle_lists(cached: Option<&str>, module: &str) -> Option<bool> {
    cached.map(|content| cache_lists_module(content, module))
}

/// 按候选顺序找主机缓存：优先能服务**第一个**请求模块的候选（`GTK_IM_MODULE`
/// 的冒号列表有优先级顺序，GTK 取第一个能解析的项），找不到再退回第一个能服务
/// 列表中任一模块的候选；两者都空才返回 `None`（调用方据此保持现状）。
/// 每个候选最多读一次文件。
#[cfg(target_os = "linux")]
fn find_host_cache(candidates: &[String], modules: &[&str]) -> Option<String> {
    find_host_cache_with(candidates, modules, read_cache_content)
}

/// 两级选择策略本身（纯逻辑），`reader` 由调用方注入：生产环境传读文件的
/// `read_cache_content`，测试传内存表。没有这个缝，`find_host_cache` 的策略
/// 就只能靠镜像一份逻辑来测——镜像和实现各自漂移，测试就成了假绿。
fn find_host_cache_with(
    candidates: &[String],
    modules: &[&str],
    reader: impl Fn(&str) -> Option<String>,
) -> Option<String> {
    // 合并成单次遍历：先扫「列出第一个请求模块」的候选（命中即返回），同时记下
    // 第一个「列出任意请求模块」的候选作兜底，避免把每个候选读两遍。
    let mut fallback = None;
    for candidate in candidates {
        let content = match reader(candidate) {
            Some(content) => content,
            None => continue,
        };
        if modules.first().is_some_and(|m| cache_lists_module(&content, m)) {
            return Some(candidate.clone());
        }
        if fallback.is_none() && modules.iter().any(|m| cache_lists_module(&content, m)) {
            fallback = Some(candidate.clone());
        }
    }
    fallback
}

/// 读入缓存内容；读失败（不存在 / 不可读）统一按"没有这份缓存"处理。
///
/// 用 `from_utf8_lossy` 容错：缓存本质是文本，个别非 UTF-8 字节不应让我们
/// 误判成"文件不存在"，也绝不应让判定朝相反方向倾斜（ASCII 字节原样保留）。
#[cfg(target_os = "linux")]
fn read_cache_content(path: &str) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 真实 bundle 缓存：issue #7746 报告的模块集合，按 `gtk-query-immodules`
    /// 的真实输出形状写——`#` 头注释 + 每个模块一个块（`.so` 路径独占一行、随后
    /// 一行恰好五个带引号字段、空行结束），块与块之间也由空行分隔。这是 GTK
    /// 3 的 `gtk_im_module_initialize()` 真正能解析的形状；故意不含任何
    /// fcitx / fcitx5 / ibus / scim 条目。
    const BUNDLE: &str = concat!(
        "# Created by gtk-query-immodules-3.0\n",
        "\"/usr/lib/gtk-3.0/immodules/im-am-et.so\"\n",
        "\"am-et\" \"Amharic\" \"gtk30\" \"/usr/share/locale\" \"am\"\n",
        "\n",
        "\"/usr/lib/gtk-3.0/immodules/im-am-et.so\"\n",
        "\"am-et\" \"Amharic (am-et)\" \"gtk30\" \"/usr/share/locale\" \"\"\n",
        "\n",
        "\"/usr/lib/gtk-3.0/immodules/im-broadway.so\"\n",
        "\"broadway\" \"Broadway\" \"gtk30\" \"/usr/share/locale\" \"\"\n",
        "\n",
        "\"/usr/lib/gtk-3.0/immodules/im-cedilla.so\"\n",
        "\"cedilla\" \"Cedilla\" \"gtk30\" \"/usr/share/locale\" \"\"\n",
        "\n",
        "\"/usr/lib/gtk-3.0/immodules/im-cyrillic-translit.so\"\n",
        "\"cyrillic-translit\" \"Cyrillic (transliterated)\" \"gtk30\" \"/usr/share/locale\" \"\"\n",
        "\n",
        "\"/usr/lib/gtk-3.0/immodules/im-ipa.so\"\n",
        "\"ipa\" \"IPA\" \"gtk30\" \"/usr/share/locale\" \"ipa\"\n",
        "\n",
        "\"/usr/lib/gtk-3.0/immodules/im-multipress.so\"\n",
        "\"multipress\" \"Multipress\" \"gtk30\" \"/usr/share/locale\" \"\"\n",
        "\n",
        "\"/usr/lib/gtk-3.0/immodules/im-thai.so\"\n",
        "\"thai\" \"Thai-Lao\" \"gtk30\" \"/usr/share/locale\" \"en_US:th\"\n",
        "\n",
        "\"/usr/lib/gtk-3.0/immodules/im-ti-er.so\"\n",
        "\"ti-er\" \"Tigrigna (EZ+)\" \"gtk30\" \"/usr/share/locale\" \"ti\"\n",
        "\n",
        "\"/usr/lib/gtk-3.0/immodules/im-ti-et.so\"\n",
        "\"ti-et\" \"Tigrinya (EZ+)\" \"gtk30\" \"/usr/share/locale\" \"ti\"\n",
        "\n",
        "\"/usr/lib/gtk-3.0/immodules/im-viqr.so\"\n",
        "\"viqr\" \"Vietnamese (VIQR)\" \"gtk30\" \"/usr/share/locale\" \"vi\"\n",
        "\n",
        "\"/usr/lib/gtk-3.0/immodules/im-wayland.so\"\n",
        "\"wayland\" \"Wayland\" \"gtk30\" \"/usr/share/locale\" \"@wayland\"\n",
        "\n",
        "\"/usr/lib/gtk-3.0/immodules/im-waylandgtk.so\"\n",
        "\"waylandgtk\" \"Waylandgtk\" \"gtk30\" \"/usr/share/locale\" \"\"\n",
        "\n",
        "\"/usr/lib/gtk-3.0/immodules/im-xim.so\"\n",
        "\"xim\" \"X Input Method\" \"gtk30\" \"/usr/share/locale\" \"@xim\"\n",
        "\n",
    );

    /// 主机缓存（fcitx4 形态，Debian multiarch 路径）：只列出 fcitx。
    /// locale 字段留空（真实缓存里的常见形态），这样 `"fcitx"` 只可能来自 id
    /// 字段——测试钉住的是"id 被列出"，而不是"某个字段恰好等于模块名"。
    const HOST_FCITX: (&str, &str) = (
        "/usr/lib/x86_64-linux-gnu/gtk-3.0/3.0.0/immodules.cache",
        concat!(
            "# Created by gtk-query-immodules-3.0\n",
            "\"/usr/lib/x86_64-linux-gnu/gtk-3.0/3.0.0/immodules/im-fcitx.so\"\n",
            "\"fcitx\" \"Fcitx\" \"gtk30\" \"/usr/share/locale\" \"\"\n",
            "\n",
        ),
    );

    /// 主机缓存（fcitx5 形态，扁平路径）：列出 fcitx5 与 ibus。
    const HOST_FCITX5: (&str, &str) = (
        "/usr/lib/gtk-3.0/3.0.0/immodules.cache",
        concat!(
            "# Created by gtk-query-immodules-3.0\n",
            "\"/usr/lib/gtk-3.0/immodules/im-fcitx5.so\"\n",
            "\"fcitx5\" \"Fcitx5\" \"gtk30\" \"/usr/share/locale\" \"@fcitx5\"\n",
            "\n",
            "\"/usr/lib/gtk-3.0/immodules/im-ibus.so\"\n",
            "\"ibus\" \"IBus\" \"gtk30\" \"/usr/share/locale\" \"@ibus\"\n",
            "\n",
            "\"/usr/lib/gtk-3.0/immodules/im-xim.so\"\n",
            "\"xim\" \"X Input Method\" \"gtk30\" \"/usr/share/locale\" \"@xim\"\n",
            "\n",
        ),
    );

    /// 主机缓存（lib64 路径）：可读，但只列出自带模块——服务不了任何请求。
    const HOST_STOCK_ONLY: (&str, &str) = (
        "/usr/lib64/gtk-3.0/3.0.0/immodules.cache",
        concat!(
            "# Created by gtk-query-immodules-3.0\n",
            "\"/usr/lib64/gtk-3.0/immodules/im-xim.so\"\n",
            "\"xim\" \"X Input Method\" \"gtk30\" \"/usr/share/locale\" \"@xim\"\n",
            "\n",
        ),
    );

    /// 反例夹具：模块名只出现在**非 id 字段**里，且形态真实（显示名含括号形式）。
    /// 用来固定"显示名不算模块名"这一不变量——它是字段判定的现实近似边界。
    const HOST_NAME_ONLY: (&str, &str) = (
        "/usr/lib/gtk-3.0/3.0.0/immodules.cache",
        concat!(
            "# Created by gtk-query-immodules-3.0\n",
            "\"/usr/lib/gtk-3.0/immodules/im-mystery.so\"\n",
            "\"mystery\" \"Fcitx (fcitx)\" \"gtk30\" \"/usr/share/locale\" \"\"\n",
            "\n",
        ),
    );

    /// AppRun 导出并经钩子拼出来的 bundle 缓存路径（注意双斜杠形态）。
    const APP: &str = "/tmp/.mount_cc";
    const HOOK: &str = "/tmp/.mount_cc//usr/lib/gtk-3.0/3.0.0/immodules.cache";
    const HOOK_MULTIARCH: &str =
        "/tmp/.mount_cc//usr/lib/x86_64-linux-gnu/gtk-3.0/3.0.0/immodules.cache";
    /// 不在静态候选里的 /usr 布局，用来验证镜像候选真的排在静态列表前面。
    const HOOK_LOCAL: &str =
        "/tmp/.mount_cc//usr/local/lib/x86_64-linux-gnu/gtk-3.0/3.0.0/immodules.cache";
    const SYSTEM_PATH: &str = "/usr/lib/gtk-3.0/3.0.0/immodules.cache";

    /// 构造主机探测的候选表与 reader：路径即候选顺序，内容即"文件内容"，
    /// 一起交给真正的 `find_host_cache_with`。测试侧**不再**复制一份选择策略——
    /// 早期这里镜像过一份只实现「任一」的逻辑，于是 `find_host_cache` 改成两级
    /// 之后测试断言的是新策略、跑的却是旧镜像，那条假红只有再读一遍才发现。
    fn host_probe(table: &[(&str, &str)]) -> (Vec<String>, impl Fn(&str) -> Option<String> + '_) {
        let paths: Vec<String> = table.iter().map(|(path, _)| (*path).to_string()).collect();
        let reader = move |path: &str| {
            table
                .iter()
                .find(|(candidate, _)| *candidate == path)
                .map(|(_, content)| (*content).to_string())
        };
        (paths, reader)
    }

    /// 所有端到端测试的统一入口：bundle 探测与主机探测都由内容表注入，
    /// 不接触进程环境、不读文件，且主机那一路走真正的 `find_host_cache_with`。
    fn run(
        override_value: Option<&str>,
        gtk_im_module: Option<&str>,
        current: Option<&str>,
        appdir: Option<&str>,
        bundle: Option<&str>,
        host: &[(&str, &str)],
    ) -> ImModuleFileAction {
        let (paths, reader) = host_probe(host);
        resolve_im_module_file_action(
            override_value,
            gtk_im_module,
            current,
            appdir,
            |module| bundle.map(|content| cache_lists_module(content, module)),
            |modules| find_host_cache_with(&paths, modules, &reader),
        )
    }

    /// 默认场景入口：钩子写入的 bundle 缓存路径 + APPDIR 可用。
    fn hooked(
        override_value: Option<&str>,
        gtk_im_module: Option<&str>,
        bundle: Option<&str>,
        host: &[(&str, &str)],
    ) -> ImModuleFileAction {
        run(override_value, gtk_im_module, Some(HOOK), Some(APP), bundle, host)
    }

    /// 完整入口，供例外场景（非 bundle 路径 / 没有 APPDIR）使用。
    fn resolve(
        override_value: Option<&str>,
        gtk_im_module: Option<&str>,
        current: Option<&str>,
        appdir: Option<&str>,
        bundle: Option<&str>,
        host: &[(&str, &str)],
    ) -> ImModuleFileAction {
        run(override_value, gtk_im_module, current, appdir, bundle, host)
    }

    #[test]
    fn host_cache_that_serves_the_module_is_selected() {
        // issue #7746 主形态：bundle 缓存没有 fcitx，主机缓存有 → 指过去。
        let action = hooked(None, Some("fcitx"), Some(BUNDLE), &[HOST_FCITX]);
        let expected = ImModuleFileAction::Set(HOST_FCITX.0.to_string());
        assert_eq!(action, expected);
    }

    /// 直接测真正的 `find_host_cache_with`（经由 reader 注入内存表）。这些用例是
    /// 两级策略唯一不会被镜像逻辑蒙混的守门人：把 prefer-first / fallback 两档
    /// 换序、或者把 fallback 档删掉，下面第一条就会红。
    #[test]
    fn host_probe_tiers_prefer_first_module_then_any() {
        let (paths, reader) = host_probe(&[HOST_FCITX5, HOST_FCITX]);
        // 请求 [fcitx, ibus]：fcitx5/ibus 缓存排在前面且能服务靠后的 ibus，
        // fcitx 缓存在后面但服务的是用户首选项——必须取后者。
        let hit = find_host_cache_with(&paths, &["fcitx", "ibus"], &reader);
        assert_eq!(hit.as_deref(), Some(HOST_FCITX.0));
        // 只要第一个模块能服务，靠后模块再靠前也要让位。
        let (paths, reader) = host_probe(&[HOST_FCITX5, HOST_FCITX]);
        let hit = find_host_cache_with(&paths, &["fcitx"], &reader);
        assert_eq!(hit.as_deref(), Some(HOST_FCITX.0));
        // 第一个模块谁都服务不了时，退回第一个能服务靠后模块的候选。
        let (paths, reader) = host_probe(&[HOST_FCITX5]);
        let hit = find_host_cache_with(&paths, &["fcitx", "ibus"], &reader);
        assert_eq!(hit.as_deref(), Some(HOST_FCITX5.0));
        // 谁都服务不了：None（调用方据此保持现状）。
        let (paths, reader) = host_probe(&[HOST_STOCK_ONLY]);
        let hit = find_host_cache_with(&paths, &["fcitx", "ibus"], &reader);
        assert_eq!(hit, None);
    }

    #[test]
    fn host_probe_skips_unreadable_candidates() {
        // 读不到的候选（reader 返回 None）直接跳过：不能因为第一个候选存在就
        // 默认用它的路径，也不能因为读不到就放弃整个探测。
        let paths = vec![HOST_STOCK_ONLY.0.to_string(), HOST_FCITX.0.to_string()];
        let reader = |path: &str| match path {
            first if first == HOST_STOCK_ONLY.0 => None,
            _ => Some(HOST_FCITX.1.to_string()),
        };
        let hit = find_host_cache_with(&paths, &["fcitx"], &reader);
        assert_eq!(hit.as_deref(), Some(HOST_FCITX.0));
    }

    #[test]
    fn readable_host_cache_without_the_module_is_skipped() {
        // 第一个候选可读但不列 fcitx：不能因为"找到文件"就把变量指过去。
        let action = hooked(None, Some("fcitx"), Some(BUNDLE), &[HOST_STOCK_ONLY, HOST_FCITX]);
        let expected = ImModuleFileAction::Set(HOST_FCITX.0.to_string());
        assert_eq!(action, expected);
    }

    #[test]
    fn no_host_cache_serves_the_module_so_nothing_changes() {
        // 一台能用的都没有：保持现状，并把原因带出来（绝不猜一个路径）。
        let action = hooked(None, Some("fcitx"), Some(BUNDLE), &[HOST_STOCK_ONLY]);
        let expected = ImModuleFileAction::KeepWithoutHostCache("fcitx".into());
        assert_eq!(action, expected);
        // 空表同理。
        let action = hooked(None, Some("ibus"), Some(BUNDLE), &[]);
        let expected = ImModuleFileAction::KeepWithoutHostCache("ibus".into());
        assert_eq!(action, expected);
    }

    #[test]
    fn quoted_names_do_not_satisfy_a_different_module() {
        // 主机缓存只有 fcitx5：用户要的 fcitx 不能被 `"fcitx5"` 满足，
        // 否则会指过去一个服务不了他的缓存。
        let action = hooked(None, Some("fcitx"), Some(BUNDLE), &[HOST_FCITX5]);
        let expected = ImModuleFileAction::KeepWithoutHostCache("fcitx".into());
        assert_eq!(action, expected);
        // 要 fcitx5 就认得出，直接指向它。
        let action = hooked(None, Some("fcitx5"), Some(BUNDLE), &[HOST_FCITX5]);
        let expected = ImModuleFileAction::Set(HOST_FCITX5.0.to_string());
        assert_eq!(action, expected);
    }

    #[test]
    fn any_module_resolved_by_the_bundle_stops_the_fix() {
        // bundle 缓存自己就有 xim / wayland / thai / am-et：保持钩子写入的路径。
        for module in ["xim", "wayland", "thai", "am-et"] {
            let action = hooked(None, Some(module), Some(BUNDLE), &[HOST_FCITX]);
            assert_eq!(action, ImModuleFileAction::Keep);
        }
    }

    #[test]
    fn colon_list_is_served_when_any_entry_resolves() {
        // GTK_IM_MODULE=fcitx:xim——fcitx 服务不了，但 xim 在 bundle 缓存里，
        // GTK 会自己落到 xim（用户列表里本来就允许），我们不动。
        let action = hooked(None, Some("fcitx:xim"), Some(BUNDLE), &[HOST_FCITX]);
        assert_eq!(action, ImModuleFileAction::Keep);
    }

    #[test]
    fn colon_list_uses_a_host_cache_for_any_entry() {
        // "fcitx:ibus" 都不在 bundle 里；主机缓存列出了 ibus → 指向它。
        let action = hooked(None, Some("fcitx:ibus"), Some(BUNDLE), &[HOST_FCITX5]);
        let expected = ImModuleFileAction::Set(HOST_FCITX5.0.to_string());
        assert_eq!(action, expected);
    }

    #[test]
    fn host_cache_prefers_the_first_requested_module() {
        // GTK_IM_MODULE=fcitx:ibus：两个主机缓存都可用，一个列出靠后的 ibus、
        // 一个列出用户首选的 fcitx——必须选后者，否则等于替用户改偏好。
        let action = hooked(None, Some("fcitx:ibus"), Some(BUNDLE), &[HOST_FCITX5, HOST_FCITX]);
        let expected = ImModuleFileAction::Set(HOST_FCITX.0.to_string());
        assert_eq!(action, expected);
    }

    #[test]
    fn colon_list_falls_back_to_a_later_module_when_the_first_is_missing() {
        // 只有列出靠后模块的缓存时，仍然要用它（比什么都不做强）。
        let action = hooked(None, Some("fcitx:ibus"), Some(BUNDLE), &[HOST_FCITX5]);
        let expected = ImModuleFileAction::Set(HOST_FCITX5.0.to_string());
        assert_eq!(action, expected);
    }

    #[test]
    fn bundle_cache_unreadable_keeps_everything() {
        // false-negative 防护：缓存读不到时没有正面证据，不动用户环境，
        // 也不去做主机探测（没有证据表明 bundle 缺这个模块）。
        let action = hooked(None, Some("fcitx"), None, &[HOST_FCITX]);
        assert_eq!(action, ImModuleFileAction::Keep);
    }

    #[test]
    fn no_im_module_configured_is_a_no_op() {
        // 没有任何输入法配置时 GTK 走自动探测，不干预。
        let action = hooked(None, None, Some(BUNDLE), &[HOST_FCITX]);
        assert_eq!(action, ImModuleFileAction::Keep);
        // 空串 / 纯空白同样视为未配置。
        let action = hooked(None, Some("  "), Some(BUNDLE), &[HOST_FCITX]);
        assert_eq!(action, ImModuleFileAction::Keep);
    }

    #[test]
    fn builtin_and_none_ids_are_no_ops() {
        // GTK 真正认的内置 id：写它们时不动环境变量。
        for module in ["gtk-im-context-simple", "gtk-im-context-none", "xim"] {
            let action = hooked(None, Some(module), Some(BUNDLE), &[HOST_FCITX]);
            assert_eq!(action, ImModuleFileAction::Keep);
        }
    }

    #[test]
    fn short_builtin_aliases_are_not_noop_ids() {
        // `simple` / `none` 是 GTK 不认的短名：不能算作"已满足"。这样的请求
        // 任何缓存都列不出来，结局仍是保守保持（但理由是主机缓存里没有）。
        let action = hooked(None, Some("simple"), None, Some(BUNDLE), &[HOST_FCITX]);
        let expected = ImModuleFileAction::KeepWithoutHostCache("simple".into());
        assert_eq!(action, expected);
    }

    #[test]
    fn override_escape_hatch_wins_in_every_shape() {
        // keep：即使命中修缺陷的条件，也保持默认行为不变（零回归逃生开关）。
        let host = &[HOST_FCITX];
        for keep in ["keep", "KEEP", "  keep  "] {
            let action = hooked(Some(keep), Some("fcitx"), Some(BUNDLE), host);
            assert_eq!(action, ImModuleFileAction::Keep);
        }
        // 自定义路径：强制覆写，两端空白被去掉。
        let action = hooked(Some("  /tmp/my.cache  "), Some("fcitx"), Some(BUNDLE), host);
        let expected = ImModuleFileAction::Set("/tmp/my.cache".to_string());
        assert_eq!(action, expected);
        // 纯空白按未设置处理，继续走自动判定。
        let action = hooked(Some("   "), Some("fcitx"), Some(BUNDLE), host);
        let expected = ImModuleFileAction::Set(HOST_FCITX.0.to_string());
        assert_eq!(action, expected);
    }

    #[test]
    fn non_appimage_path_is_untouched() {
        // 系统缓存路径（deb / rpm / 自行解包启动）：即使缺模块也不干预，
        // 主机缓存根本不参与（连候选都不组装）。
        let action = resolve(
            None,
            Some("fcitx"),
            Some(SYSTEM_PATH),
            Some(APP),
            Some(BUNDLE),
            &[HOST_FCITX],
        );
        assert_eq!(action, ImModuleFileAction::Keep);
    }

    #[test]
    fn without_appdir_we_stay_out_of_the_way() {
        // 拿不到 APPDIR 时无法把路径归因给 bundle 缓存，保守不干预。
        let action = resolve(
            None,
            Some("fcitx"),
            Some(HOOK),
            None,
            Some(BUNDLE),
            &[HOST_FCITX],
        );
        assert_eq!(action, ImModuleFileAction::Keep);
    }

    #[test]
    fn unset_current_variable_is_a_no_op() {
        let action = resolve(
            None,
            Some("fcitx"),
            None,
            Some(APP),
            Some(BUNDLE),
            &[HOST_FCITX],
        );
        assert_eq!(action, ImModuleFileAction::Keep);
    }

    #[test]
    fn requested_modules_split_the_colon_list() {
        assert_eq!(
            requested_im_modules(Some("fcitx:ibus")),
            Some(vec!["fcitx", "ibus"])
        );
        // 空项（连续冒号 / 首尾冒号）会被丢掉；但不 trim，也不展开 XMODIFIERS。
        let modules = requested_im_modules(Some(" fcitx :: : ibus "));
        assert_eq!(modules, Some(vec![" fcitx ", " ibus "]));
        // 整体为空串 / 纯空白视为未配置。
        assert_eq!(requested_im_modules(Some("   ")), None);
        assert_eq!(requested_im_modules(None), None);
    }

    #[test]
    fn empty_entries_cannot_masquerade_as_served() {
        // `fcitx::ibus`：空项被丢掉。若没丢掉，"fcitx" 与 "" 都会被送去判定，
        // 而 bundle 缓存 locale 字段里的空串 `""` 会把空项判成"已列出"——整个
        // 请求就会被误判为已满足，该修的也没修。这里必须落到改指向主机缓存。
        let action = hooked(
            None,
            Some("fcitx::ibus"),
            None,
            Some(BUNDLE),
            &[HOST_FCITX5],
        );
        let expected = ImModuleFileAction::Set(HOST_FCITX5.0.to_string());
        assert_eq!(action, expected);
        // 直接固定底层判定：空串在带空 locale 字段的缓存里确实"看起来被列出"，
        // 所以丢空项这一步必须在 requested_im_modules 完成，而不是靠缓存判定。
        assert!(cache_lists_module(BUNDLE, ""));
    }

    #[test]
    fn mirrored_host_cache_maps_the_bundle_layout_onto_the_host() {
        assert_eq!(
            mirrored_host_cache(HOOK, APP).as_deref(),
            Some("/usr/lib/gtk-3.0/3.0.0/immodules.cache")
        );
        assert_eq!(
            mirrored_host_cache(HOOK, "/tmp/.mount_cc/").as_deref(),
            Some("/usr/lib/gtk-3.0/3.0.0/immodules.cache")
        );
        assert_eq!(
            mirrored_host_cache(HOOK_MULTIARCH, APP).as_deref(),
            Some("/usr/lib/x86_64-linux-gnu/gtk-3.0/3.0.0/immodules.cache")
        );
    }

    #[test]
    fn mirrored_host_cache_rejects_unsafe_shapes() {
        // 不在 /usr 下（打包时用了 /opt 前缀）→ 交给静态候选列表。
        assert_eq!(
            mirrored_host_cache("/tmp/.mount_cc//opt/gtk/immodules.cache", APP),
            None
        );
        // 剥完还在 AppDir 里的退化形态。
        assert_eq!(mirrored_host_cache("/usr//usr/lib/x", "/usr"), None);
        // 路径压根不在 APPDIR 下。
        assert_eq!(mirrored_host_cache(SYSTEM_PATH, APP), None);
        // 空 / 纯空白 AppDir。
        assert_eq!(mirrored_host_cache(HOOK, ""), None);
        assert_eq!(mirrored_host_cache(HOOK, "   "), None);
    }

    #[test]
    fn host_candidates_dedupe_the_mirrored_entry() {
        // 镜像路径与静态列表重复时只出现一次，且仍排在第一个。
        let candidates = host_cache_candidates(HOOK_MULTIARCH, APP);
        assert_eq!(
            candidates.first().map(String::as_str),
            Some("/usr/lib/x86_64-linux-gnu/gtk-3.0/3.0.0/immodules.cache")
        );
        assert_eq!(candidates.len(), HOST_IM_MODULE_CACHES.len());
    }

    #[test]
    fn host_candidates_put_the_mirrored_layout_first() {
        // 镜像路径不在静态列表里时，它排在静态候选前面，其余全部保留。
        let candidates = host_cache_candidates(HOOK_LOCAL, APP);
        assert_eq!(
            candidates.first().map(String::as_str),
            Some("/usr/local/lib/x86_64-linux-gnu/gtk-3.0/3.0.0/immodules.cache")
        );
        assert_eq!(candidates.len(), HOST_IM_MODULE_CACHES.len() + 1);
        for candidate in HOST_IM_MODULE_CACHES {
            assert!(candidates.iter().any(|existing| existing == candidate));
        }
    }

    /// 判定必须建立在**真实**的 `gtk-query-immodules` 输出形状上：`#` 头注释，
    /// 每个模块一个块（`.so` 路径独占一行、随后一行恰好五个带引号字段、空行结束）。
    /// 这是 GTK 3 `gtk_im_module_initialize()` 唯一能解析的形状：路径行里路径
    /// 之后不能再有内容（`gtk_skip_space()` 必须失败），字段行必须恰好五个。
    /// 这里把该形状下的不变量全部钉住：两个方向 + 一个负例。
    #[test]
    fn matching_survives_real_query_immodules_output_shapes() {
        // 方向一：真实 bundle 缓存里没有 fcitx / fcitx5 / ibus——即便每个块的
        // 第二行带着显示名、domain、locale 目录与 locale 列表，也不该被误认。
        assert!(!cache_lists_module(BUNDLE, "fcitx"));
        assert!(!cache_lists_module(BUNDLE, "fcitx5"));
        assert!(!cache_lists_module(BUNDLE, "ibus"));
        assert!(!cache_lists_module(BUNDLE, "scim"));
        // bundle 自带的模块仍要认得出，否则会误改环境变量。
        for module in [
            "am-et", "broadway", "cedilla", "cyrillic-translit", "ipa", "multipress", "thai",
            "ti-er", "ti-et", "viqr", "wayland", "waylandgtk", "xim",
        ] {
            assert!(
                cache_lists_module(BUNDLE, module),
                "{module} should be listed"
            );
        }

        // 方向二：主机缓存里 fcitx5 / ibus 的整名引用认得出；带前缀的 `.so`
        // 路径（`im-fcitx5.so`）与整名 `"fcitx5"` 都不该满足用户要的 `fcitx`。
        assert!(cache_lists_module(HOST_FCITX5.1, "fcitx5"));
        assert!(!cache_lists_module(HOST_FCITX5.1, "fcitx"));
        assert!(cache_lists_module(HOST_FCITX5.1, "ibus"));
        assert!(!cache_lists_module(HOST_FCITX.1, "fcitx5"));
        assert!(cache_lists_module(HOST_FCITX.1, "fcitx"));

        // 负例：模块名只出现在**非 id 字段**（显示名的括号形式 `Fcitx (fcitx)`）
        // 时不能算作已列出——否则会把用户指向一个服务不了他的缓存。
        assert!(!cache_lists_module(HOST_NAME_ONLY.1, "fcitx"));
        assert!(cache_lists_module(HOST_NAME_ONLY.1, "mystery"));

        // 端到端走一遍真实形状：fcitx5 + bundle 缓存 + 有 fcitx5 的主机缓存
        // → 指向；fcitx5 + 系统路径（不在 APPDIR 下）→ 保持。
        let action = hooked(
            None,
            Some("fcitx5"),
            None,
            Some(BUNDLE),
            &[HOST_FCITX5],
        );
        let expected = ImModuleFileAction::Set(HOST_FCITX5.0.to_string());
        assert_eq!(action, expected);
        let action = resolve(
            None,
            Some("fcitx5"),
            Some(SYSTEM_PATH),
            Some(APP),
            Some(BUNDLE),
            &[HOST_FCITX5],
        );
        assert_eq!(action, ImModuleFileAction::Keep);

        // 负例走端到端：主机缓存只有显示名里带 "fcitx" → 判定为没有 → 保持现状，
        // 并把原因带出来（不猜一个路径）。
        let action = hooked(None, Some("fcitx"), None, Some(BUNDLE), &[HOST_NAME_ONLY]);
        let expected = ImModuleFileAction::KeepWithoutHostCache("fcitx".into());
        assert_eq!(action, expected);
    }

    #[test]
    fn appdir_prefix_check_tolerates_double_slash() {
        // 钩子写出的是 `$APPDIR//usr/...`，APPDIR 也可能带尾斜杠。
        assert!(is_appdir_relative(HOOK, APP));
        assert!(is_appdir_relative("/tmp/.mount_cc/usr/lib/x", APP));
        assert!(is_appdir_relative(HOOK, "/tmp/.mount_cc/"));
        // 只是拼接前缀不算（.mount_cc-evil）。
        assert!(!is_appdir_relative("/tmp/.mount_cc-evil/usr/x", APP));
        // AppDir 本身不是缓存文件路径。
        assert!(!is_appdir_relative(APP, APP));
        assert!(!is_appdir_relative(HOOK, "   "));
        assert!(!is_appdir_relative(HOOK, ""));
    }
}
