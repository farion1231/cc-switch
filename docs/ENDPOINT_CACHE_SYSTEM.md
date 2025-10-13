# 智能端点发现和缓存系统

## 概述

为了解决API测试接口千变万化的问题,我们实现了一个**智能端点发现和缓存系统**。该系统能够:

1. **自动记住成功的端点** - 测试成功后自动缓存端点信息
2. **优先使用缓存** - 下次测试时优先尝试已知成功的端点
3. **动态适应变化** - 如果缓存失效,自动回退到智能探测模式
4. **持久化存储** - 缓存数据保存到本地文件,重启应用后仍然有效

## 核心特性

### 1. 智能端点探测

系统会自动尝试多种常见的API端点路径组合:

```
/v1/models
/v1/messages
/v1/chat/completions
/models
/messages
/chat/completions
/api/v1/models
/api/v1/messages
/api/models
/v1
/api
/
```

### 2. 成功端点缓存

当测试成功时,系统会记录:

- **端点URL** - 完整的成功端点地址
- **认证方式** - 使用的认证变体索引
- **HTTP方法** - GET/POST/HEAD
- **性能指标** - 平均延迟、成功次数
- **时间戳** - 最后成功时间

### 3. 优先级测试策略

测试顺序:

1. **缓存的成功端点** (最高优先级)
2. **标准端点路径** (按常见程度排序)
3. **域名+路径组合** (自动处理base_url中的路径)

### 4. 自动缓存管理

- **自动清理** - 30天未使用的缓存自动过期
- **统计信息** - 记录成功次数和平均延迟
- **持久化** - 保存到 `~/.config/cc-switch/endpoint_cache.json`

## 使用示例

### 场景1: 首次测试新供应商

```
1. 用户添加新供应商 "provider-a"
2. 系统尝试所有可能的端点组合
3. 发现 https://api.example.com/v1/models 成功
4. 自动缓存这个端点信息
```

### 场景2: 再次测试同一供应商

```
1. 用户再次测试 "provider-a"
2. 系统首先尝试缓存的端点 https://api.example.com/v1/models
3. 测试成功,立即返回 (节省时间)
4. 更新缓存统计信息 (成功次数+1, 更新平均延迟)
```

### 场景3: 供应商更改了API端点

```
1. 用户测试 "provider-a"
2. 系统尝试缓存的端点失败
3. 自动回退到智能探测模式
4. 发现新的成功端点 https://api.example.com/api/v1/models
5. 更新缓存为新端点
```

## 缓存文件格式

缓存文件位置: `~/.config/cc-switch/endpoint_cache.json`

```json
{
  "cache": {
    "provider-id-1": {
      "https://api.example.com": {
        "endpoint": "https://api.example.com/v1/models",
        "auth_variant_index": 0,
        "http_method": "HEAD",
        "last_success_timestamp": 1697000000,
        "success_count": 15,
        "avg_latency_ms": 145
      }
    },
    "provider-id-2": {
      "https://api.another.com": {
        "endpoint": "https://api.another.com/api/v1/messages",
        "auth_variant_index": 1,
        "http_method": "GET",
        "last_success_timestamp": 1697000100,
        "success_count": 8,
        "avg_latency_ms": 220
      }
    }
  }
}
```

## 技术实现

### 核心模块

1. **endpoint_cache.rs** - 缓存管理模块
   - `EndpointCache` - 缓存管理器
   - `EndpointCacheEntry` - 缓存条目
   - 持久化存储和加载

2. **test_provider.rs** - 测试逻辑增强
   - `build_smart_test_endpoints()` - 智能端点生成
   - 成功时自动记录缓存
   - 优先使用缓存端点

### 关键函数

```rust
// 智能生成测试端点(优先使用缓存)
fn build_smart_test_endpoints(
    base_url: &str, 
    provider_id: Option<&str>
) -> Vec<String>

// 记录成功的端点
cache.record_success(
    provider_id,
    base_url,
    endpoint,
    auth_variant_index,
    http_method,
    latency
)

// 获取缓存的端点
cache.get_cached_endpoint(provider_id, base_url)
```

## 性能优化

### 测试速度提升

- **首次测试**: 可能需要尝试多个端点 (~2-5秒)
- **后续测试**: 直接使用缓存端点 (~0.1-0.5秒)
- **提升幅度**: 4-50倍速度提升

### 网络请求减少

- **无缓存**: 可能需要10-20个请求
- **有缓存**: 通常只需1-2个请求
- **节省**: 80-95%的网络请求

## 最佳实践

### 1. 定期清理缓存

系统会自动清理30天未使用的缓存,无需手动干预。

### 2. 监控缓存统计

可以通过日志查看缓存使用情况:

```
[INFO] 使用缓存的成功端点: https://api.example.com/v1/models (成功15次, 平均延迟145ms)
```

### 3. 处理缓存失效

如果供应商更改了API端点:
- 系统会自动检测失败
- 回退到智能探测模式
- 自动更新缓存

### 4. 手动清除缓存

如需手动清除缓存,删除文件即可:

```bash
rm ~/.config/cc-switch/endpoint_cache.json
```

## 故障排查

### 问题1: 缓存端点失效

**症状**: 测试失败,但之前成功过

**原因**: 
- 供应商更改了API端点
- API密钥权限变更
- 网络环境变化

**解决**: 
- 系统会自动回退到探测模式
- 如果持续失败,检查API配置

### 问题2: 缓存文件损坏

**症状**: 启动时出现缓存加载警告

**原因**: 
- 文件格式错误
- 磁盘空间不足
- 权限问题

**解决**: 
- 删除缓存文件,系统会自动重建
- 检查文件权限和磁盘空间

### 问题3: 缓存不生效

**症状**: 每次都进行完整探测

**原因**: 
- provider_id 不匹配
- base_url 发生变化
- 缓存已过期

**解决**: 
- 检查日志确认原因
- 确保provider配置稳定

## 未来扩展

### 可能的增强功能

1. **智能学习** - 根据历史数据预测最佳端点
2. **共享缓存** - 团队共享成功端点配置
3. **健康检查** - 定期验证缓存端点有效性
4. **性能分析** - 详细的端点性能报告
5. **A/B测试** - 自动比较不同端点性能

### 配置选项 (未来)

```json
{
  "endpoint_cache": {
    "enabled": true,
    "max_age_days": 30,
    "auto_cleanup": true,
    "cache_path": "~/.config/cc-switch/endpoint_cache.json"
  }
}
```

## 总结

智能端点发现和缓存系统通过以下方式解决了"接口千变万化"的问题:

1. ✅ **自动适应** - 无需手动配置,自动发现正确端点
2. ✅ **性能优化** - 缓存成功端点,大幅提升测试速度
3. ✅ **容错能力** - 缓存失效时自动回退到探测模式
4. ✅ **持久化** - 重启应用后仍然有效
5. ✅ **零配置** - 完全自动化,用户无感知

这个系统让用户不再需要担心API端点的变化,系统会自动学习和适应。
