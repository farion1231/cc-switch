# Vertex AI 服务账号支持实现总结

## 已完成的后端实现

### 1. 核心文件

#### 新增文件
- `src-tauri/src/proxy/providers/vertex.rs` - Vertex AI 适配器实现

#### 修改文件
- `src-tauri/src/proxy/providers/adapter.rs` - 添加 `build_url_with_provider` 和 `add_auth_headers_async` 方法
- `src-tauri/src/proxy/providers/mod.rs` - 导出 VertexAdapter，添加 Vertex 到 ProviderType 枚举
- `src-tauri/src/proxy/forwarder.rs` - 使用新的异步认证方法

### 2. 功能特性

#### 认证模式
1. **API Key 模式（快速模式）**
   - 不需要 project_id
   - API Key 直接拼接在 URL 中
   - 适用于简单场景

2. **服务账号模式**
   - 支持上传 GCP 服务账号 JSON 文件
   - 自动从 JSON 中提取 project_id
   - 使用 `gcp_auth` 库异步获取 Bearer token
   - Token 自动缓存，根据过期时间自动刷新

#### 请求模式
根据模型名称自动检测：
- **Gemini**: `gemini-*` 模型
- **Claude**: `claude-*` 模型
- **OpenSource**: 包含 `llama` 或 `-maas` 的模型

#### URL 构建规则
参考 Go 代码实现，支持：
- 服务账号模式：包含 project_id 和 region
- API Key 模式：不包含 project_id
- 支持 global 和区域化端点
- 自动根据请求模式选择正确的 publisher (google/anthropic)

### 3. 配置字段

Provider 配置支持以下字段：

```json
{
  "env": {
    "VERTEX_API_KEY": "AIza-xxx" // 或服务账号 JSON 字符串
  },
  "region": "global" // 可选，默认 global
}
```

或者：

```json
{
  "apiKey": "AIza-xxx", // 或服务账号 JSON 字符串
  "region": "us-central1"
}
```

## 前端需要实现的功能

### 1. Provider 配置界面

#### 基础配置
- **Provider 名称**: 文本输入
- **Region**: 下拉选择或文本输入
  - 默认值: `global`
  - 常用选项: `global`, `us-central1`, `us-east5`, `europe-west1`, `asia-northeast1` 等

#### 认证配置
提供两种认证方式的切换：

**方式一：API Key（快速模式）**
- 输入框：API Key
- 说明：适用于简单场景，不需要 project_id

**方式二：服务账号（推荐）**
- 文件上传：支持上传 `.json` 格式的服务账号文件
- 或文本框：直接粘贴服务账号 JSON 内容
- 说明：支持完整的 Vertex AI 功能，包括 Claude 和开源模型

### 2. 服务账号 JSON 文件上传

#### 文件验证
上传后需要验证 JSON 格式，确保包含必要字段：
```json
{
  "type": "service_account",
  "project_id": "your-project-id",
  "private_key_id": "...",
  "private_key": "...",
  "client_email": "...",
  "client_id": "...",
  "auth_uri": "...",
  "token_uri": "...",
  "auth_provider_x509_cert_url": "...",
  "client_x509_cert_url": "..."
}
```

#### 存储方式
将整个 JSON 内容作为字符串存储到 `env.VERTEX_API_KEY` 或 `apiKey` 字段中。

### 3. UI 建议

```
┌─────────────────────────────────────────┐
│ Vertex AI Provider 配置                  │
├─────────────────────────────────────────┤
│                                         │
│ Provider 名称: [Vertex AI Production]   │
│                                         │
│ Region: [global ▼]                      │
│                                         │
│ 认证方式:                                │
│ ○ API Key (快速模式)                     │
│ ● 服务账号 (推荐)                        │
│                                         │
│ ┌─────────────────────────────────────┐ │
│ │ 上传服务账号 JSON 文件                │ │
│ │ [选择文件] service-account.json      │ │
│ │                                     │ │
│ │ 或直接粘贴 JSON 内容:                │ │
│ │ ┌─────────────────────────────────┐ │ │
│ │ │ {                               │ │ │
│ │ │   "type": "service_account",    │ │ │
│ │ │   "project_id": "...",          │ │ │
│ │ │   ...                           │ │ │
│ │ │ }                               │ │ │
│ │ └─────────────────────────────────┘ │ │
│ └─────────────────────────────────────┘ │
│                                         │
│ ✓ 已验证 project_id: my-gcp-project     │
│                                         │
│ [保存配置]  [测试连接]                   │
└─────────────────────────────────────────┘
```

### 4. 配置示例

#### API Key 模式
```typescript
const provider = {
  id: "vertex-api-key",
  name: "Vertex AI (API Key)",
  settings_config: {
    env: {
      VERTEX_API_KEY: "AIza-your-api-key-here"
    },
    region: "global"
  }
};
```

#### 服务账号模式
```typescript
const provider = {
  id: "vertex-service-account",
  name: "Vertex AI (Service Account)",
  settings_config: {
    env: {
      VERTEX_API_KEY: JSON.stringify({
        type: "service_account",
        project_id: "my-gcp-project",
        private_key_id: "...",
        private_key: "...",
        client_email: "...",
        // ... 其他字段
      })
    },
    region: "us-central1"
  }
};
```

## 测试建议

### 1. API Key 模式测试
```bash
# 使用 Gemini 模型
curl -X POST http://localhost:PORT/v1/models/gemini-pro:generateContent \
  -H "Content-Type: application/json" \
  -d '{"contents":[{"parts":[{"text":"Hello"}]}]}'
```

### 2. 服务账号模式测试
```bash
# 使用 Claude 模型
curl -X POST http://localhost:PORT/v1/messages \
  -H "Content-Type: application/json" \
  -d '{"model":"claude-3-sonnet","messages":[{"role":"user","content":"Hello"}]}'
```

## 注意事项

1. **Region 默认值**: 前端配置时，region 字段默认为 `"global"`
2. **服务账号 JSON**: 必须是完整的 GCP 服务账号 JSON，不能缺少任何必要字段
3. **Token 缓存**: 后端会自动缓存 token，根据过期时间自动刷新，无需前端处理
4. **模型检测**: 后端会根据模型名称自动检测请求模式（Gemini/Claude/OpenSource）
5. **URL 构建**: 后端会根据认证模式（API Key/服务账号）和 region 自动构建正确的 URL

## 相关文档

- [Vertex AI 文档](https://cloud.google.com/vertex-ai/docs)
- [GCP 服务账号](https://cloud.google.com/iam/docs/service-accounts)
- [gcp_auth Rust 库](https://docs.rs/gcp_auth/)
