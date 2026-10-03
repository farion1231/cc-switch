# VPS 客户端密码复用：执行计划

日期：2026-10-02。基线：`62adf70b`，分支 `feat/vps-skill-management`。实施阶段授权为按调查结论制定计划、实现并验证，不包含读取日常凭据或连接真实 VPS。用户随后已授权提交并推送 fork；上游整合和发布状态以原执行记录第 22 节为准。

本计划承接 [最终方案](../proposals/vps-skill-management.md) 和 [原执行记录](vps-skill-management-execution.md) 第 20 节。此前“暂缓客户端密码复用”的决定由本轮授权更新；已有页面、托管 Skill、主机/凭据存储与指纹确认机制保持不变。

## 目标与非目标

- 用户在 VPS 页面保存一次密码，已绑定的本机客户端通过一次性 CC Switch SSH 执行入口复用密码，无需在终端再次输入。
- 保留系统凭据库、不可变凭据 revision 和系统 OpenSSH。密码不进入目录、Skill、argv、环境变量、临时文件或日志。
- 复用现有 ASKPASS 一次性通道；执行结束退出，不新增 MCP/Agent/常驻服务、第二个开关或常驻 Skill 状态行。
- 本轮提供非交互远端命令执行、stdin/stdout/stderr 转发及退出状态。交互终端、专用 SCP/SFTP 接口、sudo 密码、MFA、私钥口令、跨 WSL/容器凭据访问不是本轮目标。

## 入口与信任边界

拟定命令：

```text
<CC Switch executable> vps exec --root <absolute VPS directory> --app <existing Skill app id> --server <UUID> [--timeout <seconds>] -- <one remote command string>
```

- `--` 后恰好一个远端命令字符串，由远端 SSH 服务的 shell 解释；本地使用参数数组，不重新拼接本地 shell。stdin 可用于脚本输入；不分配远端 TTY。
- ASKPASS 分流仍为 `main` 第一项；VPS CLI 分流在其后、GUI/数据库/插件初始化前。未知或错误 VPS 参数直接失败，不打开 GUI。
- 显式 VPS 根目录避免 CLI 初始化前无法读取 GUI 自定义目录缓存的问题；不查找/猜测别的用户数据目录。
- 只接受持久化主机 ID 和现有 Skill 客户端 ID，确认绑定仍存在；不开放覆盖目标、`-F`、`-o`、代理或凭据读取命令。
- 读取并验证主机与信任快照，拒绝未完成/篡改状态；不从 CLI 自动确认指纹或修复写入主数据。凭据读取后重新核验快照，再启动 SSH。
- 每次执行从确认 pin 和目标快照生成私有临时 SSH 配置，保留严格 host-key 校验；不信任可能被换写的公共 `ssh_config`。密码只由一次性 ASKPASS 发送给 OpenSSH。
- GUI 和 CLI 是不同进程，不能把进程内 Mutex 当跨进程锁。采用只读快照、启动前重检和固定目标/pin/revision；撤销后新执行失败，已运行任务不自动终止。同 OS 用户不构成安全沙箱。

## 分阶段实施与验收

### A. CLI 解析与执行准备（先测试）

1. 严格解析 `vps exec`、帮助、根目录、客户端、UUID、命令及超时；明确本地错误分类与 SSH/远端退出码。
2. 使用注入的内存凭据库测试：无绑定/未确认指纹/目标变化时不读密码；缺少密码、凭据库失败、读取期间删除/解绑/换目标/revision 时拒绝执行。
3. 复用公钥/配置校验，新增只读信任读取和执行准备，不改固定连接测试的行为。

验收：新的失败用例先实际失败；实现后定向通过。CLI 不写数据库/主机/信任文件，不提供密码导出入口。

### B. 一次性执行与原生进程边界

1. 复用 ASKPASS broker，增加独立执行 runner，继承标准流，不沿用固定 `true`、空 stdin、20 秒/64 KiB 探测限制。
2. 支持执行超时与取消，回收自有 SSH 子进程；远端命令是否已经产生效果不能由本地取消倒推。
3. 进程启动前/凭据迟到后检查取消和截止时间；错误输出不带凭据或原始命令。
4. 原生子进程验证标准流、大输出、非零退出码、取消、超时、密码不出现在 argv/env；验证 ASKPASS 子进程真实分流。
5. Windows 验证 GUI subsystem 标准句柄继承及退出码，不以 debug 控制台版成功代替。平台未验证部分如实记录。

### C. 客户端目录与托管 Skill 接入

1. 目录版本升级并添加本机执行器 program/固定 argv（不含密码）；主机仍按现有绑定聚合。
2. 普通安装记录当前应用二进制路径；Linux AppImage 使用持久的 `APPIMAGE` 入口而非临时挂载路径。打开/保存/重同步时按现有生成回执刷新；移动/卸载导致入口不可用时停止并提示，不猜路径。
3. 通用 Skill 改为读取当前目录、调用指定执行器、传主机 ID 与一个远端命令，不再要求终端密码重输；不退回读取密码或绕过执行入口。
4. 更新四语言密码/接入说明和 CLI 诊断文本；无新 UI 控件。沿用已有生命周期、哈希、所有权、导出与最后解绑清理路径。

验收：旧目录可通过受控重生成升级，重复生成幂等；不同客户端/目录、路径含空格与 Unicode、启动器路径变化和最后解绑有回归。

### D. 集成验证与记录

- Rust：新增定向测试 → 受影响 VPS/Skill/导出/恢复测试 → 格式/Clippy → 完整库与集成测试。
- 前端：VPS 相关测试、四语言键、typecheck、format check、完整低并发测试、renderer 构建。
- 大任务串行；使用现有 `run-isolated-rust.sh` 的隔离 home/TEMP/AppData/客户端目录，保留 Cargo/rustup HOME。
- 本机受控 SSH 服务/协议替身优先使用临时密钥及虚构测试密码；不得连接生产服务器或访问日常凭据。若无法完成真实 OpenSSH 密码链路，明确标为未验证，不以 mock 冒充。
- 独立复核目标绑定、指纹、ASKPASS 秘密流向、取消与入口生命周期；发现问题先补回归再修复。
- 更新本计划实际状态、原执行记录最新节及本机验证日志。未经另行授权不 commit/push/发布，也不启动日常 GUI。

## 实施与验证结果（2026-10-02）

**阶段 A–D 的代码、Windows 本机链路及自动化门禁已完成。** 尚未执行真实客户端模型会话、生产 VPS、macOS/Linux 原生认证或签名安装包验收。没有提交、推送或更改日常客户端配置。

### 已实现

- `services/vps/cli.rs`：严格 CLI 解析、四语言诊断、执行期限、取消，以及 Windows CLI-only 控制台附加/标准句柄保留。`main.rs` 在 ASKPASS 后、GUI 前分流；早期入口不初始化数据库或 Tauri 插件。
- `services/vps/ssh/exec.rs`：核对持久化主机与当前绑定、只读信任快照、不可变凭据 revision；凭据读取后再次核对文件快照和目录身份。用本次私有配置和 pin 执行系统 SSH，转发标准流、保留退出码并回收自有子进程。
- `TrustStore` 拆出共用校验与不修复写入的读取路径，原 GUI 信任恢复契约保持；密码仍保存在原系统凭据库，无迁移、无明文回退。
- 客户端目录升级为版本 2，加入 `execution.program/args`；通用 Skill 调用该入口，不再要求密码重输。受控 v1 升级、旧安装路径刷新、重复生成幂等沿用原生成回执。
- 四语言密码说明和 CLI 指导已更新，无新控件或第二开关。`Cargo.lock` 仅为 Tokio 增加已有 `signal-hook-registry` 依赖边，没有升级包版本。

### 先失败后修复的证据

1. 最初两项入口/目录回归在旧实现上 **0 通过、2 失败**；四语言新回归在旧文案上 **3 通过、16 失败**。
2. 独立复核发现 OpenSSH 会重新解析目标后的选项。实际 GUI-subsystem 程序执行无害的远端字符串 `-V`，错误返回本地 SSH 版本和 0；补入位于 alias 前的 SSH 内层 `--` 后，真实请求到达本机 SSH 夹具，并按远端退出码返回。新增 `-V/-o/-F` 构造回归，独立复核确认修复。
3. Windows OpenSSH 9.5 原生实测：纯空格安装目录通过，包含 Unicode 的安装目录在 ASKPASS 创建进程时报 error 2。核对对应版本源码后，Windows 密码子进程改用 Unicode 工作目录和相对 ASCII 程序名，避免非 ASCII 路径通过窄字符环境变量往返；不复制辅助程序、不修改系统编码。正式包的 `cc-switch.exe` 名称保持不变。包含空格/Unicode 的安装目录和 VPS 数据目录均复验通过。
4. 本机 SSH 测试夹具首次在发送 exit-status 后过早关闭传输，造成 `client_loop: send disconnect: Connection aborted`。只修夹具关闭握手，未改产品的退出码断言或 SSH 设置。中间编译曾有两处测试比较 `OsString/String` 的类型错误，修正类型后重跑；失败日志保留。

### 最终自动化门禁

| 检查 | 本轮实际结果 |
| --- | --- |
| VPS Rust 定向 | **120 通过，0 失败** |
| 新真实二进制 CLI 集成目标 | **3 通过**，验证帮助/非法参数/ASKPASS 优先分流及无绑定/无信任本地拒绝 |
| 完整 Rust 库 | **3222 通过，0 失败，10 项原有忽略** |
| 全部 Rust 集成目标 | **181 通过，0 失败**，16 个目标全部执行 |
| VPS 四套前端定向 | **62 通过** |
| 完整前端 | **161 文件、1855 通过** |
| TypeScript / 前端格式 / Rust 格式 | 通过 |
| CI 标准 Clippy `-D warnings` | 通过 |
| renderer 构建 | 通过，保留既有动态导入/包体警告 |

未新增 ignore/skip、放宽业务断言或修改无关测试。前后端大任务串行；增强 `--all-targets` Clippy 不作为本轮已执行项。

### Windows 真实本机链路

不是只用 mock：使用实际 `cc-switch.exe`，通过 `cargo rustc --bin cc-switch -- -C debug-assertions=no` 使二进制入口采用 Windows GUI subsystem，并检查 PE Subsystem=2；库仍是本地 dev 构建，不冒称签名 release 安装包。SSH 服务是只监听 `127.0.0.1` 的 Paramiko 协议夹具，只接受固定测试操作，不执行输入的任意 shell 文本。

- Windows 自带 **OpenSSH 9.5p1** 与本机客户端终端使用的 **Git OpenSSH 9.9p2** 分别验证。
- 每次运行新建随机主机/revision、独立临时 home 和 **SESSION 级测试凭据**；仅按该随机名称确认不存在、创建、让实际程序读取并在 finally 删除，未枚举或读取日常凭据。随机测试密码包含空格、Unicode 和 Shell 特殊字符，只在内存与测试凭据项中出现。
- 无 GUI 常驻情况下连续新进程复用密码成功；stdin/stdout **2,097,423 字节**二进制往返一致，stderr **131,072 字节**独立一致，保留退出码 0/37/255 和原始命令引用。
- 未确认指纹/未绑定时不连接；服务器真实 host key 与 pin 不同时，不发送密码；解绑后的新执行、删除测试密码后的新执行均明确失败。
- 1 秒执行限时和真实 Ctrl+C 均通过。Ctrl+C 仅发送到夹具拥有的独立隐藏测试控制台，未向用户终端广播。正常退出、取消和超时后不留本次 `.ssh-exec-*` 目录。
- 安装目录及数据目录含空格/Unicode通过；没有创建 GUI 数据库，没有改写主机或信任文件。每次测试自建凭据均确认删除。

实际客户端的 Skill 发现/模型调用、生产服务器、其他 OS、MFA、sudo 密码、私钥口令、WSL/容器和强制终止 wrapper 后的远端任务状态不在本轮验收结论内。Windows helper 假定正式包的 ASCII 可执行文件名；移动目录受支持，不支持将二进制本身改名为非 ASCII 名称后继续自动密码认证。

### 日志与本机工具

日志保存在被忽略的 `.cc-switch/contribution-prep/`：

- 初始失败：`vps-client-exec-red.log`；实现中间结果：`vps-client-exec-first*.log`、`vps-client-exec-focused.log`。
- 最终后端：`vps-client-exec-path-focused.log`、`vps-client-exec-native-integration.log`、`vps-client-exec-final-{rust-fmt,clippy,rust-full}.log`。
- 前端：`vps-client-exec-{typecheck,format,frontend-full,renderer}.log`。
- 原生链路最终证据：`vps-client-native-windows-verified.{log,json}`、`vps-client-native-git-verified.{log,json}`；服务端记录实际 SSH 握手标识以确认不是只改变了 PATH。此前成功及失败保留在 `vps-client-native*.log/json`。夹具为 `vps-client-native-check.py` / `vps-client-ctrl-driver.py`。

Paramiko 4.0.0 仅安装在本机忽略目录作验证工具，不加入产品依赖。早先临时依赖目录消失导致一次夹具导入失败，随后改用该忽略目录。平台探针首次显式请求 `1.95.0` 曾意外触发 rustup 补装同版本别名缓存；实际编译均使用已有 `1.95` 工具链，没有更改系统 PATH、权限、SSH 服务或发布配置。临时程序副本按本次创建记录清理，测试证据保留。

### 补充：原生桌面的本地密码流程验收

用户随后要求开始验收，并明确现有主机是随意填写的测试信息，只需检查密码配置和流程，不连接该主机或调用模型。通过已有 `vps-desktop-acceptance.sh` 启动实际 Tauri 窗口，保持独立 identifier/home/客户端目录；用仅监听 `127.0.0.1` 的 WebView 调试连接驱动真实页面，确认原生 IPC 存在，不使用 renderer mocks。

- 新密码说明、添加表单取消、帮助打开/关闭通过。
- 只新增一条自有临时主机（loopback 地址、未确认指纹），通过真实 UI 保存随机测试密码；确认对应系统凭据存在。
- 编辑时密码框为空、不回填已保存密码；只改临时主机用途并留空密码保存，原 revision 保持且凭据仍可读。
- 更换临时密码后使用新 revision，旧凭据已清理；主机 JSON、客户端目录和 Skill 中都不含测试密码。
- 选择 Claude 后生成目录 v2、正确的本机执行参数和部署 Skill；投影与部署逐字节一致。
- 调用该新主机的本机入口，在没有确认指纹时返回 `trustRequired`/125，停在本地，不运行 SSH、不打开密码提示。
- 最后解绑清理该客户端目录/部署，但保留主机密码；再通过 UI 删除临时主机，测试凭据全部清理。
- 原有两条主机、它们的密码 revision 引用和绑定均未修改。窗口回到原有主机列表，无未保存表单或确认框。

本轮原生 UI 流程全部通过，没有访问测试地址、调用模型或修改日常配置；这不等于验证了虚构主机的可登录性。证据位于 `.cc-switch/contribution-prep/vps-client-ui-password-{check.log,result.json}`，脚本为 `vps-client-ui-password-check.py`，结束界面为 `vps-client-ui-password-complete.png`（连接信息已遮盖）。
