# CC Switch 配置机制

## 配置目录

CC Switch 通过 `claudeConfigDir` 设置来确定 Claude 的配置目录。

- **默认值**：`C:\Users\<username>\.claude`
- **可配置**：可以设置为 WSL 路径（如 `\\wsl.localhost\Ubuntu2\...`）

## 配置写入机制

### 1. 直接修改 settings.json

CC Switch 会**直接修改**目标配置目录下的 `settings.json` 文件：

```rust
// src/services/provider/live.rs
let path = get_claude_settings_path();  // 获取配置目录（受 claudeConfigDir 影响）
write_json_file(&path, &settings)?;    // 直接写入 settings.json
```

这意味着：
- 如果 `claudeConfigDir` 设置为 WSL 路径，CC Switch 会直接修改 WSL 里的 `settings.json`
- WSL 里的 Claude 无需额外配置，直接读取该目录

### 2. 启动时传入临时配置

除了直接修改 settings.json，CC Switch UI里点击启动 Claude 时还会：

```bash
claude --settings /tmp/claude_xxx.json
```

这个临时配置文件包含：
- API Key
- Base URL
- 自定义环境变量

## WSL 支持

### 多发行版配置

CC Switch 现在会自动启用系统中所有已安装的 WSL 发行版（无需在 `settings.json` 单独配置）。

### CLAUDE_CONFIG_DIR 环境变量

当通过 WSL 启动 Claude 时，CC Switch 会设置：

```bash
export CLAUDE_CONFIG_DIR="/mnt/c/Users/xxx/.claude"
claude --settings /tmp/config.json
```

这样 WSL 里的 Claude 会读取 Windows 的配置目录。

## 配置优先级

1. **启动时参数**：`--settings` 临时配置（只对当前会话生效）
2. **环境变量**：`CLAUDE_CONFIG_DIR`（如果设置）
3. **默认配置**：`~/.claude/settings.json`

## 注意事项

- 直接修改 settings.json 会持久化配置
- 临时配置文件只在当前会话生效
- WSL 访问 Windows 文件系统需要正确的路径格式
