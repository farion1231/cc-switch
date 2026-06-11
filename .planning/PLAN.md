# PLAN.md — CC Switch REST API Provider Switching

**Phase**: 1
**Goal**: 添加 REST API 端点，支持局域网内切换 Provider
**Mode**: mvp

---

## Context

CC Switch 已有代理服务器（Axum + 端口 18792），需要复用该端口添加 Provider 切换 API。

### 架构约束
- 代理服务器已使用 Axum 0.7
- Provider 切换逻辑在 `ProviderService::switch()`
- 支持多 App 类型：claude, codex, gemini, opencode, openclaw, hermes
- 需复用现有 `AppState` 和数据库连接

### 安全要求
- 默认只允许本地调用
- 可配置 API Key 验证
- 需记录切换日志

---

## Implementation Plan

### Task 1: 添加 API Handler

**File**: `src-tauri/src/proxy/handlers/switch.rs` (new)

```rust
// POST /api/provider/switch
// Body: { "app": "claude", "providerId": "xxx" }
// Response: { "success": true, "providerId": "xxx", "providerName": "xxx" }
```

**Tasks**:
- [ ] 创建 `switch_provider_handler()` 函数
- [ ] 解析请求体 `{app, providerId}`
- [ ] 调用 `ProviderService::switch()`
- [ ] 返回切换结果或错误

### Task 2: 注册路由

**File**: `src-tauri/src/proxy/server.rs`

**Changes**:
- [ ] 添加路由: `.route("/api/provider/switch", post(handlers::switch_provider))`
- [ ] 可选: 添加 `.route("/api/providers", get(handlers::list_providers))`

### Task 3: 添加配置项

**File**: `src-tauri/src/proxy/config.rs` (or `ProxyConfig` struct)

**New fields**:
- [ ] `api_enabled: bool` — 是否启用 REST API
- [ ] `api_key: Option<String>` — API Key 验证（可选）
- [ ] `api_listen_localhost_only: bool` — 是否只监听 localhost（默认 true）

### Task 4: 集成 ProviderService

**File**: `src-tauri/src/services/provider.rs`

**Verification**:
- [ ] `ProviderService::switch()` 可从 handler 调用
- [ ] 错误类型可转换为 HTTP 错误

### Task 5: 测试

**File**: `src-tauri/tests/deeplink_import.rs` (or new)

- [ ] 测试切换 Provider 端点
- [ ] 测试无效 providerId 返回 404
- [ ] 测试无效 app 类型返回 400

---

## API Specification

### POST /api/provider/switch

**Request**:
```json
{
  "app": "claude",
  "providerId": "minimax-1234567890"
}
```

**Success Response (200)**:
```json
{
  "success": true,
  "app": "claude",
  "providerId": "minimax-1234567890",
  "providerName": "MiniMax"
}
```

**Error Response (400/404)**:
```json
{
  "success": false,
  "error": "Provider not found: xxx"
}
```

### GET /api/providers?app=claude (optional MVP scope)

**Response**:
```json
{
  "providers": [
    { "id": "xxx", "name": "MiniMax", "enabled": true },
    { "id": "yyy", "name": "Anthropic", "enabled": false }
  ]
}
```

---

## Verification

```bash
# 测试切换
curl -X POST http://localhost:18792/api/provider/switch \
  -H "Content-Type: application/json" \
  -d '{"app":"claude","providerId":"xxx"}'

# 预期: {"success":true,"providerId":"xxx","providerName":"xxx"}
```

---

## Dependencies
- Task 1 → Task 2 → Task 4
- Task 3 可与 Task 1 并行

## Duration
- Task 1: 30 min
- Task 2: 10 min
- Task 3: 20 min
- Task 4: 10 min
- Task 5: 20 min
- **Total: ~90 min**

---

## Out of Scope (v1)
- API Key 验证
- HTTPS 支持
- Provider CRUD API
- WebSocket 支持
