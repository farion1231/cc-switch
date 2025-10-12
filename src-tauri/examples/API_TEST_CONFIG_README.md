# 通用 API 测试配置指南

## 概述

现在 cc-switch 支持测试任意 API 提供商！通过在供应商配置中添加 `testConfig` 字段，您可以自定义测试行为。

## 快速开始

### 基础配置示例

```json
{
  "id": "my-custom-provider",
  "name": "我的自定义供应商",
  "settingsConfig": {
    "env": {
      "API_KEY": "your-api-key-here",
      "BASE_URL": "https://api.example.com"
    },
    "testConfig": {
      "apiKeyPath": "env.API_KEY",
      "baseUrlPath": "env.BASE_URL",
      "authType": "bearer",
      "endpoints": ["/v1/models", "/v1/chat/completions"]
    }
  }
}
```

## 配置字段说明

### testConfig 字段

| 字段 | 类型 | 必需 | 默认值 | 说明 |
|------|------|------|--------|------|
| `apiKeyPath` | string | 否 | - | API Key 在配置中的路径，如 `"env.API_KEY"` |
| `baseUrlPath` | string | 否 | - | Base URL 在配置中的路径，如 `"env.BASE_URL"` |
| `authType` | string | 否 | `"auto"` | 认证类型 |
| `authHeader` | string | 否 | - | 自定义认证 Header 名称（authType 为 `"custom"` 时使用） |
| `authPrefix` | string | 否 | - | 认证值前缀，如 `"Bearer "` |
| `endpoints` | array | 否 | 默认端点 | 要测试的端点列表 |
| `httpMethod` | string | 否 | `"HEAD"` | HTTP 方法 |
| `customHeaders` | object | 否 | - | 自定义额外的 HTTP Headers |

### authType 选项

| 值 | 说明 | 示例 Header |
|----|------|-------------|
| `"auto"` | 自动尝试多种认证方式（推荐） | 多种组合 |
| `"bearer"` | Bearer Token 认证 | `Authorization: Bearer <token>` |
| `"api-key"` | API Key Header | `api-key: <key>` |
| `"x-api-key"` | X-API-Key Header | `x-api-key: <key>` |
| `"custom"` | 自定义 Header | 使用 `authHeader` 指定 |

### 配置路径格式

路径使用点号分隔，支持多层嵌套：
- `"env.API_KEY"` → `settingsConfig.env.API_KEY`
- `"auth.OPENAI_API_KEY"` → `settingsConfig.auth.OPENAI_API_KEY`
- `"credentials.token"` → `settingsConfig.credentials.token`

## 常见用例

### 1. OpenAI 兼容 API

```json
{
  "settingsConfig": {
    "env": {
      "OPENAI_API_KEY": "sk-...",
      "OPENAI_BASE_URL": "https://api.openai.com"
    },
    "testConfig": {
      "apiKeyPath": "env.OPENAI_API_KEY",
      "baseUrlPath": "env.OPENAI_BASE_URL",
      "authType": "bearer",
      "endpoints": ["/v1/models"]
    }
  }
}
```

### 2. 88code 等特殊代理

```json
{
  "settingsConfig": {
    "env": {
      "ANTHROPIC_AUTH_TOKEN": "your-key",
      "ANTHROPIC_BASE_URL": "https://api.88code.com"
    },
    "testConfig": {
      "apiKeyPath": "env.ANTHROPIC_AUTH_TOKEN",
      "baseUrlPath": "env.ANTHROPIC_BASE_URL",
      "authType": "auto",
      "customHeaders": {
        "user-agent": "Claude-Code/1.0",
        "x-client-name": "claude-code"
      }
    }
  }
}
```

### 3. 自定义 API Key Header

```json
{
  "settingsConfig": {
    "auth": {
      "MY_KEY": "custom-key"
    },
    "config": {
      "url": "https://api.custom.com"
    },
    "testConfig": {
      "apiKeyPath": "auth.MY_KEY",
      "baseUrlPath": "config.url",
      "authType": "custom",
      "authHeader": "X-Custom-Api-Key",
      "endpoints": ["/api/test"]
    }
  }
}
```

### 4. Azure OpenAI

```json
{
  "settingsConfig": {
    "azure": {
      "api_key": "your-azure-key",
      "endpoint": "https://your-resource.openai.azure.com"
    },
    "testConfig": {
      "apiKeyPath": "azure.api_key",
      "baseUrlPath": "azure.endpoint",
      "authType": "custom",
      "authHeader": "api-key",
      "endpoints": ["/openai/deployments?api-version=2023-05-15"]
    }
  }
}
```

### 5. 最小配置（使用默认值）

```json
{
  "settingsConfig": {
    "env": {
      "API_KEY": "your-key",
      "BASE_URL": "https://api.example.com"
    },
    "testConfig": {
      "apiKeyPath": "env.API_KEY",
      "baseUrlPath": "env.BASE_URL"
    }
  }
}
```

## 向后兼容性

如果不配置 `testConfig`，系统会根据 `app_type`（Claude/Codex）自动使用内置的测试逻辑：
- **Claude**: 测试 Anthropic API 端点，支持多种认证方式
- **Codex**: 测试 OpenAI 兼容端点

## 调试技巧

1. **查看日志**: 测试过程中会输出详细的日志信息
2. **使用 auto 认证**: 如果不确定认证方式，使用 `"authType": "auto"`
3. **测试多个端点**: 配置多个端点以增加成功率
4. **自定义 Headers**: 某些 API 需要特定的客户端标识

## 故障排查

### 问题：测试一直失败
- 检查 `apiKeyPath` 和 `baseUrlPath` 是否正确
- 确认 API Key 有效且未过期
- 验证 Base URL 格式正确（包含协议，不以 `/` 结尾）
- 尝试使用 `"authType": "auto"`

### 问题：401 错误
- 检查 API Key 是否正确
- 确认 `authType` 与 API 要求的认证方式匹配

### 问题：404 错误
- 检查 `endpoints` 配置是否正确
- 验证 Base URL 是否包含了不应该包含的路径

## 更多示例

查看 `api-test-config-examples.json` 文件获取更多配置示例。
