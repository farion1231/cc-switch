# 远端命令安全 PATH 设计

## 背景

桌面端已能把内嵌的 Linux Agent 上传到远端，但 `dubhe-dev-xh` 的 SSH 非交互式 `zsh` 没有可用的标准 `PATH`。Agent 启动模板因此找不到 `wc`、`tr`，兜底清理也找不到 `rm`，最终返回 `AGENT_INTEGRITY_FAILED`。

交互式探测确认 `wc`、`tr`、`sha256sum`、`awk`、`chmod` 均位于 `/bin`；`rm` 在交互式 shell 中被定义为 `rm -i` alias。问题属于远端命令环境不完整，不是 Agent 损坏或服务器缺包。

## 目标

- 预检、Agent 完整性校验、启动和兜底清理不依赖 SSH 非交互式 shell 继承用户 PATH。
- 远端命令不受 `rm` 等用户 alias 或同名 shell 函数影响。
- 保留现有随机临时路径、长度与 SHA-256 校验、stdio 协议和错误码。
- 不要求用户修改远端 shell 配置或安装额外软件。

## 方案

所有由桌面端生成的远端命令统一在最前面设置并导出安全 PATH：

```sh
PATH="/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin${PATH:+:$PATH}"
export PATH
```

标准目录放在原 PATH 前面，确保常见 Linux 工具优先从系统目录解析；原 PATH 仍保留在末尾，以兼容 NixOS、自定义发行版或管理员提供的额外目录。

远端工具通过 shell 内建的 `command` 调用，例如 `command wc`、`command sha256sum` 和 `command rm`。这样既使用统一 PATH，又绕过同名 shell 函数；命令模板仍只插入桌面端生成的十六进制临时路径、十进制长度和 SHA-256，不增加用户输入面。

## 代码边界

- `remote/ephemeral_deploy.rs` 负责维护安全 PATH 前缀，并生成预检、启动和清理命令。
- `remote/ssh.rs` 的预检改用受控命令生成器，不再直接拼写 `uname -s; uname -m`。
- 不修改 `build_ssh_args` 的参数隔离职责，也不改变本地 `ssh`、`scp` 调用方式。
- 不修改 Agent 二进制、帧协议、远程目标模型或前端。

## 错误处理

- 工具在安全 PATH 中仍不可用时，保留现有失败分类：完整性阶段返回 `AGENT_INTEGRITY_FAILED`，启动阶段返回 `AGENT_START_FAILED`。
- SSH 登录警告继续从 stderr 捕获和清洗，不进入 Agent stdout 协议。
- 正常或异常退出仍由远端 trap 与桌面端独立 SSH 清理共同兜底。

## 测试

1. 先新增失败测试，断言预检、启动和清理命令都以安全 PATH 环境开头。
2. 断言 `wc`、`tr`、`sha256sum`、`awk`、`chmod`、`rm` 通过 `command` 调用，避免 alias 或函数劫持。
3. 保留并运行 `remote_ephemeral_deploy`、`remote_agent_minimal` 与相关 SSH 参数隔离测试。
4. 使用已下载的双架构 Agent 重新构建 Tauri dev。
5. 在 `dubhe-dev-xh` 上真实连接，确认不再出现 `command not found` 或 `AGENT_INTEGRITY_FAILED: size`，并确认会话退出后临时 Agent 被删除。

## 非目标

- 不为缺少 `sha256sum` 的非标准系统增加多套摘要工具探测。
- 不启动登录 shell，也不加载 `.zshrc`、`.profile` 等用户配置。
- 不在本次修复中调整 SSH banner 的展示文案。
- 不扩大远端可执行命令白名单或增加持久 Agent 缓存。

## 成功标准

- `dubhe-dev-xh` 在无需修改远端配置的情况下完成 Agent 校验和启动。
- 清理命令不受交互式 `rm -i` alias 影响。
- 自动化测试覆盖安全 PATH 与命令解析约束。
- Tauri dev 恢复运行，Git 工作区仅包含本次设计与实现改动。
