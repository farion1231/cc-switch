# CC Switch — 快速上手指南

## 启动应用

```bash
cd D:\14-OneAgentSwithc

# 设置编译环境（每次新终端需要执行）
export PATH="/c/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/VC/Tools/MSVC/14.44.35207/bin/Hostx64/x64:$PATH"
export LIB="/c/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/VC/Tools/MSVC/14.44.35207/lib/x64;/c/Program Files (x86)/Windows Kits/10/Lib/10.0.26100.0/um/x64;/c/Program Files (x86)/Windows Kits/10/Lib/10.0.26100.0/ucrt/x64"
export INCLUDE="/c/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/VC/Tools/MSVC/14.44.35207/include;/c/Program Files (x86)/Windows Kits/10/Include/10.0.26100.0/ucrt;/c/Program Files (x86)/Windows Kits/10/Include/10.0.26100.0/um;/c/Program Files (x86)/Windows Kits/10/Include/10.0.26100.0/shared"

# 开发模式（带热重载 + 调试）
pnpm dev

# 或直接运行编译好的产品
./src-tauri/target/release/cc-switch.exe
```

---

## 第一步：导入供应商

首次打开会看到「还没有添加任何供应商」，点击 **「导入当前配置」**，自动读取本地的 Claude/Codex/Gemini 配置。

---

## 第二步：切换供应商

顶部导航栏有多个 App 标签：

| 按钮 | 对应工具 |
|------|----------|
| Claude | Claude Code CLI |
| Claude Desktop | Claude Desktop 桌面版 |
| Codex | OpenAI Codex CLI |
| Gemini | Google Gemini CLI |

每个 App 下可添加多个供应商（不同的 API key + 地址），点击即可切换。

---

## 第三步：启动代理服务器

应用内置了一个代理服务器（默认 `127.0.0.1:15721`），用于拦截和转发 AI 请求。

1. 进入 **设置** → 找到代理相关配置
2. 点击 **启动代理**
3. 代理会自动接管对应 App 的请求

---

## 第四步：启用编排引擎（核心新功能）

### 打开编排面板

点击顶部导航栏的 **「编排引擎」** 按钮。

### 界面说明

| 面板 | 作用 |
|------|------|
| **编排引擎状态** | 开关按钮 + 重新加载配置 |
| **策略编辑器** | 查看/编辑 4 种策略（route、cascade、debate、moa） |
| **执行流程** | 可视化展示 Request → Classify → Strategy → Quality Gate → Response |
| **审计日志** | 每次编排的详细事件记录 |
| **人工审批** | Critical/High 风险任务的人工确认队列 |
| **模型排行榜** | 各模型的质量分数和调用统计 |
| **预测洞察** | AI 生成的优化建议 |

### 启用编排

1. 打开 **编排引擎** 面板
2. 切换 **「启用多模型编排」** 开关为开
3. 编排引擎开始工作

### 策略配置

策略文件在 `configs/strategies.yaml`，可热修改（改完点「重新加载配置」即可）：

```yaml
strategies:
  route:        # 简单路由：低复杂度 → 便宜模型
    when:
      complexity: [0, 0.4]
      risk: ["low"]
    action:
      type: route
      use_model: cheap_coder      # DeepSeek

  cascade:      # 级联验证：中复杂度 → 便宜模型 → 质量检查 → 升级
    when:
      complexity: [0.4, 0.7]
      risk: ["medium", "high"]
    action:
      type: cascade
      models: [cheap_coder, frontier]  # DeepSeek → Anthropic
      quality_threshold: 0.65

  debate:       # 多模型辩论：高复杂度 → 多模型讨论
    when:
      complexity: [0.7, 1.0]
      risk: ["high", "critical"]

  moa:          # 智能体混合：极高复杂度 + 编码任务
    when:
      complexity: [0.8, 1.0]
      risk: ["critical"]
      task_type: ["coding", "architecture"]
```

**策略匹配规则：** 请求进来后，根据复杂度评分和风险等级自动匹配策略。低复杂度用便宜模型，高复杂度用强模型 + 质量验证。

---

## 其他功能速览

| 功能 | 入口 | 说明 |
|------|------|------|
| Skills 管理 | 顶部「Skills」 | 安装/管理 Claude 技能 |
| MCP 管理 | 顶部「MCP 管理」 | 配置 MCP 服务器 |
| 提示词 | 顶部「提示词」 | 编辑各 App 的系统提示词 |
| 会话管理 | 顶部「会话管理」 | 浏览/删除历史会话 |
| 用量统计 | 设置中查看 | 请求数、token 数、费用 |

---

## 编译命令速查

```bash
# 检查编译（最快）
cargo check --manifest-path src-tauri/Cargo.toml

# 运行单元测试
cargo test --manifest-path src-tauri/Cargo.toml --lib -- orchestration

# 编译 release 版本
cargo build --manifest-path src-tauri/Cargo.toml --release

# 编译前端
pnpm build:renderer
```
