# AWS Bedrock UI 合并设计

## 背景

cc-switch 项目中 AWS Bedrock 当前有两个独立的 Claude preset：`AWS Bedrock (AKSK)` 和 `AWS Bedrock (API Key)`。两者功能高度重叠，仅认证方式不同。需要将它们合并为一个统一的 preset，简化用户体验。

## 需求

1. 将两个 Bedrock preset 合并为一个 `"AWS Bedrock"` preset
2. API Key 为推荐认证方式，AKSK 为备选
3. 用户无需手动填写请求地址，根据 Region 自动生成
4. Region 保留文本输入，增加格式验证
5. 所有认证字段平铺展示，用户填哪个就用哪个
6. 两种认证都填时，API Key 优先

## 方案选择

选择 **方案 A：合并为单个 Preset，全字段平铺**。理由：改动集中，不需要新 UI 组件，符合 YAGNI 原则。

## 设计

### Preset 数据结构

将 `claudeProviderPresets.ts` 中的两个旧 preset 合并为一个：

```typescript
{
  id: "claude_bedrock",
  name: "AWS Bedrock",
  category: "cloud_provider",
  icon: { type: "aws", color: "#FF9900" },
  templateValues: {
    AWS_REGION: {
      label: "AWS Region",
      placeholder: "us-west-2",
      editorValue: "us-west-2",
    },
    BEDROCK_API_KEY: {
      label: "Bedrock API Key (推荐)",
      placeholder: "br-...",
      editorValue: "",
      optional: true,
    },
    AWS_ACCESS_KEY_ID: {
      label: "Access Key ID",
      placeholder: "AKIA...",
      editorValue: "",
      optional: true,
    },
    AWS_SECRET_ACCESS_KEY: {
      label: "Secret Access Key",
      placeholder: "wJalr...",
      editorValue: "",
      isSecret: true,
      optional: true,
    },
  },
  settingsConfig: {
    env: {
      ANTHROPIC_BASE_URL: "https://bedrock-runtime.${AWS_REGION}.amazonaws.com",
      AWS_REGION: "${AWS_REGION}",
      CLAUDE_CODE_USE_BEDROCK: "1",
    },
  },
}
```

### 提交逻辑

表单提交时根据填写内容条件性生成 settingsConfig：

1. **API Key 有值** → 写入 `apiKey` 字段，不写入 AKSK env
2. **API Key 为空，AKSK 都有值** → 写入 `AWS_ACCESS_KEY_ID` 和 `AWS_SECRET_ACCESS_KEY` 到 env
3. **都为空** → 验证拦截，提示"请至少填写一种认证方式"

API Key 模式输出：
```typescript
{
  env: {
    ANTHROPIC_BASE_URL: "https://bedrock-runtime.us-west-2.amazonaws.com",
    AWS_REGION: "us-west-2",
    CLAUDE_CODE_USE_BEDROCK: "1",
  },
  apiKey: "br-xxxxx",
}
```

AKSK 模式输出：
```typescript
{
  env: {
    ANTHROPIC_BASE_URL: "https://bedrock-runtime.us-west-2.amazonaws.com",
    AWS_REGION: "us-west-2",
    AWS_ACCESS_KEY_ID: "AKIA...",
    AWS_SECRET_ACCESS_KEY: "wJalr...",
    CLAUDE_CODE_USE_BEDROCK: "1",
  },
}
```

### Region 验证

- 正则：`^[a-z]{2}-[a-z]+-\d+$`
- 匹配示例：`us-west-2`, `ap-northeast-1`, `eu-central-1`
- 验证失败提示：请输入有效的 AWS Region 格式

### UI 布局

```
┌─────────────────────────────────────────┐
│  AWS Bedrock                            │
├─────────────────────────────────────────┤
│  AWS Region          [us-west-2       ] │
│  Bedrock API Key     [               ] │  ← 推荐
│  Access Key ID       [               ] │
│  Secret Access Key   [●●●●●●●●       ] │
│  ── Model Selection ──                  │
│  Haiku / Sonnet / Opus dropdowns        │
│            [  Save  ]                   │
└─────────────────────────────────────────┘
```

- Base URL 完全隐藏，根据 Region 自动拼接
- Secret Access Key 使用密码遮罩
- API Key 字段排在 AKSK 之前

## 涉及文件

| 文件 | 改动 |
|------|------|
| `src/config/claudeProviderPresets.ts` | 删除两个旧 Bedrock preset，新建合并的 preset |
| `src/utils/providerConfigUtils.ts` | 修改 `applyTemplateValues()`，增加 Bedrock 条件逻辑 |
| `src/components/providers/forms/ClaudeFormFields.tsx` | optional 字段渲染，Bedrock 下隐藏 Base URL |
| `src/components/providers/forms/ProviderForm.tsx` | Bedrock 表单验证（至少一种认证 + Region 格式） |

## 不在范围内

- OpenCode 的 Bedrock preset
- Model 选择器逻辑
- 后端存储结构
- 其他 provider 配置
