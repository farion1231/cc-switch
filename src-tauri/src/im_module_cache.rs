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

/// 钩子（linuxdeploy-plugin-gtk 的 AppRun）无条件导出的变量名：GTK 初始化时读
/// 的就是它，我们只在判定要改时才回头写同一个名字。
const GTK_IM_MODULE_FILE_ENV: &str = "GTK_IM_MODULE_FILE";

/// AppRun 导出的 AppDir 路径：拿不到它就无法把缓存路径归因给 bundle 缓存。
const APPDIR_ENV: &str = "APPDIR";

/// 用户配置的输入法模块（冒号分隔的有序列表）。GTK 挑输入法模块时只读它、
/// 不读 `XMODIFIERS`，所以本补丁也只读它。
const GTK_IM_MODULE_ENV: &str = "GTK_IM_MODULE";

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

/// 「这台机器要不要为输入法动手」的前置判断：纯字符串，不碰 IO。
///
/// 与 [`resolve_im_module_file_action`] 的前四步一致。它现在的唯一生产用处是
/// 让 [`apply_gtk_im_module_file_fix`] 核对一条不变量：**它放行时 bundle 路径
/// 必定已经拿到**（第 2/3/4 条条件正是 `bundle_path` 的定义）——所以读不读
/// 文件只由 `bundle_path` / `appdir` 说话，不需要另一道门。
///
/// 曾经那道门长这样：`let bundle_content = if probe_files { 读 } else { None }`。
/// 它对正确性是冗余的（resolve 的前四步、以及第五步 `served` 的短路都排在主机
/// 那一档之前返回，门不放行时 bundle 内容与主机候选根本进不了决策），却给
/// "把开关反接"这种变异留了一个在发布形态下静默空转的位置——这层代码读的是
/// 进程环境，把开关藏在这里，就等于放在任何测试都碰不到的地方。门删掉了，
/// 改由 `bundle_path` 驱动惰性。
///
/// 五个条件全部满足才返回 `true`，任一条不满足就说明这台机器要么没走 AppImage
/// 启动、要么用户已经表过态、要么压根没配非内置输入法模块：
/// 1. 逃生开关没有非空取值（`keep` / 自定义路径都算用户已经表过态）；
/// 2. `GTK_IM_MODULE_FILE` 有值（钩子没导出就不是 AppImage 启动）；
/// 3. `APPDIR` 有值（拿不到就无法把路径归因给 bundle 缓存）；
/// 4. 当前路径在 AppDir 内（见 [`is_appdir_relative`]）；
/// 5. 请求的模块里至少有一个不是内置 id（见 [`NO_OP_MODULES`]）。
///
/// 参数传原始值即可：两端空白在这里被忽略（与 resolve 的处理一致），传已经
/// trim 过的值也安全——trim 是幂等的。
fn should_probe_files(
    override_value: Option<&str>,
    current: Option<&str>,
    appdir: Option<&str>,
    gtk_im_module: Option<&str>,
) -> bool {
    // 1) 显式逃生开关优先，与 resolve 第 1 步同一风格。
    let override_value = override_value.map(str::trim).unwrap_or_default();
    if !override_value.is_empty() {
        return false;
    }
    // 2) 钩子没设 GTK_IM_MODULE_FILE（非 AppImage 启动）时无事可做。
    let current = match current.map(str::trim).filter(|c| !c.is_empty()) {
        Some(current) => current,
        None => return false,
    };
    // 3) 没有 APPDIR 就无法把路径归因给 bundle 缓存。
    let appdir = match appdir.map(str::trim).filter(|d| !d.is_empty()) {
        Some(appdir) => appdir,
        None => return false,
    };
    // 4) 只处理指向 AppDir 内 bundle 缓存的路径。
    if !is_appdir_relative(current, appdir) {
        return false;
    }
    // 5) 请求的模块全是内置 id（或压根没配）时，主机缓存给不了更多信息。
    match requested_im_modules(gtk_im_module) {
        Some(modules) => modules.iter().any(|m| !NO_OP_MODULES.contains(m)),
        None => false,
    }
}

/// 在 GTK 初始化前修正 `GTK_IM_MODULE_FILE`：钩子写入的 bundle 缓存服务不了
/// 用户请求的输入法模块时，把它改指向主机上真正列出该模块的缓存。
/// 只在 Linux 编译。
///
/// 只是 [`decide_and_apply_gtk_im_module_file`] 的一层壳：真正干活的函数是私有的
/// （理由见它的文档），公开签名保持 `()` 不变，`main.rs` 里的调用点不用动。
#[cfg(target_os = "linux")]
pub fn apply_gtk_im_module_file_fix() {
    decide_and_apply_gtk_im_module_file();
}

/// 读环境、做判定、把决定写回环境，并把**决定本身交出来**。
///
/// 为什么身体要单独拆一层：`apply_gtk_im_module_file_fix` 是 `pub` 且被 lib.rs
/// 重新导出，它的返回类型就得和它一样公开；而 [`ImModuleFileAction`] 是模块内私有
/// 枚举，改成 `pub` 也不够——私有模块会把有效可见性压回 crate 内，还得去 lib.rs
/// 补一条 `pub use`，那超出了本次改动允许碰的文件范围。所以公开签名不动，把身体
/// 拆到这个私有函数上：测试直接调它，照样拿得到决定（`Keep` 与
/// `KeepWithoutHostCache` 因此可区分），照样覆盖 `resolve_im_module_file_action`
/// 那个调用点与 `env::set_var`。
///
/// 只有这一个出口：前面没有任何提前 return，"每条路径都返回决定"因此是天然的。
#[cfg(target_os = "linux")]
fn decide_and_apply_gtk_im_module_file() -> ImModuleFileAction {
    let override_value = env::var(IM_MODULE_FILE_OVERRIDE_ENV).ok();
    let current = env::var(GTK_IM_MODULE_FILE_ENV).ok();
    let appdir = env::var(APPDIR_ENV).ok();
    let gtk_im_module = env::var(GTK_IM_MODULE_ENV).ok();

    let override_value = override_value.as_deref().map(str::trim).unwrap_or_default();
    let current_path = current.as_deref().map(str::trim).filter(|p| !p.is_empty());
    let appdir = appdir.as_deref().map(str::trim).filter(|d| !d.is_empty());
    let bundle_path = match (current_path, appdir) {
        (Some(path), Some(dir)) if is_appdir_relative(path, dir) => Some(path),
        _ => None,
    };
    let probe_files = should_probe_files(
        Some(override_value),
        current_path,
        appdir,
        gtk_im_module.as_deref(),
    );

    // bundle_path 就是 IO 的总开关：非 AppImage 启动（没有 APPDIR / 变量没设 /
    // 变量指向 AppDir 之外）时它是 None，一次 read 都不会有；逃生开关已经表态、
    // 或压根没配输入法时它仍是 Some，那一趟 read 是白读的——用一次白读换掉
    // 「反接门能让整个补丁静默空转」的位置，划算。而 should_probe_files 放行时
    // bundle_path 必定是 Some（它的第 2/3/4 条条件正是 bundle_path 的定义），
    // 所以这里不再另设一道 probe_files 门：那两道门对正确性是冗余的（resolve
    // 的前四步、以及第五步 served 的短路都排在主机那一档之前返回，门不放行时
    // bundle 内容与主机候选根本进不了决策），却恰好是"反接门"能让整个补丁静默
    // 空转的地方——这层代码读的是进程环境，把开关藏在这里，就等于放在任何测试
    // 都碰不到的地方。
    debug_assert!(!probe_files || bundle_path.is_some());

    // bundle 缓存与主机候选都由 bundle_path / appdir 驱动：前者读不到时保持
    // None（resolve 据此判断"没有正面证据"），后者是空表；探到主机缓存时才读。
    let bundle_content = bundle_path.and_then(read_cache_content);
    let host_candidates = match (bundle_path, appdir) {
        (Some(path), Some(dir)) => host_cache_candidates(path, dir),
        _ => Vec::new(),
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
    //
    // 按引用匹配：决定还要交出去，不能在这里被 move 掉。
    match &action {
        ImModuleFileAction::Keep => {}
        ImModuleFileAction::Set(path) => {
            eprintln!("[cc-switch] GTK_IM_MODULE_FILE 已指向 {path}");
            env::set_var(GTK_IM_MODULE_FILE_ENV, path);
        }
        ImModuleFileAction::KeepWithoutHostCache(modules) => {
            eprintln!("[cc-switch] 主机 GTK 缓存没有列出 {modules}，保持默认行为");
        }
    }
    action
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
        if modules
            .first()
            .is_some_and(|m| cache_lists_module(&content, m))
        {
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
    use serial_test::serial;

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

    /// 主机缓存（/usr/local/lib64 路径）：只列出靠后模块 ibus，没有 fcitx。
    /// 与 HOST_FCITX5 配成一对，用来钉住兜底档"取最早的那个候选"——两个候选都
    /// 只能兜底时，不能因为遍历顺序反了、或者 `fallback.is_none()` 被删掉，
    /// 就换成后面那个。
    const HOST_IBUS_ONLY: (&str, &str) = (
        "/usr/local/lib64/gtk-3.0/3.0.0/immodules.cache",
        concat!(
            "# Created by gtk-query-immodules-3.0\n",
            "\"/usr/local/lib64/gtk-3.0/immodules/im-ibus.so\"\n",
            "\"ibus\" \"IBus\" \"gtk30\" \"/usr/share/locale\" \"@ibus\"\n",
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

    /// 静态候选的**字面**清单：故意与 `HOST_IM_MODULE_CACHES` 重复一遍。
    ///
    /// 测试必须自己列出期望路径。若改成遍历 `HOST_IM_MODULE_CACHES`，从生产
    /// 列表里删掉一项（例如唯一的 arm64-Debian 路径）也不会有任何测试变红
    /// ——断言读的正是被删的那个常量。所以这里一个字一个字照抄：改了生产
    /// 列表，就该有测试变红。
    const EXPECTED_HOST_CACHES: [&str; 6] = [
        "/usr/lib/gtk-3.0/3.0.0/immodules.cache",
        "/usr/lib64/gtk-3.0/3.0.0/immodules.cache",
        "/usr/lib/x86_64-linux-gnu/gtk-3.0/3.0.0/immodules.cache",
        "/usr/lib/aarch64-linux-gnu/gtk-3.0/3.0.0/immodules.cache",
        "/usr/local/lib/gtk-3.0/3.0.0/immodules.cache",
        "/usr/local/lib64/gtk-3.0/3.0.0/immodules.cache",
    ];

    // reader 是借住 `table` 活着的，所以这一层生命周期必须显式写出来：返回类型里
    // 用省略的 `'_` 时，`table` 有多个可省略的生命周期，rustc 推不出该绑哪一个
    // （E0106 missing lifetime specifier）。
    /// 构造主机探测的候选表与 reader：路径即候选顺序，内容即"文件内容"，
    /// 一起交给真正的 `find_host_cache_with`。测试侧**不再**复制一份选择策略——
    /// 早期这里镜像过一份只实现「任一」的逻辑，于是 `find_host_cache` 改成两级
    /// 之后测试断言的是新策略、跑的却是旧镜像，那条假红只有再读一遍才发现。
    fn host_probe<'a>(
        table: &'a [(&str, &str)],
    ) -> (Vec<String>, impl Fn(&str) -> Option<String> + 'a) {
        let paths: Vec<String> = table.iter().map(|(path, _)| (*path).to_string()).collect();
        let reader = move |path: &str| {
            table
                .iter()
                .find(|(candidate, _)| *candidate == path)
                .map(|(_, content)| (*content).to_string())
        };
        (paths, reader)
    }

    /// 把一份缓存内容写进临时目录，返回它的路径（`name` 可以带子目录，父目录会
    /// 一并建出来——bundle 缓存在 AppDir 里是有层级的）。
    ///
    /// 给真正的 `find_host_cache` 与 `apply` 用：`find_host_cache` 自己焊死了
    /// `read_cache_content` 这个 reader，没有注入缝，只能拿真实文件喂它——顺带把
    /// 真实的读失败路径（文件不存在 / 是个目录）也一并走到。
    fn write_cache(dir: &tempfile::TempDir, name: &str, content: &str) -> String {
        let path = dir.path().join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create cache dir");
        }
        std::fs::write(&path, content).expect("write fixture");
        path.to_str().expect("utf-8 temp path").to_string()
    }

    /// 所有端到端测试的统一入口：bundle 探测与主机探测都由内容表注入，
    /// 不接触进程环境、不读文件，且两路都走真正的生产函数——`bundle_lists` 与
    /// `find_host_cache_with`——测试侧不再镜像任何一档的折叠逻辑。
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
            |module| bundle_lists(bundle, module),
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
        run(
            override_value,
            gtk_im_module,
            Some(HOOK),
            Some(APP),
            bundle,
            host,
        )
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
    /// 两级策略唯一不会被镜像逻辑蒙混的守门人：把 prefer-first 的优先级降到
    /// fallback 之下、或者把 fallback 那一档整个删掉，下面的用例就会红。
    /// （注意是**优先级**，不是语句顺序：any 那一档只往 fallback 里记一笔，
    /// 所以把两个 if 的字面位置对调是等价变异，测不出来也不需要测。）
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

    /// 兜底档的两个性质要分开钉：**第一档够不着**（才会走到兜底）与**取最早的
    /// 那个兜底候选**（走遍了该怎么挑）。上面几条例子里能兜底的候选只有一个，
    /// 所以这两条性质一直是同一个用例顺带覆盖的——删掉 `fallback.is_none() &&`
    /// （后一个覆盖前一个）或者把候选遍历整个反过来，全都不红。
    #[test]
    fn host_probe_fallback_keeps_the_earliest_candidate() {
        // 两个候选都只列出靠后模块：兜底档必须取排在前面的那个。
        let (paths, reader) = host_probe(&[HOST_FCITX5, HOST_IBUS_ONLY]);
        let hit = find_host_cache_with(&paths, &["fcitx", "ibus"], &reader);
        assert_eq!(hit.as_deref(), Some(HOST_FCITX5.0));
        // 对照：第一个模块有候选能服务时，兜底档排得再靠前也不该赢——"取最早"
        // 只属于兜底档，别让它替第一档做主。
        let (paths, reader) = host_probe(&[HOST_IBUS_ONLY, HOST_FCITX]);
        let hit = find_host_cache_with(&paths, &["fcitx", "ibus"], &reader);
        assert_eq!(hit.as_deref(), Some(HOST_FCITX.0));
    }

    /// 直接测生产形态的 `find_host_cache`（真实文件、真实 `read_cache_content`）。
    /// `find_host_cache_with` 那一组把两级策略本身钉住，这一组钉住"生产代码真的
    /// 被接上了"：reader 注入缝被焊死、候选类型被换掉、或者整个函数被旁路掉，
    /// 上面的用例照旧绿，只有这几条会红。
    #[test]
    fn find_host_cache_prefers_the_first_requested_module() {
        let dir = tempfile::tempdir().expect("tempdir");
        // 请求 [fcitx, ibus]：第一个候选只列出靠后的 ibus，第二个列出用户首选的
        // fcitx——必须取第二个，否则等于替用户改偏好。
        let later = write_cache(&dir, "later.cache", HOST_FCITX5.1);
        let first = write_cache(&dir, "first.cache", HOST_FCITX.1);
        let candidates = [later, first];
        let hit = find_host_cache(&candidates, &["fcitx", "ibus"]);
        assert_eq!(hit.as_deref(), Some(candidates[1].as_str()));
        // 两个候选都列出第一个请求模块时，候选顺序说话（镜像布局优先）。
        let multiarch = write_cache(&dir, "multiarch.cache", HOST_FCITX.1);
        let flat = write_cache(&dir, "flat.cache", HOST_FCITX.1);
        let candidates = [multiarch, flat];
        let hit = find_host_cache(&candidates, &["fcitx"]);
        assert_eq!(hit.as_deref(), Some(candidates[0].as_str()));
    }

    #[test]
    fn find_host_cache_falls_back_to_a_later_requested_module() {
        let dir = tempfile::tempdir().expect("tempdir");
        // 第一个请求模块谁都没有时，退回第一个能服务靠后模块的候选（比什么都不强）。
        let only_later = write_cache(&dir, "only-later.cache", HOST_FCITX5.1);
        let candidates = [only_later];
        let hit = find_host_cache(&candidates, &["fcitx", "ibus"]);
        assert_eq!(hit.as_deref(), Some(candidates[0].as_str()));
    }

    #[test]
    fn find_host_cache_returns_none_when_no_candidate_lists_a_module() {
        let dir = tempfile::tempdir().expect("tempdir");
        // 谁都列不出请求的模块：None（调用方据此保持现状）。
        let stock = write_cache(&dir, "stock.cache", HOST_STOCK_ONLY.1);
        let candidates = [stock];
        assert_eq!(find_host_cache(&candidates, &["fcitx", "ibus"]), None);
        // 一张候选都没有时同样 None，不能慌着猜一个路径。
        assert_eq!(find_host_cache(&[], &["fcitx"]), None);
    }

    #[test]
    fn find_host_cache_skips_a_candidate_it_cannot_read() {
        let dir = tempfile::tempdir().expect("tempdir");
        // 读不到的候选排在前面（文件不存在）：必须跳过它，不能因为它排在第一个
        // 就默认用它的路径，也不能因为第一个读不到就放弃整个探测。
        let good = write_cache(&dir, "good.cache", HOST_FCITX.1);
        let missing = dir.path().join("missing.cache");
        let missing = missing.to_str().expect("utf-8 temp path").to_string();
        let candidates = [missing, good];
        let hit = find_host_cache(&candidates, &["fcitx"]);
        assert_eq!(hit.as_deref(), Some(candidates[1].as_str()));
    }

    #[test]
    fn readable_host_cache_without_the_module_is_skipped() {
        // 第一个候选可读但不列 fcitx：不能因为"找到文件"就把变量指过去。
        let action = hooked(
            None,
            Some("fcitx"),
            Some(BUNDLE),
            &[HOST_STOCK_ONLY, HOST_FCITX],
        );
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
        // 靠后那一项由 bundle 缓存列出时同样停手：thai 不是内置 id，它得真的
        // 从 bundle 缓存里查出来，而不是靠 NO_OP 短路糊过去。
        let action = hooked(None, Some("fcitx:thai"), Some(BUNDLE), &[HOST_FCITX]);
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
        let action = hooked(
            None,
            Some("fcitx:ibus"),
            Some(BUNDLE),
            &[HOST_FCITX5, HOST_FCITX],
        );
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

    /// 直接测 `bundle_lists`：resolve 与"缓存读没读到"之间唯一的三值折叠点。
    /// 端到端那一路（run() 的 bundle 闭包）现在也走这个函数本身，这一组把它
    /// 单独钉一遍——尤其是 None 与 Some(false) 的区别。
    #[test]
    fn bundle_lists_answers_from_the_cache_when_it_is_readable() {
        // 缓存可读：如实回答它列不列这个模块，判定走带引号的整名。
        assert_eq!(bundle_lists(Some(BUNDLE), "xim"), Some(true));
        assert_eq!(bundle_lists(Some(BUNDLE), "fcitx"), Some(false));
        // 空模块名在真实缓存里"看起来被列出了"（locale 字段里的空串 `""`）：
        // 所以丢空项必须由 requested_im_modules 完成，bundle_lists 不负责过滤。
        assert_eq!(bundle_lists(Some(BUNDLE), ""), Some(true));
    }

    #[test]
    fn bundle_lists_reports_none_when_the_cache_is_unreadable() {
        // 读不到必须折成 None（"没有正面证据"），不能折成 Some(false)——
        // 后者会被当成"缓存说没有这个模块"，于是照样去改用户环境。
        assert_eq!(bundle_lists(None, "fcitx"), None);
        assert_eq!(bundle_lists(None, ""), None);
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
        let action = hooked(None, Some("simple"), Some(BUNDLE), &[HOST_FCITX]);
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

    /// 门放行的组合：这些情况下 apply 必须真的去读文件。把 wants_host_cache
    /// 反接成 false（或把整个门反过来）时，唯一的发布形态下整个补丁静默空转，
    /// 而其余测试照旧全绿——这几条就是钉住它的钉子。
    #[test]
    fn a_non_builtin_input_method_needs_files() {
        let hits = should_probe_files(None, Some(HOOK), Some(APP), Some("fcitx"));
        assert!(hits);
        // 钩子真实的双斜杠形态与 multiarch 布局同样放行。
        let hits = should_probe_files(None, Some(HOOK_MULTIARCH), Some(APP), Some("fcitx"));
        assert!(hits);
        // 冒号列表里只要有一项不是内置 id 就要读文件：GTK 按顺序取第一个能解析
        // 的项，fcitx 排在前、排在后都一样。
        let hits = should_probe_files(None, Some(HOOK), Some(APP), Some("fcitx:xim"));
        assert!(hits);
        let hits = should_probe_files(None, Some(HOOK), Some(APP), Some("xim:fcitx"));
        assert!(hits);
        // " fcitx " 带空格：模块名不 trim（GTK 用 g_strcmp0 精确匹配），它仍然
        // 不是内置 id，所以门放行；读不读得到由后面的缓存判定决定。
        let hits = should_probe_files(None, Some(HOOK), Some(APP), Some(" fcitx "));
        assert!(hits);
    }

    /// 显式逃生开关：keep / KEEP / 自定义路径 / 两端空白都不该触发读文件；
    /// 纯空白按未设置处理，门照旧放行（否则用户只写了空格，补丁就空转）。
    #[test]
    fn an_explicit_escape_hatch_needs_no_files() {
        for hatch in ["keep", "KEEP", "  keep  ", "/tmp/my.cache"] {
            let hits = should_probe_files(Some(hatch), Some(HOOK), Some(APP), Some("fcitx"));
            assert!(!hits);
        }
        let hits = should_probe_files(Some("   "), Some(HOOK), Some(APP), Some("fcitx"));
        assert!(hits);
    }

    /// 钩子没设 GTK_IM_MODULE_FILE（未设 / 空串 / 纯空白）：非 AppImage 启动，
    /// 一次 read 都不该做。
    #[test]
    fn no_files_without_a_hooked_cache_path() {
        for current in [None, Some(""), Some("   ")] {
            let hits = should_probe_files(None, current, Some(APP), Some("fcitx"));
            assert!(!hits);
        }
    }

    /// 路径不在 AppDir 内（deb / rpm / 自行解包启动）：连候选都不该组装。
    #[test]
    fn no_files_when_the_path_is_outside_appdir() {
        let hits = should_probe_files(None, Some(SYSTEM_PATH), Some(APP), Some("fcitx"));
        assert!(!hits);
        // 只是拼接出来的前缀不算（.mount_cc-evil）。
        let hits = should_probe_files(
            None,
            Some("/tmp/.mount_cc-evil/usr/lib/x"),
            Some(APP),
            Some("fcitx"),
        );
        assert!(!hits);
    }

    /// 拿不到 APPDIR（未设 / 空串 / 纯空白）：无法把路径归因给 bundle 缓存。
    #[test]
    fn no_files_without_an_appdir() {
        for appdir in [None, Some(""), Some("   ")] {
            let hits = should_probe_files(None, Some(HOOK), appdir, Some("fcitx"));
            assert!(!hits);
        }
    }

    /// 压根没配非内置输入法模块：未设 / 空串 / 纯空白 / 只有冒号，以及全是
    /// GTK 真正认的内置 id（gtk-im-context-simple / gtk-im-context-none / xim）。
    #[test]
    fn no_files_without_a_real_input_method() {
        for module in [None, Some(""), Some("   "), Some(":"), Some("::")] {
            let hits = should_probe_files(None, Some(HOOK), Some(APP), module);
            assert!(!hits);
        }
        for module in ["gtk-im-context-simple", "gtk-im-context-none", "xim"] {
            let hits = should_probe_files(None, Some(HOOK), Some(APP), Some(module));
            assert!(!hits);
        }
        // 冒号列表里全是内置 id 同样无事可做。
        let hits = should_probe_files(
            None,
            Some(HOOK),
            Some(APP),
            Some("gtk-im-context-simple:xim"),
        );
        assert!(!hits);
    }

    #[test]
    fn requested_modules_split_the_colon_list() {
        assert_eq!(
            requested_im_modules(Some("fcitx:ibus")),
            Some(vec!["fcitx", "ibus"])
        );
        // 空项（连续冒号 / 首尾冒号）会被丢掉；但不 trim，也不展开 XMODIFIERS。
        let modules = requested_im_modules(Some(" fcitx :: : ibus "));
        // 中间那个 `" "` 也活着：过滤器只丢**真空串**，而 `" : "` 切出来是一个
        // 空格段、不是空段。这与 GTK 一致——`lookup_immodule()` 对每个条目都做
        // `g_strcmp0` 与哈希查找，空格段查不中就当没这一项继续，不会因此短路。
        assert_eq!(modules, Some(vec![" fcitx ", " ", " ibus "]));
        // 整体为空串 / 纯空白视为未配置。
        assert_eq!(requested_im_modules(Some("   ")), None);
        assert_eq!(requested_im_modules(None), None);
    }

    #[test]
    fn empty_entries_cannot_masquerade_as_served() {
        // `fcitx::ibus`：空项被丢掉。若没丢掉，"fcitx" 与 "" 都会被送去判定，
        // 而 bundle 缓存 locale 字段里的空串 `""` 会把空项判成"已列出"——整个
        // 请求就会被误判为已满足，该修的也没修。这里必须落到改指向主机缓存。
        let action = hooked(None, Some("fcitx::ibus"), Some(BUNDLE), &[HOST_FCITX5]);
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

    /// 每个静态候选都必须出现，且只出现一次（去重不变量）。
    /// 期望路径写死在 EXPECTED_HOST_CACHES 里，不从生产常量里取。
    fn assert_static_candidates_present(candidates: &[String]) {
        for path in EXPECTED_HOST_CACHES {
            let found = candidates.iter().filter(|c| c.as_str() == path).count();
            assert_eq!(found, 1, "{path} should appear exactly once");
        }
    }

    #[test]
    fn host_candidates_dedupe_the_mirrored_entry() {
        // 镜像路径与静态列表重复时只出现一次，且仍排在第一个。
        let candidates = host_cache_candidates(HOOK_MULTIARCH, APP);
        assert_eq!(
            candidates.first().map(String::as_str),
            Some("/usr/lib/x86_64-linux-gnu/gtk-3.0/3.0.0/immodules.cache")
        );
        // 静态候选一个不少、也一个不多——镜像的那一个就在其中。
        assert_eq!(candidates.len(), EXPECTED_HOST_CACHES.len());
        assert_static_candidates_present(&candidates);
    }

    #[test]
    fn host_candidates_put_the_mirrored_layout_first() {
        // 镜像路径不在静态列表里时，它排在静态候选前面，其余全部保留。
        let candidates = host_cache_candidates(HOOK_LOCAL, APP);
        assert_eq!(
            candidates.first().map(String::as_str),
            Some("/usr/local/lib/x86_64-linux-gnu/gtk-3.0/3.0.0/immodules.cache")
        );
        // 镜像候选 + 全部静态候选：静态的一个不少，也没有哪个出现两次。
        assert_eq!(candidates.len(), EXPECTED_HOST_CACHES.len() + 1);
        assert_static_candidates_present(&candidates);
        // 而且静态那一段必须**原样跟上静态列表的顺序**：顺序决定多个候选都可读
        // 时用户拿到哪个缓存，只断言"都在"的话把整个列表调换顺序也不会有测试红。
        for (index, path) in EXPECTED_HOST_CACHES.iter().enumerate() {
            assert_eq!(candidates[index + 1].as_str(), *path);
        }
    }

    /// 静态候选表本身的顺序也是承重的（见 `HOST_IM_MODULE_CACHES` 的文档）：
    /// 扁平 `/usr/lib`（Arch / Gentoo）优先，`/usr/lib64`（Fedora / openSUSE）
    /// 次之，然后是 Debian 系 multiarch，最后才是本机编译的 `/usr/local`。
    /// 期望值写死在 `EXPECTED_HOST_CACHES` 里，不从生产常量取——反过来读生产
    /// 常量的话，改它就不可能有测试红。
    #[test]
    fn static_candidates_keep_their_documented_order() {
        assert_eq!(HOST_IM_MODULE_CACHES, EXPECTED_HOST_CACHES);
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
            "am-et",
            "broadway",
            "cedilla",
            "cyrillic-translit",
            "ipa",
            "multipress",
            "thai",
            "ti-er",
            "ti-et",
            "viqr",
            "wayland",
            "waylandgtk",
            "xim",
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

        // 负例走端到端：主机缓存只有显示名里带 "fcitx" → 判定为没有 → 保持现状，
        // 并把原因带出来（不猜一个路径）。另外两个端到端形状（fcitx5 命中主机
        // 缓存、系统路径下不干预）已由 quoted_names_do_not_satisfy_a_different_module
        // 与 non_appimage_path_is_untouched 各自覆盖，这里不再重复。
        let action = hooked(None, Some("fcitx"), Some(BUNDLE), &[HOST_NAME_ONLY]);
        let expected = ImModuleFileAction::KeepWithoutHostCache("fcitx".into());
        assert_eq!(action, expected);
    }

    /// 直接测 `read_cache_content`：它是 bundle 与主机两路共用的读文件出口。
    /// 关键是把"读失败"与"读到了空内容"分开——读失败折成 `Some(String::new())`
    /// 会让每份缓存都变成"可读但列不出任何模块"，补丁就永远不生效。
    #[test]
    fn read_cache_content_returns_the_file_text() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = write_cache(&dir, "immodules.cache", HOST_FCITX.1);
        assert_eq!(read_cache_content(&path).as_deref(), Some(HOST_FCITX.1));
    }

    #[test]
    fn read_cache_content_treats_unreadable_paths_as_absent() {
        let dir = tempfile::tempdir().expect("tempdir");
        // 不存在的路径：None（没有这份缓存），不能是读到了空串。
        let missing = dir.path().join("missing.cache");
        let missing = missing.to_str().expect("utf-8 temp path");
        assert_eq!(read_cache_content(missing), None);
        // 目录：std::fs::read 直接失败，同样按"没有这份缓存"处理。
        let dir_path = dir.path();
        let dir_path = dir_path.to_str().expect("utf-8 temp path");
        assert_eq!(read_cache_content(dir_path), None);
    }

    #[test]
    fn read_cache_content_keeps_ascii_from_a_non_utf8_cache() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("weird.cache");
        let weird = b"\xff\"fcitx\"\n";
        std::fs::write(&path, weird).expect("write fixture");
        let path = path.to_str().expect("utf-8 temp path");
        // 个别非 UTF-8 字节不该让整份缓存被判成"不存在"：ASCII 部分原样保留，
        // 模块名照样查得到。
        let read = read_cache_content(path).expect("lossy read");
        assert!(read.contains("\"fcitx\""));
    }

    #[test]
    fn appdir_prefix_check_tolerates_double_slash_and_rejects_the_rest() {
        // 正向：钩子写出的是 `$APPDIR//usr/...`，APPDIR 也可能带尾斜杠；
        // 少一个斜杠的普通拼接同样算。
        assert!(is_appdir_relative(HOOK, APP));
        assert!(is_appdir_relative("/tmp/.mount_cc/usr/lib/x", APP));
        assert!(is_appdir_relative(HOOK, "/tmp/.mount_cc/"));
        // 这一对才是承重的：单斜杠路径 + 带尾斜杠的 APPDIR，两个因素必须同时
        // 由 `trim_end_matches('/')` 兜住。上面三条都被 HOOK 自己的双斜杠满足了
        // ——把那个 trim 删掉它们照样绿，只有这一条会红；没有它，`APPDIR` 带尾
        // 斜杠 + 普通单斜杠路径的组合会静默失配（判定成"不是 bundle 缓存"，
        // 补丁在该启动的机器上不启动）。
        assert!(is_appdir_relative("/tmp/.mount_cc/usr/lib/x", "/tmp/.mount_cc/"));
        // 负向：只是拼接前缀不算（.mount_cc-evil）、AppDir 本身不是缓存路径、
        // AppDir 空 / 纯空白时判定不了，一律当作"不是 bundle 缓存"。
        assert!(!is_appdir_relative("/tmp/.mount_cc-evil/usr/x", APP));
        assert!(!is_appdir_relative(APP, APP));
        assert!(!is_appdir_relative(HOOK, "   "));
        assert!(!is_appdir_relative(HOOK, ""));
    }

    /// 环境变量名必须是钩子与 GTK 真正读的那几个。改错一个字母，逃生开关或者
    /// 整个补丁就静默失效——而其余测试传的都是字面量，抓不到这种错。
    #[test]
    fn env_var_names_match_what_the_hook_and_gtk_read() {
        assert_eq!(IM_MODULE_FILE_OVERRIDE_ENV, "CC_SWITCH_GTK_IM_MODULE_FILE");
        assert_eq!(GTK_IM_MODULE_FILE_ENV, "GTK_IM_MODULE_FILE");
        assert_eq!(APPDIR_ENV, "APPDIR");
        assert_eq!(GTK_IM_MODULE_ENV, "GTK_IM_MODULE");
    }

    /// 还原 `apply` 动过的四个进程环境变量。nextest 一个测试一个进程，本来就不脏；
    /// 但本地 `cargo test` 是同进程多线程，`#[serial]` 只保证不并发、不保证干净，
    /// 所以该还的还得还。
    struct EnvGuard([(&'static str, Option<std::ffi::OsString>); 4]);

    impl EnvGuard {
        fn new() -> Self {
            let names = [
                IM_MODULE_FILE_OVERRIDE_ENV,
                GTK_IM_MODULE_FILE_ENV,
                APPDIR_ENV,
                GTK_IM_MODULE_ENV,
            ];
            Self(names.map(|name| (name, std::env::var_os(name))))
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (name, previous) in &self.0 {
                match previous {
                    Some(value) => std::env::set_var(name, value),
                    None => std::env::remove_var(name),
                }
            }
        }
    }

    /// 端到端走一遍真正干活的 `decide_and_apply_gtk_im_module_file`（`apply` 只是
    /// 它的一层壳）：此前这一层没有任何测试接过，于是 `resolve` 的返回值再对，也
    /// 证明不了它把四个参数按正确顺序交了出去、或者真的把决定写回了进程环境。
    ///
    /// 这里只钉**主机缓存够不着**的形态：候选路径是写死的绝对路径（`/usr/...`），
    /// 测试里造不出来，所以不假设机器上装了什么输入法——那样的用例只会在有 fcitx
    /// 的机器上绿，是假绿。
    ///
    /// 走私有的那层而不是公开壳：它把**决定本身**交出来，而 `Keep` 与
    /// `KeepWithoutHostCache` 的区分只有看得到决定才成立——环境变量在两种情况下
    /// 都是"没变"，区分不了。
    #[test]
    #[serial]
    fn apply_writes_its_decision_back_into_the_environment() {
        let _guard = EnvGuard::new();
        let dir = tempfile::tempdir().expect("tempdir");
        let appdir = dir.path();
        let hooked = write_cache(&dir, "usr/lib/gtk-3.0/3.0.0/immodules.cache", BUNDLE);
        std::env::set_var(APPDIR_ENV, appdir);
        std::env::set_var(GTK_IM_MODULE_FILE_ENV, &hooked);
        std::env::set_var(GTK_IM_MODULE_ENV, "fcitx");

        // 一、逃生开关：强制改写。这是不依赖主机缓存就能拿到 `Set` 的唯一形状，
        // 也是唯一能证明"决定真的被导出到环境里"的形状——把 `env::set_var` 删掉，
        // 诊断照样打、决定照样算，可什么都没导出，补丁白干。
        let forced = dir.path().join("forced.cache");
        let forced = forced.to_str().expect("utf-8 temp path");
        std::env::set_var(IM_MODULE_FILE_OVERRIDE_ENV, forced);
        let action = decide_and_apply_gtk_im_module_file();
        let exported = std::env::var(GTK_IM_MODULE_FILE_ENV).expect("must export");
        assert_eq!(exported, forced);
        assert_eq!(action, ImModuleFileAction::Set(forced.to_string()));

        // 二、没有逃生开关、bundle 缓存里没有这个模块、主机候选一个都够不着：
        // 保持钩子写入的路径，绝不猜一个，也绝不顺手把环境改写掉。模块名故意取一个
        // 任何主机缓存都列不出来的，免得这条用例依赖机器上装了什么。
        let module = "cc-switch-no-such-im-module";
        std::env::remove_var(IM_MODULE_FILE_OVERRIDE_ENV);
        std::env::set_var(GTK_IM_MODULE_FILE_ENV, &hooked);
        std::env::set_var(GTK_IM_MODULE_ENV, module);
        let action = decide_and_apply_gtk_im_module_file();
        let kept = std::env::var(GTK_IM_MODULE_FILE_ENV).expect("var stays set");
        assert_eq!(kept, hooked);
        // 决定必须带着请求的模块名出来。把 `gtk_im_module` 与 `current` 在调用点
        // 上调个交换（四个 `Option<&str>` 同型，编译器看不出来），`resolve` 会在
        // 第三步就返回 `Keep`——环境照样"没变"，只有这一条会红。
        let expected = ImModuleFileAction::KeepWithoutHostCache(module.to_string());
        assert_eq!(action, expected);
    }
}
