# 错误处理改进说明

## 改进内容

现在当 API 测试失败时，系统会返回更详细的错误信息，包括：

1. **具体的 HTTP 状态码**（如果有）
2. **详细的错误消息**
3. **最后一个尝试失败的端点信息**

## 错误返回格式

### ProviderTestResult 结构

```rust
{
    "success": false,
    "status": 402,           // ← 现在会返回具体的状态码
    "latency_ms": null,
    "message": "API 测试失败 (状态码: 402)",  // ← 包含状态码的消息
    "detail": "所有测试端点和认证方式都无法访问\n\n最后一个错误:\n端点: https://api.example.com/v1/models - 错误: ...\n\n请检查:\n1. Base URL 是否正确\n2. API Key 配置路径是否正确\n3. 网络连接是否正常\n4. API 服务是否在线"
}
```

## 常见状态码含义

### 2xx - 成功
- **200 OK**: 测试成功

### 4xx - 客户端错误
- **400 Bad Request**: 请求格式不正确（通常表示连接成功但测试请求不完整，这是正常的）
- **401 Unauthorized**: API 密钥无效或过期
- **402 Payment Required**: 需要付费或账户余额不足
- **403 Forbidden**: 访问被拒绝，权限不足
- **404 Not Found**: API 端点不存在，检查 Base URL
- **429 Too Many Requests**: 请求过于频繁，触发限流

### 5xx - 服务器错误
- **500 Internal Server Error**: 服务器内部错误
- **502 Bad Gateway**: 网关错误
- **503 Service Unavailable**: 服务暂时不可用

### 无状态码
如果 `status` 字段为 `null`，表示连接失败（网络问题、DNS 解析失败等）

## 错误排查流程

### 1. 检查状态码

**如果有状态码（status 不为 null）:**
- 401 → 检查 API Key 是否正确
- 402 → 检查账户余额或订阅状态
- 403 → 检查 API Key 权限
- 404 → 检查 Base URL 是否正确
- 429 → 等待一段时间后重试
- 5xx → 服务器问题，等待服务恢复

**如果没有状态码（status 为 null）:**
- 检查网络连接
- 检查 Base URL 格式（必须包含 `https://` 或 `http://`）
- 检查防火墙设置
- 尝试在浏览器中访问 Base URL

### 2. 查看详细错误信息

`detail` 字段包含：
- 最后一个尝试的端点
- 具体的错误原因
- 排查建议

### 3. 检查配置

```json
{
  "testConfig": {
    "apiKeyPath": "env.API_KEY",      // ← API Key 路径是否正确
    "baseUrlPath": "env.BASE_URL",    // ← Base URL 路径是否正确
    "endpoints": ["/v1/models"],      // ← 端点是否正确
    "authType": "bearer"              // ← 认证方式是否匹配
  }
}
```

## 示例

### 示例 1: API Key 无效

**返回:**
```json
{
  "success": false,
  "status": 401,
  "message": "身份验证失败 (401) - API密钥无效或过期",
  "detail": "错误响应体内容..."
}
```

**解决方案:** 更新 API Key

### 示例 2: 余额不足

**返回:**
```json
{
  "success": false,
  "status": 402,
  "message": "API 测试失败 (状态码: 402)",
  "detail": "所有测试端点和认证方式都无法访问\n\n最后一个错误:\n端点: https://api.example.com/v1/models - 错误: HTTP status client error (402 Payment Required)..."
}
```

**解决方案:** 检查账户余额或充值

### 示例 3: 网络连接失败

**返回:**
```json
{
  "success": false,
  "status": null,
  "message": "无法连接到 API 服务",
  "detail": "所有测试端点和认证方式都无法访问\n\n最后一个错误:\n端点: https://api.example.com/v1/models - 错误: error sending request for url..."
}
```

**解决方案:** 检查网络连接和 Base URL

### 示例 4: 端点不存在

**返回:**
```json
{
  "success": false,
  "status": 404,
  "message": "API端点不存在 (404) - 请检查 Base URL 和端点配置",
  "detail": "尝试的端点: https://api.example.com/v1/models"
}
```

**解决方案:** 修正 Base URL 或配置正确的 endpoints

## 调试技巧

1. **查看日志**: 启用调试日志查看详细的测试过程
2. **逐步测试**: 先用 `authType: "auto"` 让系统尝试各种认证方式
3. **使用浏览器**: 在浏览器中访问 Base URL，确认服务可达
4. **检查响应体**: `detail` 字段可能包含服务器返回的具体错误信息
5. **咨询提供商**: 如果持续失败，联系 API 提供商确认服务状态

## 技术细节

### 错误捕获机制

系统会：
1. 尝试所有配置的端点和认证方式组合
2. 记录每次尝试的结果
3. 优先返回有 HTTP 响应的错误（即使是错误状态码）
4. 如果都是网络错误，返回最后一个错误
5. 提取并返回 HTTP 状态码（如果有）

### 状态码优先级

当有多个错误时，系统会选择最有价值的错误返回：
1. 400 - 通常最有诊断价值（说明连接成功）
2. 其他 4xx - 客户端错误
3. 5xx - 服务器错误
4. 网络错误（无状态码）
