# AWS Bedrock UI 合并实施计划

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 将 AWS Bedrock 的两个独立 Claude preset（AKSK 和 API Key）合并为一个统一的 preset，支持平铺展示所有认证字段，根据 Region 自动生成 Base URL。

**Architecture:** 修改 preset 定义为单一合并 preset，扩展 TemplateValueConfig 类型支持 optional/isSecret 字段，在 UI 层和提交逻辑层处理条件性认证字段。Base URL 隐藏，由 Region 自动拼接。

**Tech Stack:** React, TypeScript, react-hook-form, shadcn/ui

---

### Task 1: 扩展 TemplateValueConfig 类型

**Files:**
- Modify: `src/config/claudeProviderPresets.ts:6-11`

**Step 1: 修改 TemplateValueConfig 接口**

在 `src/config/claudeProviderPresets.ts` 第 6-11 行，扩展接口：

```typescript
export interface TemplateValueConfig {
  label: string;
  placeholder: string;
  defaultValue?: string;
  editorValue: string;
  optional?: boolean;   // 可选字段，不参与必填校验
  isSecret?: boolean;    // 密码字段，使用 password 类型渲染
}
```

**Step 2: Commit**

```bash
cd /root/keith-space/github-search/cc-switch
git add src/config/claudeProviderPresets.ts
git commit -m "feat: extend TemplateValueConfig with optional and isSecret fields"
```

---

### Task 2: 合并 Bedrock Preset 定义

**Files:**
- Modify: `src/config/claudeProviderPresets.ts:536-610`

**Step 1: 替换两个旧 preset 为一个合并 preset**

删除第 536-610 行的两个 preset（`"AWS Bedrock (AKSK)"` 和 `"AWS Bedrock (API Key)"`），替换为：

```typescript
  {
    name: "AWS Bedrock",
    websiteUrl: "https://aws.amazon.com/bedrock/",
    settingsConfig: {
      apiKey: "${BEDROCK_API_KEY}",
      env: {
        ANTHROPIC_BASE_URL:
          "https://bedrock-runtime.${AWS_REGION}.amazonaws.com",
        AWS_ACCESS_KEY_ID: "${AWS_ACCESS_KEY_ID}",
        AWS_SECRET_ACCESS_KEY: "${AWS_SECRET_ACCESS_KEY}",
        AWS_REGION: "${AWS_REGION}",
        ANTHROPIC_MODEL: "global.anthropic.claude-opus-4-6-v1",
        ANTHROPIC_DEFAULT_HAIKU_MODEL:
          "global.anthropic.claude-haiku-4-5-20251001-v1:0",
        ANTHROPIC_DEFAULT_SONNET_MODEL:
          "global.anthropic.claude-sonnet-4-5-20250929-v1:0",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "global.anthropic.claude-opus-4-6-v1",
        CLAUDE_CODE_USE_BEDROCK: "1",
      },
    },
    category: "cloud_provider",
    templateValues: {
      AWS_REGION: {
        label: "AWS Region",
        placeholder: "us-west-2",
        editorValue: "us-west-2",
      },
      BEDROCK_API_KEY: {
        label: "Bedrock API Key (推荐)",
        placeholder: "your-bedrock-api-key",
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
        placeholder: "your-secret-key",
        editorValue: "",
        optional: true,
        isSecret: true,
      },
    },
    icon: "aws",
    iconColor: "#FF9900",
  },
```

**Step 2: Commit**

```bash
cd /root/keith-space/github-search/cc-switch
git add src/config/claudeProviderPresets.ts
git commit -m "feat: merge two Bedrock presets into single unified preset"
```

---

### Task 3: 更新模板值验证逻辑（跳过 optional 字段）

**Files:**
- Modify: `src/components/providers/forms/hooks/useTemplateValues.ts:257-282`

**Step 1: 修改 validateTemplateValues 跳过 optional 字段**

在 `useTemplateValues.ts` 的 `validateTemplateValues` 函数中，第 265 行后增加 optional 跳过逻辑：

```typescript
  const validateTemplateValues = useCallback((): {
    isValid: boolean;
    missingField?: { key: string; label: string };
  } => {
    if (templateValueEntries.length === 0) {
      return { isValid: true };
    }

    for (const [key, config] of templateValueEntries) {
      // 跳过可选字段
      if (config.optional) {
        continue;
      }
      const entry = templateValues[key];
      const resolvedValue = (
        entry?.editorValue ??
        entry?.defaultValue ??
        config.defaultValue ??
        ""
      ).trim();
      if (!resolvedValue) {
        return {
          isValid: false,
          missingField: { key, label: config.label },
        };
      }
    }

    return { isValid: true };
  }, [templateValueEntries, templateValues]);
```

**Step 2: Commit**

```bash
cd /root/keith-space/github-search/cc-switch
git add src/components/providers/forms/hooks/useTemplateValues.ts
git commit -m "feat: skip optional template values in validation"
```

---

### Task 4: 更新 ClaudeFormFields 渲染 isSecret 字段 + 隐藏 Bedrock 的 Base URL

**Files:**
- Modify: `src/components/providers/forms/ClaudeFormFields.tsx:133-153` (template rendering)
- Modify: `src/components/providers/forms/ClaudeFormFields.tsx:20-71` (props)

**Step 1: 添加 isBedrock prop**

在 `ClaudeFormFieldsProps` 接口中（约第 20 行）添加：

```typescript
  // Bedrock 模式标识
  isBedrock?: boolean;
```

在函数参数解构中也添加 `isBedrock`。

**Step 2: 更新模板变量渲染支持 isSecret**

修改第 133-153 行的模板变量渲染部分，让 `isSecret` 字段使用 `type="password"`：

```typescript
            {templateValueEntries.map(([key, config]) => (
              <div key={key} className="space-y-2">
                <FormLabel htmlFor={`template-${key}`}>
                  {config.label}
                </FormLabel>
                <Input
                  id={`template-${key}`}
                  type={config.isSecret ? "password" : "text"}
                  required={!config.optional}
                  value={
                    templateValues[key]?.editorValue ??
                    config.editorValue ??
                    config.defaultValue ??
                    ""
                  }
                  onChange={(e) => onTemplateValueChange(key, e.target.value)}
                  placeholder={config.placeholder || config.label}
                  autoComplete="off"
                />
              </div>
            ))}
```

**Step 3: 隐藏 Bedrock 的 Base URL 和 Speed Test**

修改第 158-189 行，在 Bedrock 模式下隐藏 Base URL 区域：

```typescript
      {/* Base URL 输入框 - Bedrock 模式下隐藏（URL 由 Region 自动生成） */}
      {shouldShowSpeedTest && !isBedrock && (
        <EndpointField
          ...
        />
      )}

      {/* 端点测速弹窗 - Bedrock 模式下隐藏 */}
      {shouldShowSpeedTest && !isBedrock && isEndpointModalOpen && (
        <EndpointSpeedTest
          ...
        />
      )}
```

**Step 4: Commit**

```bash
cd /root/keith-space/github-search/cc-switch
git add src/components/providers/forms/ClaudeFormFields.tsx
git commit -m "feat: support isSecret template fields and hide base URL for Bedrock"
```

---

### Task 5: 在 ProviderForm 中传递 isBedrock prop

**Files:**
- Modify: `src/components/providers/forms/ProviderForm.tsx:1920-1958`

**Step 1: 添加 isBedrock 判断**

在 ProviderForm 中 `ClaudeFormFields` 组件调用处（约第 1920 行），添加 `isBedrock` prop：

```typescript
        {appId === "claude" && (
          <ClaudeFormFields
            providerId={providerId}
            isBedrock={category === "cloud_provider"}
            shouldShowApiKey={shouldShowApiKey(
              ...
```

**Step 2: Commit**

```bash
cd /root/keith-space/github-search/cc-switch
git add src/components/providers/forms/ProviderForm.tsx
git commit -m "feat: pass isBedrock prop to ClaudeFormFields"
```

---

### Task 6: 更新提交验证逻辑

**Files:**
- Modify: `src/components/providers/forms/ProviderForm.tsx:1301-1355` (validation)
- Modify: `src/components/providers/forms/ProviderForm.tsx:1241-1253` (template validation)

**Step 1: 跳过 cloud_provider 的 baseUrl/apiKey 必填校验**

修改第 1301-1355 行的验证逻辑，`cloud_provider` 跳过通用的 endpoint 和 API Key 校验：

```typescript
    // 非官方供应商必填校验：端点和 API Key
    // cloud_provider（如 Bedrock）有自己的认证方式，跳过通用校验
    if (category !== "official" && category !== "cloud_provider") {
      if (appId === "claude") {
        if (!baseUrl.trim()) {
          ...
```

**Step 2: 添加 Bedrock 专属验证（Region 格式 + 至少一种认证）**

在第 1253 行（模板值验证之后）添加 Bedrock 专属验证：

```typescript
    // Bedrock 专属验证
    if (appId === "claude" && category === "cloud_provider") {
      // 验证 Region 格式
      const regionValue = templateValues.AWS_REGION?.editorValue?.trim() || "";
      const regionPattern = /^[a-z]{2}-[a-z]+-\d+$/;
      if (!regionPattern.test(regionValue)) {
        toast.error(
          t("providerForm.invalidRegion", {
            defaultValue: "请输入有效的 AWS Region 格式（如 us-west-2）",
          }),
        );
        return;
      }

      // 验证至少填写一种认证方式
      const apiKeyValue = templateValues.BEDROCK_API_KEY?.editorValue?.trim() || "";
      const accessKeyValue = templateValues.AWS_ACCESS_KEY_ID?.editorValue?.trim() || "";
      const secretKeyValue = templateValues.AWS_SECRET_ACCESS_KEY?.editorValue?.trim() || "";
      const hasApiKey = apiKeyValue.length > 0;
      const hasAksk = accessKeyValue.length > 0 && secretKeyValue.length > 0;

      if (!hasApiKey && !hasAksk) {
        toast.error(
          t("providerForm.bedrockAuthRequired", {
            defaultValue: "请至少填写一种认证方式：Bedrock API Key 或 Access Key ID + Secret Access Key",
          }),
        );
        return;
      }
    }
```

**Step 3: Commit**

```bash
cd /root/keith-space/github-search/cc-switch
git add src/components/providers/forms/ProviderForm.tsx
git commit -m "feat: add Bedrock-specific validation (region format + auth check)"
```

---

### Task 7: 添加提交时 Bedrock settingsConfig 清理逻辑

**Files:**
- Modify: `src/components/providers/forms/ProviderForm.tsx:1416-1418` (settingsConfig assembly)

**Step 1: 在 settingsConfig 组装处添加 Bedrock 清理**

在第 1416 行的 `else` 分支（Claude 的 settingsConfig 组装），添加 Bedrock 特殊处理：

```typescript
    } else if (appId === "claude" && category === "cloud_provider") {
      // Bedrock: 根据认证方式清理 settingsConfig
      try {
        const config = JSON.parse(values.settingsConfig.trim());
        const apiKeyValue = (config.apiKey || "").trim();
        const accessKeyValue = (config.env?.AWS_ACCESS_KEY_ID || "").trim();
        const secretKeyValue = (config.env?.AWS_SECRET_ACCESS_KEY || "").trim();

        if (apiKeyValue) {
          // API Key 模式：移除 AKSK 字段
          delete config.env.AWS_ACCESS_KEY_ID;
          delete config.env.AWS_SECRET_ACCESS_KEY;
        } else {
          // AKSK 模式：移除空的 apiKey
          delete config.apiKey;
          // 如果 AKSK 字段也为空则已在验证阶段拦截
        }

        // 移除空的 apiKey（如果值为空字符串）
        if (!apiKeyValue) {
          delete config.apiKey;
        }

        settingsConfig = JSON.stringify(config);
      } catch {
        settingsConfig = values.settingsConfig.trim();
      }
    } else {
      settingsConfig = values.settingsConfig.trim();
    }
```

注意：需要把原来的 `} else {` 改成 `} else if (...) { ... } else {`。

**Step 2: Commit**

```bash
cd /root/keith-space/github-search/cc-switch
git add src/components/providers/forms/ProviderForm.tsx
git commit -m "feat: clean up Bedrock settingsConfig based on auth method on submit"
```

---

### Task 8: 验证和测试

**Step 1: 启动开发服务器**

```bash
cd /root/keith-space/github-search/cc-switch
npm run dev
```

**Step 2: 手动验证清单**

1. 打开添加供应商页面，确认只有一个 "AWS Bedrock" 预设（不再有两个）
2. 选择 "AWS Bedrock" 预设，确认显示 4 个字段：Region、API Key、Access Key ID、Secret Access Key
3. 确认 Secret Access Key 字段为密码输入类型
4. 确认 Base URL 不可见
5. 只填 Region + API Key，提交，确认 settingsConfig 中无 AKSK 字段
6. 只填 Region + AKSK，提交，确认 settingsConfig 中无 apiKey 字段
7. 都不填认证字段，提交，确认出现错误提示
8. Region 填无效值（如 "abc"），提交，确认出现格式错误提示
9. 两种认证都填，提交，确认 API Key 优先（无 AKSK 字段）

**Step 3: 确认无 TypeScript 错误**

```bash
cd /root/keith-space/github-search/cc-switch
npx tsc --noEmit
```

**Step 4: Commit 最终状态（如有修复）**

```bash
cd /root/keith-space/github-search/cc-switch
git add -A
git commit -m "fix: address issues found during testing"
```
