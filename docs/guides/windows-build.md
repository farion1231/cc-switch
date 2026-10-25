# Windows 上可重复的 Tauri 构建

在仓库根目录运行：

~~~powershell
.\scripts\build-windows-tauri.cmd
# Debug:
.\scripts\build-windows-tauri.cmd --debug
~~~

## 可执行程序位置

| 构建类型 | 项目内路径 | 当前本机文件 |
| --- | --- | --- |
| Release | `src-tauri/target/release/cc-switch.exe` | `D:\Projects\WebStorm\cc-switch\src-tauri\target\release\cc-switch.exe` |
| Debug | `src-tauri/target/debug/cc-switch.exe` | `D:\Projects\WebStorm\cc-switch\src-tauri\target\debug\cc-switch.exe` |

截至 2026-10-24，Release 文件时间为 11:45:44（47,801,856 字节），Debug 文件时间为 12:00:26（85,075,968 字节）；两者均包含本轮无 8888 改动。Debug 使用 `.\scripts\build-windows-tauri.cmd --debug` 构建，Tauri 成功退出（0），`dev` profile 用时 5 分 25 秒。当前运行的是 Release 程序；Debug 的构建已验证，未切换运行实例。

脚本调用 tauri build --no-bundle，先构建前端，再编译带有内嵌前端资源的 Rust 程序。不要改用 cargo build：缺少 Tauri 编译时环境变量会使 WebView2 指向开发服务器，启动后白屏。若目标 exe 正在运行且链接时提示文件被占用，先退出该实例再重试；不要为了开始编译就中断正在使用的 15721 网关。

## 为什么要使用这个脚本

2026-10-24 01:32 生成的 src-tauri/target/release/cc-switch.exe 是本机的构建产物。Cargo 缓存中 aws-lc-sys 于 01:25 成功完成的记录表明，当时初始化了 Visual Studio x64 开发者环境，并设置了：

~~~cmd
CFLAGS=/nologo /MD /Z7
AWS_LC_SYS_CFLAGS=/nologo /MD /Z7 /W3 /we4013
~~~

普通 PowerShell 会话没有这些环境变量，原生依赖重新编译时遇到 MSVC 14.44 的 C1056（“无法更新 .o 中的时间日期戳字段”）。恢复旧环境后，C1056 仍可能在另一个 C 文件出现，因此脚本另设 CRATE_CC_NO_DEFAULTS=1，让本地使用的 cc-rs 1.2.56 不再给 MSVC 添加 -Brepro；所需的 /MD、/Z7 由上面的 CFLAGS 显式提供。这次重试已通过此前报错的 aws-lc-sys、ring 等原生依赖，并于 2026-10-24 11:45 使用 `tauri build --no-bundle` 成功生成 Release exe（命令退出码 0）。不能仅凭一次成功构建断言所有机器上的 C1056 都由 -Brepro 引起。

本机全局 pnpm 会根据项目的 packageManager 字段尝试自安装另一个 pnpm 版本，导致 Tauri 的版本检查停住。脚本将 npm_config_manage_package_manager_versions=false 限定在本次进程，已验证 pnpm --version 和 pnpm list ... --json --depth 0 可立即返回，无须等待自动安装。

## 验证

~~~powershell
Get-Item src-tauri\target\release\cc-switch.exe | Select-Object Length, LastWriteTime
Get-Content "$env:USERPROFILE\.cc-switch\logs\cc-switch.log" -Tail 30
~~~

如果 native 构建失败，先记录完整的首个 C/Rust 错误、cl.exe 版本和这四个环境变量的状态；不要把前端构建通过或旧 exe 仍存在误判为新版本已生成。只在 exe 的 LastWriteTime 更新且 Tauri 返回成功后，才切换运行中的程序。

## 本机验证结果（2026-10-24）

`tauri build --no-bundle` 成功退出（0），生成 `src-tauri/target/release/cc-switch.exe`，时间戳 11:45:44，大小 47,801,856 字节。随后启动该 Release exe，日志确认网关监听 `127.0.0.1:15721`。构建脚本的 VS 环境初始化、Tauri 版本检查及默认 `pnpm run build:renderer` 前端阶段也已通过；为避免重复约 20 分钟的 Release LTO，第二次完整链接在首次成功后主动中止。
