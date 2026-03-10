# II-Agent Upstream Issues Analysis

## GitHub Issues Summary

### Open Issues (4 total)

| # | Title | Date | Relevance |
|---|-------|------|-----------|
| 181 | Performance Optimization - Agent Execution Caching | 2026-02-20 | Low |
| 171 | Gemini API Error: Pydantic V2 Schema Validation | 2025-12-25 | Medium |
| 168 | Sandbox error while trying agent mode | 2025-12-07 | Low |
| 165 | Python 3.14 Compatibility: fastmcp incompatible | 2025-11-19 | Low |

### Closed Issues (Proxy/Models Related)

| # | Title | Status | Date | Summary |
|---|-------|--------|------|---------|
| 155 | [Feature Request] Option to switch settings (esp model) in-between session | ✅ Closed | 2025-11-19 | **HIGHLY RELEVANT** - Request for mid-session model switching |
| 134 | Models like Kimi-K2 can output both tool_calls and content | ✅ Closed | 2025-07-31 | Model capability issue |
| 59 | Is it possible to add support for the ollama qwen model? | ✅ Closed | 2025-11-19 | Model support request |
| 35 | Is it possible to add support for the Gemini model? | ✅ Closed | 2025-06-02 | Model support request |

---

## Key Issue: #155 - Mid-Session Model Switching

**Title**: "[Feature Request] Option to switch the settings (esp model) in-between the session."

**Author**: gauravdhiman

**Status**: CLOSED (but no details on resolution)

**Request**:
> Currently, settings must be configured before the session starts. Once a session is created and active, there's no way to modify them. It would be helpful to allow users to change the model mid-session. This would allow them to switch models in case of rate limits or context window issues, and to try different models within the same session. The same applies to other settings as well.

**Use Cases Identified**:
1. **Rate Limit Handling** - Switch to different model when hitting rate limits
2. **Context Window Issues** - Switch to model with larger context
3. **Model Experimentation** - Try different models within same session

**Current II-Agent Implementation**:
From `src/ii_agent/llm/proxy/manager.py`:
```python
def switch_provider(self, provider_id: str, app_id: str) -> Tuple[bool, str]:
    """Switch to a different provider for an app."""
    provider = self.config.get(provider_id)
    if not provider:
        return False, f"Provider {provider_id} does not exist"
    
    success = self.config.set_current(app_id, provider_id)
    if success:
        return True, f"Switched to {provider.name}"
    return False, "Failed to switch provider"
```

**Gap**: This switches provider at app level, but doesn't appear to support switching mid-session for active conversations.

---

## II-Agent Proxy Architecture Analysis

### Module Structure

```
src/ii_agent/llm/proxy/
├── __init__.py         # Module exports
├── types.py            # Type definitions
├── config.py           # Provider configuration storage
├── factory.py          # ModelProxyFactory for creating LLM clients
├── manager.py          # ProviderManager for CRUD operations
├── enumerator.py       # Model enumeration utilities
├── model_picker.py     # Predefined 5-model catalogs
└── README.md           # Documentation
```

### Key Components

#### 1. ProviderManager (manager.py)

**Features**:
- ✅ Provider CRUD operations
- ✅ Provider switching per app
- ✅ Model enumeration from provider APIs
- ✅ Sort order management
- ✅ Default provider import

**Missing** (compared to CC-Switch):
- ❌ No failover mechanism
- ❌ No health checking
- ❌ No circuit breaker
- ❌ No concurrent request limiting
- ❌ No key rotation

#### 2. ModelProxyFactory (factory.py)

**Features**:
- ✅ Creates LLM clients from provider configs
- ✅ Supports OpenAI, Anthropic, Gemini APIs
- ✅ Proxy configuration per provider
- ✅ Configuration overrides

**Proxy Support**:
```python
# From types.py
class ProviderProxyConfig(BaseModel):
    enabled: bool
    proxy_type: Literal["http", "https", "socks5"]
    proxy_host: str
    proxy_port: int
    proxy_username: Optional[str]
    proxy_password: Optional[str]
```

**Gap**: Per-provider proxy config exists but no integration with HTTP_PROXY env var handling.

#### 3. Model Picker (model_picker.py)

**Features**:
- ✅ Exactly 5 models per provider (matches CC-Switch)
- ✅ Pydantic schemas for validation
- ✅ Tauri integration ready
- ✅ Same model catalogs as CC-Switch

**Providers Supported**:
- OpenRouter (5 models)
- Anthropic (5 models)
- OpenAI (5 models)
- Google (5 models)
- DeepSeek (5 models)

---

## CC-Switch vs II-Agent Feature Comparison

| Feature | CC-Switch | II-Agent | Gap |
|---------|-----------|----------|-----|
| **Provider CRUD** | ✅ SQLite + JSON | ✅ JSON files | None |
| **Provider Switching** | ✅ Per-app | ✅ Per-app | None |
| **Model Picker** | ✅ 5 models/provider | ✅ 5 models/provider | None |
| **Proxy Support** | ✅ Global + per-provider | ✅ Per-provider only | CC-Switch has global |
| **HTTP_PROXY Handling** | ✅ Fixed (#1100) | ❌ Not implemented | II-Agent needs fix |
| **Failover** | ✅ Automatic | ❌ None | Major gap |
| **Circuit Breaker** | ✅ Yes | ❌ None | Major gap |
| **Key Rotation** | ⏳ Designed | ❌ None | Both need |
| **Model Family Routing** | ⏳ Designed | ❌ None | Both need |
| **Concurrency Limits** | ⏳ Designed | ❌ None | Both need |
| **Health Checking** | ✅ Yes | ❌ None | Major gap |
| **Tauri Integration** | ✅ Full | ⏳ Partial | II-Agent needs commands |

---

## Recommendations for II-Agent Integration

### Priority 1: Borrow CC-Switch Proxy Features

#### 1.1 Add Failover Support

**CC-Switch Implementation**:
```rust
// src/proxy/provider_router.rs
pub async fn select_providers(&self, app_type: &str) -> Result<Vec<Provider>, AppError> {
    // Returns prioritized list based on failover queue
    // Automatically skips providers in cooldown
}
```

**II-Agent Integration**:
```python
# Add to src/ii_agent/llm/proxy/manager.py
class ProviderManager:
    async def get_failover_providers(self, app_id: str) -> List[Provider]:
        """Get providers in failover order."""
        # Borrow logic from CC-Switch provider_router.rs
        pass
```

#### 1.2 Add Circuit Breaker

**CC-Switch Implementation**:
```rust
// src/proxy/circuit_breaker.rs
pub struct CircuitBreaker {
    failure_threshold: u32,
    success_threshold: u32,
    timeout_seconds: u64,
    error_rate_threshold: f64,
}
```

**II-Agent Integration**:
```python
# New file: src/ii_agent/llm/proxy/circuit_breaker.py
class CircuitBreaker:
    def __init__(self, failure_threshold: int = 5):
        self.failure_threshold = failure_threshold
        self.failures = 0
        self.last_failure_time: Optional[float] = None
    
    async def allow_request(self) -> bool:
        """Check if request should be allowed."""
        pass
    
    async def record_success(self):
        """Record successful request."""
        pass
    
    async def record_failure(self):
        """Record failed request."""
        pass
```

#### 1.3 Fix HTTP_PROXY Handling

**CC-Switch Fix** (already done):
```rust
// src-tauri/src/proxy/http_client.rs
fn points_to_cc_switch_proxy(value: &str) -> bool {
    // Only bypass if pointing to CC-Switch's own port
    // Allow external proxies (v2ray/Clash) for upstream
}
```

**II-Agent Integration**:
```python
# Add to src/ii_agent/llm/proxy/factory.py
def _build_http_client(self, provider: Provider) -> httpx.AsyncClient:
    """Build HTTP client with proper proxy handling."""
    # Borrow logic from CC-Switch http_client.rs
    proxy_config = provider.meta.proxy_config if provider.meta else None
    
    if proxy_config and proxy_config.enabled:
        # Use provider-specific proxy
        return self._create_proxy_client(proxy_config)
    else:
        # Respect system HTTP_PROXY but avoid recursion
        return self._create_system_proxy_client()
```

---

### Priority 2: Address Issue #155 (Mid-Session Model Switching)

#### Current State
- Settings configured before session starts
- No way to modify during active session
- Users want to switch on rate limits, context issues

#### Proposed Solution

**Backend API** (Python):
```python
# src/ii_agent/llm/proxy/manager.py
class ProviderManager:
    async def switch_model_mid_session(
        self,
        session_id: str,
        new_provider_id: str,
        new_model: str,
    ) -> Tuple[bool, str]:
        """Switch model for active session."""
        session = self.session_store.get(session_id)
        if not session:
            return False, "Session not found"
        
        # Validate new provider/model
        provider = self.config.get(new_provider_id)
        if not provider:
            return False, "Provider not found"
        
        # Update session config
        session.config.provider_id = new_provider_id
        session.config.model = new_model
        
        # Recreate LLM client with new config
        session.llm_client = self.factory.create_client(
            provider_id=new_provider_id,
            model=new_model,
        )
        
        return True, f"Switched to {provider.name}/{new_model}"
```

**Frontend API** (Tauri Command):
```python
# src/ii_agent/server/commands/provider.py
@tauri_command
async def switch_session_model(
    session_id: str,
    provider_id: str,
    model: str,
) -> dict:
    """Switch model for active session (Issue #155)."""
    manager = ProviderManager()
    success, message = await manager.switch_model_mid_session(
        session_id, provider_id, model
    )
    return {"success": success, "message": message}
```

**Frontend UI** (React):
```tsx
// frontend/src/components/SessionModelSwitcher.tsx
function SessionModelSwitcher({ sessionId }: { sessionId: string }) {
  const [currentModel, setCurrentModel] = useState('');
  
  const switchModel = async (newProviderId: string, newModel: string) => {
    const result = await invoke('switch_session_model', {
      sessionId,
      providerId: newProviderId,
      model: newModel,
    });
    
    if (result.success) {
      toast.success(result.message);
      setCurrentModel(newModel);
    } else {
      toast.error(result.message);
    }
  };
  
  return (
    <ModelPicker
      models={availableModels}
      selectedModel={currentModel}
      onModelChange={(model) => switchModel(model.providerId, model.id)}
      label="Switch Model (Mid-Session)"
    />
  );
}
```

---

### Priority 3: Model Enumeration Enhancement

#### Current II-Agent Implementation
```python
# src/ii_agent/llm/proxy/manager.py
async def enumerate_models(
    self,
    provider_id: str,
    force_refresh: bool = False,
) -> List[RemoteModelInfo]:
    """Enumerate available models for a provider."""
    provider = self.config.get(provider_id)
    if not provider:
        raise ValueError(f"Provider {provider_id} not found")
    
    # Fetch from provider's /models endpoint
    models = await enumerate_models(
        base_url=provider.settings_config.get("base_url"),
        api_key=provider.settings_config.get("api_key"),
        api_format=provider.meta.api_format,
    )
    
    return models
```

#### Enhancement: Cache + Model Picker Integration

```python
# Enhanced version with caching and model picker
class ProviderManager:
    def __init__(self):
        self.model_cache = TTLCache(maxsize=100, ttl=3600)  # 1 hour cache
    
    async def get_available_models(
        self,
        provider_id: str,
        use_cache: bool = True,
    ) -> List[ModelInfo]:
        """Get models with cache and model picker fallback."""
        
        # Try model picker first (curated list)
        picker = get_model_picker(provider_id)
        if picker:
            return picker.models
        
        # Try cache
        if use_cache and provider_id in self.model_cache:
            return self.model_cache[provider_id]
        
        # Fetch from provider API
        models = await self.enumerate_models(provider_id)
        
        # Cache result
        self.model_cache[provider_id] = models
        return models
    
    async def validate_model(
        self,
        provider_id: str,
        model_id: str,
    ) -> bool:
        """Validate model exists."""
        models = await self.get_available_models(provider_id)
        return any(m.id == model_id for m in models)
```

---

## Implementation Roadmap for II-Agent

### Phase 1: Core Proxy Features (Week 1-2)
- [ ] Add circuit breaker (`src/ii_agent/llm/proxy/circuit_breaker.py`)
- [ ] Add failover manager (`src/ii_agent/llm/proxy/failover.py`)
- [ ] Fix HTTP_PROXY handling (`src/ii_agent/llm/proxy/factory.py`)
- [ ] Add health checking (`src/ii_agent/llm/proxy/health.py`)

**Estimated**: 20-30 hours

### Phase 2: Mid-Session Switching (Week 2-3)
- [ ] Add session model switching API (Issue #155)
- [ ] Update session store to support config changes
- [ ] Add Tauri commands for model switching
- [ ] Build frontend UI component

**Estimated**: 16-24 hours

### Phase 3: Advanced Features (Week 3-4)
- [ ] Multi-API key support (like CC-Switch #1006)
- [ ] Model family routing (like CC-Switch #1085)
- [ ] Concurrency limits (like CC-Switch #961)
- [ ] Model enumeration caching

**Estimated**: 24-32 hours

### Phase 4: Integration & Testing (Week 4-5)
- [ ] Write unit tests for all new features
- [ ] Integration tests with CC-Switch proxy
- [ ] Performance testing
- [ ] Documentation updates

**Estimated**: 16-20 hours

**Total Estimated**: 76-106 hours

---

## Code Sharing Opportunities

### Shared Model Catalogs
Both CC-Switch and II-Agent have identical model picker catalogs:
- OpenRouter (5 models)
- Anthropic (5 models)
- OpenAI (5 models)
- Google (5 models)
- DeepSeek (5 models)

**Recommendation**: Create shared package
```
packages/model-catalog/
├── src/
│   ├── index.ts        # TypeScript exports
│   ├── models.py       # Python exports
│   └── data/
│       └── models.json # Shared model data
```

### Shared Proxy Logic
CC-Switch's HTTP_PROXY fix and circuit breaker can be ported to II-Agent:

**CC-Switch Rust → II-Agent Python**:
```rust
// Rust (CC-Switch)
fn points_to_cc_switch_proxy(value: &str) -> bool {
    // Logic here
}
```

```python
# Python (II-Agent)
def points_to_ii_agent_proxy(value: str) -> bool:
    # Same logic, Python syntax
    pass
```

---

## Conclusion

### What II-Agent Has (Better than CC-Switch)
- ✅ Python-based (easier to extend)
- ✅ Session management built-in
- ✅ Agent orchestration
- ✅ Sandbox integration

### What CC-Switch Has (II-Agent Needs)
- ✅ Failover mechanism
- ✅ Circuit breaker
- ✅ Health checking
- ✅ HTTP_PROXY compatibility
- ✅ Model family routing
- ✅ Concurrency limits

### Best Path Forward
1. **Borrow CC-Switch proxy features** (failover, circuit breaker, health)
2. **Fix HTTP_PROXY handling** (use CC-Switch fix as reference)
3. **Implement Issue #155** (mid-session model switching)
4. **Share model catalogs** (avoid duplication)
5. **Keep projects separate** (no monorepo)

**No modifications to OpenCode or OpenClaw repos** - integration via configuration only.

---

## References

### II-Agent Files
- `src/ii_agent/llm/proxy/manager.py` - Provider CRUD
- `src/ii_agent/llm/proxy/factory.py` - Client factory
- `src/ii_agent/llm/proxy/model_picker.py` - Model catalogs
- `src/ii_agent/llm/proxy/README.md` - Documentation

### CC-Switch Files
- `src-tauri/src/proxy/provider_router.rs` - Failover router
- `src-tauri/src/proxy/circuit_breaker.rs` - Circuit breaker
- `src-tauri/src/proxy/http_client.rs` - HTTP_PROXY fix
- `src/opencode_config.rs` - Model picker (Rust backend)

### GitHub Issues
- II-Agent #155: Mid-session model switching
- CC-Switch #1100: HTTP_PROXY compatibility
- CC-Switch #1006: Multi-API key support
- CC-Switch #1085: Model family routing
