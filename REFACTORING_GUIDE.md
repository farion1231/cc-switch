# CC-Switch Refactoring Guide for II-Agent Integration

## Based on GitHub Issues Analysis

This document synthesizes user requests from CC-Switch GitHub issues to guide the II-Agent integration refactoring.

---

## Key User Pain Points (from Issues)

### 1. OpenClaw Support Requests

**Issues**: #900, #1029, #1078

#### User Requests:
- #900: "cc switch 实在太好用，一处地方可以管理多个 app 配置。希望可以支持 openclaw"
- #1078: "希望增加针对 OpenClaw 的 api 管理和对暂时无用的 API 进行冻结的功能"

#### What Users Want:
1. **Centralized API Management** - Manage OpenClaw providers from CC-Switch UI
2. **API Freezing** - Temporarily disable unused APIs without deleting
3. **Same UX as Claude/Codex/Gemini** - Consistent provider management

#### Refactoring Implications for II-Agent:
```typescript
// II-Agent should support similar provider management
interface IIAgentProvider {
  id: string
  name: string
  baseUrl: string
  apiKey: string
  frozen?: boolean  // ← New: API freezing
  models: string[]
}
```

---

### 2. OpenCode Configuration Issues

**Issues**: #875, #876, #909, #864, #895, #896, #940

#### Critical Issues:
- #875 (Enhancement): "现有的 opencode 配置管理很不好用"
  - Model selection after fetching
  - Oh-my-opencode multi-config management
  - Plugin configuration
  - Primary model configuration
  
- #876: "希望添加支持外部导入 opencode 配置"
- #909: "支持将 opencode serve 作为模型供应商么"
- #895: "opencode 添加英伟达供应商失败"
- #940: "添加 mcp 服务后，cc 无法修改 opencode 配置文件"

#### What Users Want:
1. **Better Model Picker** - Select specific models from provider catalog
2. **External Config Import** - Import existing opencode.json
3. **Oh-My-Opencode Support** - Multi-config switching
4. **OpenCode Serve as Provider** - Use local opencode instance
5. **NVIDIA Provider Support** - Add NVIDIA API integration
6. **MCP Compatibility** - Don't break opencode config when adding MCP

#### What We Built (Matches User Requests):
✅ **Model Picker Catalog** - 5 curated models per provider
✅ **External Import** - `import_opencode_providers_from_live()` command
✅ **-cc Suffix Providers** - CC-Switch managed configs
✅ **Tauri Commands** - `get_opencode_model_picker`, `set_opencode_provider_models`

#### Refactoring Implications for II-Agent:
```python
# II-Agent should implement similar model picker
from ii_agent.llm.proxy.model_picker import get_model_picker

picker = get_model_picker("openrouter")
# Returns exactly 5 curated models
```

---

### 3. Proxy & Failover Issues

**Issues**: #1085, #1100, #961, #1069

#### Critical Issues:
- #1085: "代理模式支持按模型家族（Haiku/Sonnet/Opus）配置其他模型厂商独立端点和密钥"
  - Haiku requests → cheap provider A
  - Sonnet/Opus requests → capable provider B
  - Different providers per project
  
- #1100: "开启代理的时候，无法使用 HTTP_PROXY"
  - Proxy mode conflicts with HTTP_PROXY env var
  - 503 errors when both enabled
  
- #961: "为故障转移状态下增加供应商并发限制的设置"
  - Rate limit issues with shared API keys
  - Need per-provider concurrency limits
  
- #1069: "cc-switch 代理实现的响应头处理问题"

#### What Users Want:
1. **Model Family Routing** - Different providers for Haiku/Sonnet/Opus
2. **HTTP_PROXY Compatibility** - Don't break when system proxy enabled
3. **Concurrency Limits** - Prevent rate limiting on shared keys
4. **Better Response Header Handling** - Fix proxy response issues

#### Refactoring Implications for II-Agent:
```python
# II-Agent proxy should support model family routing
class IIAgentProxy:
    async def route_request(self, model: str, request: dict):
        family = self.detect_model_family(model)  # Haiku/Sonnet/Opus
        
        # Route to family-specific provider
        if family == "haiku":
            provider = self.config.haiku_provider
        elif family == "sonnet":
            provider = self.config.sonnet_provider
        else:
            provider = self.config.opus_provider
        
        # Apply concurrency limit
        await provider.semaphore.acquire()
        try:
            return await provider.forward(request)
        finally:
            provider.semaphore.release()
```

---

### 4. Multi-API Key Management

**Issue**: #1006

#### User Request:
- #1006: "目前一个链接只能配一个 api key，管理不便。希望大佬能增加一下多 api 的管理和测试"

#### What Users Want:
1. **Multiple API Keys per Provider** - Rotate between keys
2. **Key Testing** - Validate keys before use
3. **Automatic Rotation** - Switch keys on rate limit

#### Refactoring Implications for II-Agent:
```python
# II-Agent should support multiple API keys
class IIAgentProvider:
    def __init__(self, config: dict):
        self.api_keys = config.get("apiKeys", [])  # ← List of keys
        self.current_key_index = 0
    
    def get_next_key(self) -> str:
        # Round-robin rotation
        key = self.api_keys[self.current_key_index]
        self.current_key_index = (self.current_key_index + 1) % len(self.api_keys)
        return key
    
    async def test_keys(self) -> list:
        # Test all keys and return valid ones
        valid_keys = []
        for key in self.api_keys:
            if await self.test_key(key):
                valid_keys.append(key)
        return valid_keys
```

---

### 5. Model Management Issues

**Issues**: #1075, #984, #985, #1077

#### Critical Issues:
- #1075: "Some models don't seem to work in Claude Code on my Mac"
- #984: "There's an issue with the selected model (claude-sonnet-4-5-20250929)"
- #985: "建议添加 CLAUDE_CODE_SUBAGENT_MODEL"
- #1077: "也许加入自动获取目前可用模型是个好点子"

#### What Users Want:
1. **Model Validation** - Check if model exists before using
2. **Auto-Discovery** - Fetch available models from provider
3. **Subagent Model Support** - Separate model for subagent tasks
4. **Model Compatibility Checking** - Verify model works with target app

#### What We Built (Matches User Requests):
✅ **Model Picker** - Pre-validated 5 models per provider
✅ **Model Catalog** - Known working models with metadata
✅ **Tauri Commands** - `get_opencode_model_picker` for discovery

#### Refactoring Implications for II-Agent:
```python
# II-Agent model validation
from ii_agent.llm.proxy.model_picker import get_model_picker, get_models_array

class IIAgentModelManager:
    def __init__(self):
        self.model_cache = {}  # Cache provider models
    
    async def get_available_models(self, provider_id: str) -> list:
        # Try model picker first (curated list)
        picker = get_model_picker(provider_id)
        if picker:
            return picker.models
        
        # Fall back to provider's models endpoint
        if provider_id not in self.model_cache:
            models = await self.fetch_from_provider(provider_id)
            self.model_cache[provider_id] = models
        
        return self.model_cache[provider_id]
    
    def validate_model(self, provider_id: str, model_id: str) -> bool:
        # Check if model is in curated list or cache
        models = self.get_available_models(provider_id)
        return any(m.id == model_id for m in models)
```

---

## II-Agent Integration Architecture

### Recommended Design (Based on Issues)

```
┌─────────────────────────────────────────────────────────┐
│                    II-Agent Frontend                     │
│  ┌─────────────┐  ┌──────────────┐  ┌────────────────┐  │
│  │   Provider  │  │ Model Picker │  │  API Key Mgmt  │  │
│  │   Manager   │  │   (5 models) │  │  (Multi-key)   │  │
│  └─────────────┘  └──────────────┘  └────────────────┘  │
└────────────────────────┬────────────────────────────────┘
                         │ Tauri Commands
┌────────────────────────▼────────────────────────────────┐
│                   II-Agent Backend                       │
│  ┌─────────────┐  ┌──────────────┐  ┌────────────────┐  │
│  │ Model Picker│  │  Provider    │  │  Concurrency   │  │
│  │   Catalog   │  │   Router     │  │  Limiter       │  │
│  └─────────────┘  └──────────────┘  └────────────────┘  │
│  ┌─────────────┐  ┌──────────────┐  ┌────────────────┐  │
│  │  API Key    │  │  Model       │  │  Health        │  │
│  │  Rotator    │  │  Validator   │  │  Checker       │  │
│  └─────────────┘  └──────────────┘  └────────────────┘  │
└────────────────────────┬────────────────────────────────┘
                         │
┌────────────────────────▼────────────────────────────────┐
│              CC-Switch Proxy (Optional)                  │
│  - Failover routing                                      │
│  - Model family routing (Haiku/Sonnet/Opus)             │
│  - HTTP_PROXY compatibility                              │
│  - Response header normalization                         │
└──────────────────────────────────────────────────────────┘
```

---

## Implementation Priority (Based on Issue Frequency)

### Phase 1: Core Provider Management (Highest Demand)

**Issues Addressed**: #900, #1006, #875, #876

**Tasks**:
1. ✅ Copy II-Agent presets to CC-Switch (already exists)
2. ⏳ Implement multi-API key support
3. ⏳ Add external config import (opencode.json)
4. ⏳ Build model picker UI component

**Estimated Effort**: 8-12 hours

---

### Phase 2: Model Management (User Confusion)

**Issues Addressed**: #1075, #984, #985, #1077

**Tasks**:
1. ✅ Model picker catalog (already built for OpenCode)
2. ⏳ Model validation API
3. ⏳ Auto-discovery from provider endpoints
4. ⏳ Subagent model support

**Estimated Effort**: 6-10 hours

---

### Phase 3: Proxy Enhancements (Advanced Users)

**Issues Addressed**: #1085, #1100, #961, #1069

**Tasks**:
1. ⏳ Model family routing (Haiku/Sonnet/Opus)
2. ⏳ HTTP_PROXY compatibility fix
3. ⏳ Per-provider concurrency limits
4. ⏳ Response header normalization

**Estimated Effort**: 12-20 hours

---

### Phase 4: OpenClaw Integration (Feature Request)

**Issues Addressed**: #900, #1029, #1078

**Tasks**:
1. ⏳ OpenClaw provider format support
2. ⏳ API freezing mechanism
3. ⏳ OpenClaw config import/export
4. ⏳ Test with OpenClaw Gateway

**Estimated Effort**: 16-24 hours

---

## What NOT to Do (Based on Issues)

### 1. Don't Break Existing Configs

**Issue**: #1088 "cc-switch 导致 codex 的 MCP、模型等设置直接丢失无法找回"

**Lesson**: 
- Always backup before modifying configs
- Import don't overwrite - merge with existing
- Provide rollback mechanism

### 2. Don't Require Config File Editing

**Issue**: #875 "现有的 opencode 配置管理很不好用"

**Lesson**:
- Provide UI for all config operations
- No manual JSON editing
- Visual model picker, not text input

### 3. Don't Ignore HTTP_PROXY

**Issue**: #1100 "开启代理的时候，无法使用 HTTP_PROXY"

**Lesson**:
- Respect system proxy settings
- Don't override HTTP_PROXY unless explicitly configured
- Test with common proxy tools (v2ray, Clash, etc.)

### 4. Don't Lock Users into Single API Key

**Issue**: #1006 "目前一个链接只能配一个 api key，管理不便"

**Lesson**:
- Support multiple API keys per provider
- Allow key rotation
- Provide key testing UI

---

## II-Agent Specific Recommendations

### 1. Use Existing Model Picker Spec

The II-Agent cc-switch-preset already defines model picker with exactly 5 models:

```python
# From ii-agent/cc-switch-preset/MODEL_PICKER.md
from ii_agent.llm.proxy.model_picker import get_model_picker

picker = get_model_picker("openrouter")
# Returns:
# - provider_id: "openrouter"
# - provider_name: "OpenRouter"
# - default_model: "anthropic/claude-3.5-sonnet"
# - models: [5 ModelInfo objects]
```

**Action**: Don't reinvent - use this existing spec.

---

### 2. Automatic Trait Derivation

II-Agent presets use automatic trait derivation from `transport.formats`:

```typescript
// formats: ["anthropic"] → claude, opencode, openclaw
// formats: ["openai_chat", "openai_responses"] → codex, opencode, openclaw
// formats: ["anthropic", "openai_chat"] → ALL apps (universal)
```

**Action**: Copy this pattern - no manual trait configuration needed.

---

### 3. Provider Presets (11 Ready)

II-Agent already has 11 provider presets:

| Preset | Formats | Universal |
|--------|---------|-----------|
| Anthropic | `["anthropic"]` | ❌ |
| OpenAI | `["openai_chat", "openai_responses"]` | ❌ |
| DeepSeek | `["anthropic", "openai_chat"]` | ✅ |
| Zhipu GLM | `["anthropic", "openai_chat"]` | ✅ |
| Bailian | `["anthropic", "openai_chat"]` | ✅ |
| Kimi | `["anthropic", "openai_chat"]` | ✅ |
| MiniMax | `["anthropic", "openai_chat"]` | ✅ |
| Custom Anthropic | `["anthropic", "openai_chat"]` | ✅ |
| Custom OpenAI | `["openai_chat", "openai_responses"]` | ❌ |

**Action**: Copy these presets to II-Agent integration.

---

## Testing Checklist (Before PR)

### Provider Management
- [ ] Add provider with multiple API keys
- [ ] Test API key rotation
- [ ] Import external opencode.json config
- [ ] Freeze/unfreeze provider

### Model Picker
- [ ] Display exactly 5 models per provider
- [ ] Model validation before selection
- [ ] Auto-discovery from provider endpoint
- [ ] Subagent model configuration

### Proxy (If Implemented)
- [ ] Model family routing (Haiku/Sonnet/Opus)
- [ ] HTTP_PROXY compatibility
- [ ] Per-provider concurrency limits
- [ ] Response header handling

### OpenClaw (If Implemented)
- [ ] Provider config import
- [ ] API freezing
- [ ] Config export
- [ ] Gateway integration test

---

## References

### GitHub Issues:
- #900: OpenClaw support request
- #1078: OpenClaw API management + freezing
- #875: OpenCode config management issues
- #876: External OpenCode config import
- #1085: Model family routing in proxy mode
- #1100: HTTP_PROXY conflict with proxy
- #1006: Multi-API key management
- #1077: Auto-discover available models

### II-Agent Files:
- `cc-switch-preset/iiAgentProviderPresets.ts`
- `cc-switch-preset/INTEGRATION.md`
- `cc-switch-preset/MODEL_PICKER.md`
- `src/ii_agent/llm/proxy/model_picker.py`

### CC-Switch Files:
- `src/opencode_config.rs` - Model picker catalog
- `src/proxy/providers/opencode.rs` - OpenCode adapter
- `OPENCODE_INTEGRATION_PLAN.md` - Integration design
- `REFACTORING_ANALYSIS.md` - Project analysis

---

## Conclusion

**Key Insight from Issues**: Users want **simpler configuration management**, **better model selection**, and **more flexible provider routing**.

**II-Agent Integration Should**:
1. ✅ Use existing model picker spec (5 models per provider)
2. ✅ Support multi-API key management
3. ✅ Import external configs (don't overwrite)
4. ✅ Respect HTTP_PROXY settings
5. ✅ Provide visual UI (no manual JSON editing)
6. ✅ Support model family routing (Haiku/Sonnet/Opus)
7. ✅ Add concurrency limits for shared keys

**What Makes II-Agent Different**:
- Python backend (vs CC-Switch Rust)
- Tauri frontend (same as CC-Switch)
- Model proxy API (REST endpoints)
- Focus on agent orchestration (not just config management)

**Integration Strategy**:
- Borrow CC-Switch's provider adapter pattern
- Use II-Agent's model picker catalog
- Share endpoint annotations
- Keep projects separate but compatible
