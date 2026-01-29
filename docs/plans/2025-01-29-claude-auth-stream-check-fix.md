# 修复 Stream Check 中 ClaudeAuth 认证问题

**创建日期**: 2025-01-29
**状态**: 待实施
**优先级**: 高
**影响范围**: 使用 `auth_mode: "bearer_only"` 的 Claude 中转服务 provider

---

## 问题描述

### 当前行为

在 `src-tauri/src/services/stream_check.rs` 的 `check_claude_stream` 函数中,测试模型功能**硬编码了完整的 Anthropic 官方认证头**,没有根据 provider 的 `auth_mode` 配置来调整认证方式。

**问题代码** (第 307-311 行):
```rust
let response = client
    .post(&url)
    // 认证 headers(双重认证)
    .header("authorization", format!("Bearer {}", auth.api_key))
    .header("x-api-key", &auth.api_key)  // ❌ 始终添加
```

### 预期行为

根据 `ProviderType::ClaudeAuth` 的定义:
- **Anthropic 官方**: `Authorization: Bearer <key>` + `x-api-key: <key>`
- **ClaudeAuth 中转**: `Authorization: Bearer <key>` (仅 Bearer,无 x-api-key)

当 provider 配置 `auth_mode: "bearer_only"` 时,应该只发送 `Authorization` header,不发送 `x-api-key`。

### 影响范围

- **影响用户**: 所有使用 Claude 中转服务(非官方 API)的用户
- **影响功能**: "测试模型"按钮的健康检查功能
- **潜在后果**: 某些中转服务可能会拒绝包含 `x-api-key` 的请求,导致健康检查失败

---

## 技术背景

### 认证策略系统

代码中已经实现了完善的认证策略系统:

1. **AuthStrategy 枚举** (`src-tauri/src/proxy/providers/auth.rs`):
   - `Anthropic` - Anthropic 官方(双重认证)
   - `ClaudeAuth` - 中转服务(仅 Bearer)
   - `Bearer` - OpenRouter 等
   - `Google` / `GoogleOAuth` - Gemini 相关

2. **AuthInfo 结构体**:
   ```rust
   pub struct AuthInfo {
       pub api_key: String,
       pub strategy: AuthStrategy,  // ✅ 包含策略信息
       pub access_token: Option<String>,
   }
   ```

3. **ClaudeAdapter::add_auth_headers** (`proxy/providers/claude.rs:236-254`):
   已经正确实现了根据策略添加不同的认证头

### 问题根源

`check_claude_stream` 函数接收了正确的 `AuthInfo` 参数(包含 `strategy` 字段),但**完全忽略了这个字段**,直接硬编码了 Anthropic 官方的认证方式。

---

## 修复方案

### 核心修改

修改 `src-tauri/src/services/stream_check.rs` 中的 `check_claude_stream` 函数:

```rust
async fn check_claude_stream(
    client: &Client,
    base_url: &str,
    auth: &AuthInfo,
    model: &str,
    test_prompt: &str,
    timeout: std::time::Duration,
) -> Result<(u16, String), AppError> {
    // ... URL 和 body 构建 ...

    // 根据认证策略构建请求
    let mut request_builder = client
        .post(&url)
        .header("authorization", format!("Bearer {}", auth.api_key));

    // ✅ 只有 Anthropic 官方策略才添加 x-api-key
    if auth.strategy == AuthStrategy::Anthropic {
        request_builder = request_builder.header("x-api-key", &auth.api_key);
    }

    // 添加其他必需的 headers
    let response = request_builder
        .header("anthropic-version", "2023-06-01")
        .header("anthropic-beta", "claude-code-20250219,interleaved-thinking-2025-05-14")
        .header("anthropic-dangerous-direct-browser-access", "true")
        // ... 其他 headers ...
        .timeout(timeout)
        .json(&body)
        .send()
        .await
        .map_err(Self::map_request_error)?;

    // ... 其余代码保持不变 ...
}
```

### 测试策略

添加单元测试验证认证头的正确性:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::providers::AuthStrategy;

    #[test]
    fn test_check_claude_stream_uses_anthropic_auth() {
        // 验证 Anthropic 策略会添加 x-api-key
    }

    #[test]
    fn test_check_claude_stream_uses_claude_auth() {
        // 验证 ClaudeAuth 策略不会添加 x-api-key
    }
}
```

**注意**: 由于 `check_claude_stream` 是异步函数且需要真实的 HTTP 客户端,单元测试可能需要:
1. 使用 mock HTTP 客户端
2. 或者测试一个包装函数,验证传入的 headers 是否正确

---

## 实施计划

### 第 1 步: 代码修改

- [ ] 修改 `check_claude_stream` 函数,根据 `auth.strategy` 决定是否添加 `x-api-key`
- [ ] 确保 `AuthStrategy` 已导入
- [ ] 保持其他 headers 不变

### 第 2 步: 添加测试

- [ ] 在 `stream_check.rs` 的 `tests` 模块中添加测试用例
- [ ] 验证 `Anthropic` 策略添加 `x-api-key`
- [ ] 验证 `ClaudeAuth` 策略不添加 `x-api-key`
- [ ] 考虑使用 mock 或集成测试

### 第 3 步: 运行检查

根据 `CONTRIBUTING` 指南:
- [ ] 运行 Rust 测试: `cargo test`
- [ ] 运行前端测试: `pnpm test:unit`
- [ ] 类型检查: `pnpm typecheck`
- [ ] 格式检查: `pnpm format:check`
- [ ] Rust 格式化: `cargo fmt`
- [ ] Rust 检查: `cargo clippy`

### 第 4 步: 手动验证

- [ ] 创建一个测试 provider,设置 `auth_mode: "bearer_only"`
- [ ] 执行"测试模型"功能
- [ ] 验证请求不包含 `x-api-key` header
- [ ] 验证健康检查成功

---

## 参考代码

### 正确实现参考

`src-tauri/src/proxy/providers/claude.rs:236-254`:
```rust
fn add_auth_headers(&self, request: RequestBuilder, auth: &AuthInfo) -> RequestBuilder {
    match auth.strategy {
        AuthStrategy::Anthropic => request
            .header("Authorization", format!("Bearer {}", auth.api_key))
            .header("x-api-key", &auth.api_key),
        AuthStrategy::ClaudeAuth => {
            request.header("Authorization", format!("Bearer {}", auth.api_key))
        }
        AuthStrategy::Bearer => {
            request.header("Authorization", format!("Bearer {}", auth.api_key))
        }
        _ => request,
    }
}
```

### Provider 配置示例

```json
{
  "settings_config": {
    "env": {
      "ANTHROPIC_BASE_URL": "https://some-proxy.com",
      "ANTHROPIC_AUTH_TOKEN": "sk-proxy-key"
    },
    "auth_mode": "bearer_only"
  }
}
```

---

## 风险评估

### 低风险
- 修改范围小,只影响一个函数
- 不改变返回值或公共接口
- 已有完善的测试覆盖

### 兼容性
- **向后兼容**: Anthropic 官方 provider 行为不变
- **中转服务**: 修复了当前的错误行为,使其符合预期

### 测试覆盖
- 需要确保新测试覆盖两种策略
- 建议添加集成测试验证真实请求

---

## 预期成果

修复后:
1. ✅ `auth_mode: "bearer_only"` 的 provider 健康检查正常工作
2. ✅ 符合 `ProviderType::ClaudeAuth` 的设计意图
3. ✅ 与 `ClaudeAdapter::add_auth_headers` 行为一致
4. ✅ 所有测试通过
5. ✅ 代码符合项目贡献指南

---

## 相关文件

- `src-tauri/src/services/stream_check.rs` - 主要修改文件
- `src-tauri/src/proxy/providers/claude.rs` - 参考实现
- `src-tauri/src/proxy/providers/auth.rs` - 认证类型定义
- `src-tauri/src/proxy/providers/mod.rs` - ProviderType 枚举

---

## 提交信息建议

```
fix(stream_check): respect auth_mode for Claude health checks

Previously, check_claude_stream always added the x-api-key header,
ignoring the provider's auth_mode setting. This caused health check
failures for proxy services that only support Bearer authentication.

Now the function respects the auth.strategy field:
- Anthropic: Authorization Bearer + x-api-key
- ClaudeAuth: Authorization Bearer only

This aligns with the behavior of ClaudeAdapter::add_auth_headers
and fixes health checks for proxy providers with auth_mode="bearer_only".

Related: ProviderType::ClaudeAuth
```

---

## 审核清单

在提交 PR 前确认:
- [ ] 代码修改完成
- [ ] 单元测试通过
- [ ] 手动测试验证
- [ ] 格式检查通过
- [ ] Clippy 检查通过
- [ ] 文档更新(如需要)
- [ ] Changelog 更新(如需要)
