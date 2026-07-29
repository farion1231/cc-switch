# Ephemeral SSH Agent Design

## 目标

用户只需提供可用的 OpenSSH Host。远程服务器不需要预装、配置或编译 CC Switch，不需要 Rust、Node、sudo 或额外监听端口。桌面端在 SSH 会话期间透明启动最小 Agent，断开后不保留持久安装。

## 边界定义

“服务端零操作”指零人工准备和零持久安装。远程能力仍然必须在服务器上执行一个临时进程，否则无法访问远程数据库和配置文件。允许桌面端通过 SSH 协议写入权限为 `0700` 的随机临时文件、启动它，并在会话退出时删除；不允许写入 `~/.cc-switch/agents` 等长期缓存目录。

## 架构

Rust 代码拆成三个明确边界：

1. `cc-switch-core`：数据库、配置模型和适用于无界面环境的业务服务，不依赖 Tauri、GTK、WebKit 或窗口生命周期。
2. `cc-switch-agent`：只包含帧协议、能力白名单、stdio 会话和 core 命令分发。它构建为 Linux musl 静态二进制。
3. `cc-switch` 桌面端：保留 Tauri 命令、UI、SSH 目标和连接状态，内嵌受支持架构的 Agent 字节。

桌面端发布物内嵌 `linux-x86_64` 与 `linux-aarch64` 两个 Agent。它们不是用户需要管理的独立安装包。开发构建可通过显式环境变量覆盖内嵌产物，但正常运行不能依赖外部文件。

## 连接流程

1. 使用 OpenSSH 预检认证，并读取远端 `uname -s`、`uname -m`。
2. 从桌面端内嵌资源选择匹配架构的 Agent，并校验编译时记录的 SHA-256。
3. 通过 `scp`（同样基于 SSH）写入 `/tmp/cc-switch-agent-<随机值>`，不使用固定路径。
4. 通过 SSH 启动一个受控 shell：设置 `trap`，校验文件大小与 SHA-256，执行 `chmod 700`，随后 `exec` Agent 的 `--stdio` 模式。
5. 通过既有帧协议完成版本、schema 与能力协商。
6. 会话退出、握手失败或启动超时后删除临时文件；桌面端也执行一次幂等清理作为兜底。

## 安全约束

- 临时路径由桌面端生成不可预测标识，不能包含用户输入。
- scp/ssh 参数始终使用参数数组，不经过本地 shell。
- 远端 shell 模板只插入经过十六进制校验的随机 ID、长度和 SHA-256。
- Agent stdout 只承载协议帧，诊断只写 stderr。
- Agent 不监听 TCP/Unix socket，不在远端产生安装状态。
- 二进制使用 musl 静态链接，避免依赖服务器上的 GTK、WebKit 或特定 glibc 版本。

## 错误语义

- `AGENT_EMBEDDED_ARTIFACT_MISSING`：桌面构建缺少该架构的内嵌 Agent，属于构建缺陷。
- `AGENT_UPLOAD_FAILED`：临时传输失败。
- `AGENT_INTEGRITY_FAILED`：远端大小或摘要不一致。
- `AGENT_START_FAILED`：权限、`/tmp` 挂载策略或执行策略阻止启动。
- `AGENT_INCOMPATIBLE`：协议或 schema 不兼容。

错误必须区分 SSH 认证、临时投送和协议握手，不能再用 `AGENT_ARTIFACT_MISSING` 暗示用户手工准备文件。

## 测试

1. 独立 Agent 包依赖审计：依赖树中不得出现 `tauri`、`webkit2gtk`、`gtk` 或任何 Tauri plugin。
2. 协议与 Provider 纵向测试继续在临时 HOME 和数据库中运行。
3. 假 SSH/scp 测试验证架构选择、随机临时路径、参数隔离、摘要检查和所有退出路径的清理。
4. Linux CI 使用真实 sshd，验证一台未安装 CC Switch 的干净容器可在首次连接直接完成 Provider 闭环，断开后不存在 Agent 文件。
5. 桌面端本机模式和现有前端测试必须保持通过。

## 非目标

- 不在本阶段实现 Agent 自动升级缓存；每次会话使用临时投送。
- 不要求完全无远端磁盘写入；跨发行版可靠的纯内存执行依赖 `memfd`/解释器，不具备一致可用性。
- 不复制 Provider 业务规则到 Agent；桌面端和 Agent 必须共享同一 headless core。

## 成功标准

- 用户在远端不执行任何安装或构建命令，保存 SSH Host 后即可连接。
- Agent release 二进制不动态依赖 GTK、WebKit、Tauri 或 glibc。
- 断开连接后远端不保留版本目录或 Agent 安装。
- Provider 远程纵向闭环与当前行为一致。
- 所有代码保持未暂存、未提交。
