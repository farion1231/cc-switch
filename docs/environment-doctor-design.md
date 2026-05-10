# CC Doctor 环境诊断与修复功能设计方案

## 一、项目背景

### 1.1 现状分析

**cc-doctor 已有能力**：
- ✅ 完整的工具版本检测（Claude Code、Codex、Gemini CLI、OpenCode）
- ✅ 环境变量冲突检测（`check_env_conflicts`）
- ✅ 跨平台支持（macOS、Linux、Windows/WSL）
- ✅ 配置文件读写能力
- ✅ 系统命令执行能力（Tauri）
- ✅ 完善的 UI 组件库（AboutSection.tsx）

**用户痛点**：
1. 未安装 Claude Code 时，只能手动复制命令执行
2. 环境冲突（官方登录 + API key 共存）导致报错，用户不知如何修复
3. Node.js 版本不兼容、配置文件损坏等问题缺少自动化修复手段

### 1.2 目标

在 cc-doctor 现有架构上扩展，实现：
1. **一键安装**：自动检测并安装 Claude Code 及其依赖（Node.js）
2. **一键修复**：自动诊断并修复常见环境问题
3. **智能引导**：根据检测结果展示对应的操作按钮

---

## 二、架构设计

### 2.1 整体架构

```
┌─────────────────────────────────────────────────────────────┐
│                    前端 UI 层 (React/TypeScript)              │
│  AboutSection.tsx - 环境检查 UI                               │
│  EnvironmentDoctorPanel.tsx - 诊断结果展示（新增）            │
└─────────────────────────────────────────────────────────────┘
                            ↓ Tauri IPC
┌─────────────────────────────────────────────────────────────┐
│                    后端命令层 (Rust)                          │
│  commands/doctor.rs - 环境诊断与修复命令（新增）              │
│  commands/misc.rs - 扩展现有工具检测                          │
└─────────────────────────────────────────────────────────────┘
                            ↓
┌─────────────────────────────────────────────────────────────┐
│                    服务层 (Rust)                              │
│  services/env_doctor.rs - 诊断逻辑（新增）                    │
│  services/installer.rs - 安装逻辑（新增）                     │
│  services/env_checker.rs - 复用现有冲突检测                   │
└─────────────────────────────────────────────────────────────┘
                            ↓
┌─────────────────────────────────────────────────────────────┐
│                    系统层 (Shell/Node.js)                     │
│  执行安装脚本、修复配置文件                                    │
└─────────────────────────────────────────────────────────────┘
```

### 2.2 数据流

```
用户打开"关于"页面
  ↓
前端调用 diagnose_environment()
  ↓
后端执行诊断逻辑
  ├─ 检测工具安装状态
  ├─ 检测 Node.js 版本
  ├─ 检测环境变量冲突
  ├─ 检测配置文件完整性
  └─ 检测权限问题
  ↓
返回诊断结果 DiagnosisResult
  ↓
前端根据结果展示：
  - 未安装 → [一键安装] 按钮
  - 已安装但有问题 → [一键修复] 按钮
  - 一切正常 → "环境正常" 提示
```

---

## 三、核心功能设计

### 3.1 环境诊断（Diagnosis）

#### 3.1.1 诊断项清单

| 诊断项 | 检测内容 | 严重程度 |
|--------|---------|---------|
| **工具安装状态** | Claude Code / Codex / Gemini CLI / OpenCode 是否已安装 | Critical |
| **Node.js 环境** | Node.js 版本是否满足要求（≥18.0.0） | Critical |
| **环境变量冲突** | ANTHROPIC_API_KEY 等是否与官方登录冲突 | High |
| **配置文件完整性** | settings.json 是否存在、格式是否正确 | High |
| **权限问题** | 配置目录是否可读写 | Medium |
| **版本过期** | 工具版本是否落后于最新版 | Low |

#### 3.1.2 数据结构

```rust
// src-tauri/src/services/env_doctor.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosisResult {
    pub overall_status: HealthStatus,
    pub issues: Vec<DiagnosisIssue>,
    pub tools_status: HashMap<String, ToolStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HealthStatus {
    Healthy,           // 一切正常
    NeedsInstall,      // 需要安装
    NeedsRepair,       // 需要修复
    PartiallyHealthy,  // 部分工具有问题
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosisIssue {
    pub id: String,
    pub severity: IssueSeverity,
    pub category: IssueCategory,
    pub title: String,
    pub description: String,
    pub auto_fixable: bool,
    pub fix_action: Option<FixAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IssueSeverity {
    Critical,  // 阻塞使用
    High,      // 严重影响
    Medium,    // 中等影响
    Low,       // 轻微影响
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IssueCategory {
    NotInstalled,
    EnvConflict,
    ConfigCorrupted,
    PermissionDenied,
    VersionOutdated,
    NodeJsMissing,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FixAction {
    InstallTool { tool: String },
    InstallNodeJs,
    RemoveEnvVar { var_name: String, source: String },
    RepairConfig { path: String },
    FixPermission { path: String },
    UpdateTool { tool: String, current: String, latest: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolStatus {
    pub installed: bool,
    pub version: Option<String>,
    pub latest_version: Option<String>,
    pub issues: Vec<String>,
}
```

### 3.2 一键安装（Install）

#### 3.2.1 安装流程

```
检测 Node.js
  ├─ 已安装且版本符合 → 跳过
  └─ 未安装或版本过低 → 安装 Node.js
      ├─ macOS: 使用 Homebrew (brew install node)
      └─ 失败 → 提示用户手动安装
  ↓
安装 Claude Code
  ├─ 执行官方安装脚本: curl -fsSL https://claude.ai/install.sh | bash
  ├─ 实时反馈安装进度
  └─ 安装完成 → 验证安装结果
  ↓
返回安装结果
```

#### 3.2.2 命令设计

```rust
// src-tauri/src/commands/doctor.rs

#[tauri::command]
pub async fn install_tool(
    tool: String,
    app: AppHandle,
) -> Result<InstallResult, String> {
    // 1. 检查 Node.js
    if !check_nodejs_installed()? {
        install_nodejs(app.clone()).await?;
    }
    
    // 2. 执行安装脚本
    let result = match tool.as_str() {
        "claude" => install_claude_code(app).await?,
        "codex" => install_codex(app).await?,
        "gemini" => install_gemini_cli(app).await?,
        "opencode" => install_opencode(app).await?,
        _ => return Err(format!("Unsupported tool: {}", tool)),
    };
    
    Ok(result)
}

#[derive(Debug, Clone, Serialize)]
pub struct InstallResult {
    pub success: bool,
    pub message: String,
    pub installed_version: Option<String>,
}
```

### 3.3 一键修复（Repair）

#### 3.3.1 修复策略

| 问题类型 | 修复策略 | 实现方式 |
|---------|---------|---------|
| **环境变量冲突** | 备份后删除冲突的环境变量 | 复用 `delete_env_vars` |
| **配置文件损坏** | 从备份恢复或重新生成默认配置 | 读取 `.backup` 文件或生成模板 |
| **权限问题** | 修复目录权限（chmod） | `std::fs::set_permissions` |
| **版本过期** | 提示用户更新（不自动更新） | 展示更新命令 |

#### 3.3.2 命令设计

```rust
#[tauri::command]
pub async fn fix_environment(
    issues: Vec<DiagnosisIssue>,
    app: AppHandle,
) -> Result<FixResult, String> {
    let mut fixed = Vec::new();
    let mut failed = Vec::new();
    
    for issue in issues {
        if !issue.auto_fixable {
            continue;
        }
        
        let result = match issue.fix_action {
            Some(FixAction::RemoveEnvVar { var_name, source }) => {
                fix_env_conflict(var_name, source).await
            }
            Some(FixAction::RepairConfig { path }) => {
                repair_config_file(path).await
            }
            Some(FixAction::FixPermission { path }) => {
                fix_permission(path).await
            }
            _ => continue,
        };
        
        match result {
            Ok(_) => fixed.push(issue.id),
            Err(e) => failed.push((issue.id, e)),
        }
    }
    
    Ok(FixResult { fixed, failed })
}

#[derive(Debug, Clone, Serialize)]
pub struct FixResult {
    pub fixed: Vec<String>,
    pub failed: Vec<(String, String)>,
}
```

---

## 四、前端 UI 设计

### 4.1 组件结构

```tsx
// src/components/settings/AboutSection.tsx (扩展现有组件)

export function AboutSection({ isPortable }: AboutSectionProps) {
  const [diagnosis, setDiagnosis] = useState<DiagnosisResult | null>(null);
  const [isInstalling, setIsInstalling] = useState(false);
  const [isFixing, setIsFixing] = useState(false);
  
  // 执行诊断
  const runDiagnosis = async () => {
    const result = await settingsApi.diagnoseEnvironment();
    setDiagnosis(result);
  };
  
  // 一键安装
  const handleInstall = async (tool: string) => {
    setIsInstalling(true);
    try {
      const result = await settingsApi.installTool(tool);
      toast.success(t('doctor.installSuccess', { tool }));
      await runDiagnosis(); // 重新诊断
    } catch (error) {
      toast.error(t('doctor.installFailed', { error }));
    } finally {
      setIsInstalling(false);
    }
  };
  
  // 一键修复
  const handleFix = async () => {
    setIsFixing(true);
    try {
      const fixableIssues = diagnosis.issues.filter(i => i.auto_fixable);
      const result = await settingsApi.fixEnvironment(fixableIssues);
      toast.success(t('doctor.fixSuccess', { count: result.fixed.length }));
      await runDiagnosis(); // 重新诊断
    } catch (error) {
      toast.error(t('doctor.fixFailed', { error }));
    } finally {
      setIsFixing(false);
    }
  };
  
  useEffect(() => {
    runDiagnosis();
  }, []);
  
  return (
    <motion.section>
      {/* 现有的版本信息卡片 */}
      
      {/* 环境诊断卡片（新增） */}
      {diagnosis && (
        <EnvironmentDoctorPanel
          diagnosis={diagnosis}
          onInstall={handleInstall}
          onFix={handleFix}
          isInstalling={isInstalling}
          isFixing={isFixing}
        />
      )}
      
      {/* 现有的工具版本检测 */}
    </motion.section>
  );
}
```

### 4.2 诊断结果展示

```tsx
// src/components/settings/EnvironmentDoctorPanel.tsx (新增)

interface EnvironmentDoctorPanelProps {
  diagnosis: DiagnosisResult;
  onInstall: (tool: string) => Promise<void>;
  onFix: () => Promise<void>;
  isInstalling: boolean;
  isFixing: boolean;
}

export function EnvironmentDoctorPanel({
  diagnosis,
  onInstall,
  onFix,
  isInstalling,
  isFixing,
}: EnvironmentDoctorPanelProps) {
  const { t } = useTranslation();
  
  const getStatusIcon = () => {
    switch (diagnosis.overall_status) {
      case 'Healthy':
        return <CheckCircle2 className="h-5 w-5 text-green-500" />;
      case 'NeedsInstall':
        return <AlertCircle className="h-5 w-5 text-yellow-500" />;
      case 'NeedsRepair':
        return <XCircle className="h-5 w-5 text-red-500" />;
      default:
        return <Info className="h-5 w-5 text-blue-500" />;
    }
  };
  
  return (
    <motion.div className="rounded-xl border bg-card p-6 space-y-4">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          {getStatusIcon()}
          <h3 className="text-lg font-semibold">
            {t('doctor.environmentStatus')}
          </h3>
        </div>
        
        {/* 操作按钮 */}
        {diagnosis.overall_status === 'NeedsInstall' && (
          <Button
            onClick={() => onInstall('claude')}
            disabled={isInstalling}
          >
            {isInstalling ? (
              <>
                <Loader2 className="h-4 w-4 animate-spin mr-2" />
                {t('doctor.installing')}
              </>
            ) : (
              <>
                <Download className="h-4 w-4 mr-2" />
                {t('doctor.oneClickInstall')}
              </>
            )}
          </Button>
        )}
        
        {diagnosis.overall_status === 'NeedsRepair' && (
          <Button
            onClick={onFix}
            disabled={isFixing}
            variant="destructive"
          >
            {isFixing ? (
              <>
                <Loader2 className="h-4 w-4 animate-spin mr-2" />
                {t('doctor.fixing')}
              </>
            ) : (
              <>
                <Wrench className="h-4 w-4 mr-2" />
                {t('doctor.oneClickFix')}
              </>
            )}
          </Button>
        )}
      </div>
      
      {/* 问题列表 */}
      {diagnosis.issues.length > 0 && (
        <div className="space-y-2">
          {diagnosis.issues.map((issue) => (
            <IssueCard key={issue.id} issue={issue} />
          ))}
        </div>
      )}
      
      {/* 健康状态 */}
      {diagnosis.overall_status === 'Healthy' && (
        <div className="text-sm text-muted-foreground">
          {t('doctor.allGood')}
        </div>
      )}
    </motion.div>
  );
}
```

---

## 五、文件改动清单

### 5.1 新增文件

```
src-tauri/src/
├── commands/
│   └── doctor.rs                    # 新增：诊断与修复命令
├── services/
│   ├── env_doctor.rs                # 新增：诊断逻辑
│   └── installer.rs                 # 新增：安装逻辑

src/
├── components/settings/
│   └── EnvironmentDoctorPanel.tsx   # 新增：诊断结果 UI
├── lib/api/
│   └── doctor.ts                    # 新增：前端 API 封装
```

### 5.2 修改文件

```
src-tauri/src/
├── commands/mod.rs                  # 添加 doctor 模块导出
├── lib.rs                           # 注册新命令
└── services/mod.rs                  # 添加新服务模块

src/
├── components/settings/AboutSection.tsx  # 集成诊断面板
├── i18n/locales/
│   ├── zh.json                      # 添加中文翻译
│   └── en.json                      # 添加英文翻译
```

---

## 六、国际化文案

### 6.1 中文（zh.json）

```json
{
  "doctor": {
    "environmentStatus": "环境状态",
    "oneClickInstall": "一键安装",
    "oneClickFix": "一键修复",
    "installing": "安装中...",
    "fixing": "修复中...",
    "allGood": "✓ 环境正常，所有工具运行良好",
    "installSuccess": "{{tool}} 安装成功",
    "installFailed": "安装失败：{{error}}",
    "fixSuccess": "成功修复 {{count}} 个问题",
    "fixFailed": "修复失败：{{error}}",
    "issues": {
      "notInstalled": "{{tool}} 未安装",
      "envConflict": "环境变量冲突：{{var}}",
      "configCorrupted": "配置文件损坏：{{path}}",
      "permissionDenied": "权限不足：{{path}}",
      "versionOutdated": "{{tool}} 版本过期（当前：{{current}}，最新：{{latest}}）",
      "nodejsMissing": "Node.js 未安装或版本过低"
    }
  }
}
```

### 6.2 英文（en.json）

```json
{
  "doctor": {
    "environmentStatus": "Environment Status",
    "oneClickInstall": "One-Click Install",
    "oneClickFix": "One-Click Fix",
    "installing": "Installing...",
    "fixing": "Fixing...",
    "allGood": "✓ All good, everything is working properly",
    "installSuccess": "{{tool}} installed successfully",
    "installFailed": "Installation failed: {{error}}",
    "fixSuccess": "Successfully fixed {{count}} issue(s)",
    "fixFailed": "Fix failed: {{error}}",
    "issues": {
      "notInstalled": "{{tool}} is not installed",
      "envConflict": "Environment variable conflict: {{var}}",
      "configCorrupted": "Config file corrupted: {{path}}",
      "permissionDenied": "Permission denied: {{path}}",
      "versionOutdated": "{{tool}} is outdated (current: {{current}}, latest: {{latest}})",
      "nodejsMissing": "Node.js is not installed or version is too low"
    }
  }
}
```

---

## 七、风险评估与缓解

### 7.1 风险清单

| 风险 | 严重程度 | 缓解措施 |
|------|---------|---------|
| **安装脚本执行失败** | High | 1. 详细的错误日志<br>2. 回退机制<br>3. 提供手动安装指引 |
| **权限不足** | Medium | 1. 检测权限后再执行<br>2. 提示用户使用 sudo<br>3. 提供手动修复步骤 |
| **配置文件损坏** | High | 1. 修复前自动备份<br>2. 提供恢复选项<br>3. 生成默认配置 |
| **环境变量误删** | Medium | 1. 删除前备份<br>2. 提供恢复功能<br>3. 用户确认后再执行 |
| **Node.js 安装失败** | Medium | 1. 检测 Homebrew 是否可用<br>2. 提供官方下载链接<br>3. 跳过 Node.js 安装 |

### 7.2 安全措施

1. **命令注入防护**：
   - 所有用户输入严格校验
   - 使用 Rust 的 `Command::new()` 而非 shell 字符串拼接

2. **权限最小化**：
   - 优先使用用户权限
   - 仅在必要时提示 sudo

3. **备份机制**：
   - 所有破坏性操作前自动备份
   - 备份文件带时间戳
   - 提供一键恢复功能

4. **用户确认**：
   - 高风险操作前弹窗确认
   - 清晰说明操作后果

---

## 八、实施计划

### Phase 1：环境诊断（1-2 天）
- [ ] 实现 `diagnose_environment` 命令
- [ ] 实现诊断逻辑（检测工具、Node.js、环境变量、配置文件）
- [ ] 前端展示诊断结果
- [ ] 添加国际化文案

### Phase 2：一键安装（2-3 天）
- [ ] 实现 Node.js 检测与安装
- [ ] 实现 Claude Code 安装脚本执行
- [ ] 实时反馈安装进度
- [ ] 错误处理与回退机制
- [ ] 前端集成安装按钮

### Phase 3：一键修复（2-3 天）
- [ ] 实现环境变量冲突修复
- [ ] 实现配置文件修复
- [ ] 实现权限修复
- [ ] 备份与恢复机制
- [ ] 前端集成修复按钮

### Phase 4：测试与优化（1-2 天）
- [ ] 单元测试
- [ ] 集成测试
- [ ] 用户体验优化
- [ ] 文档完善

**总计：6-10 天**

---

## 九、后续扩展

### 9.1 Windows 支持（Phase 5）
- PowerShell 脚本实现
- WSL 环境检测与修复
- Windows 特有问题处理

### 9.2 高级诊断（Phase 6）
- 网络连接检测
- API 可用性测试
- 代理配置验证
- 磁盘空间检查

### 9.3 自动修复增强（Phase 7）
- 智能推荐修复方案
- 批量修复多个问题
- 定时健康检查
- 问题预警机制

---

## 十、总结

本方案基于 cc-doctor 现有架构，通过扩展而非重构的方式，实现环境诊断与修复功能。核心优势：

1. **低风险**：不改动核心逻辑，只在现有能力上扩展
2. **高复用**：充分利用现有的环境检测、配置管理能力
3. **渐进式**：可分阶段实施，每个阶段都能独立交付价值
4. **可扩展**：架构设计支持后续功能扩展

**下一步**：确认方案后，开始 Phase 1 的实现。
