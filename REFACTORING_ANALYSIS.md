# OpenClaw, II-Agent, OpenCode Refactoring Analysis

## Executive Summary

After analyzing all three projects (OpenClaw, II-Agent, OpenCode) and their relationship with CC-Switch, here are the key findings:

### No Open Issues Found
- **OpenClaw**: No existing cc-switch integration issues
- **II-Agent**: Has cc-switch-preset package ready for integration
- **OpenCode**: No cc-switch references (confirmed - no proxy support)

---

## Project Analysis

### 1. OpenClaw

**Status**: Mature production project with extensive provider support

#### Key Features Found:
- ✅ **Model Failover System** (`/docs/concepts/model-failover.md`)
  - Auth profile rotation within providers
  - Model fallback chains
  - Cooldowns with exponential backoff
  - Billing failure handling
  
- ✅ **Model Selection CLI** (`/docs/concepts/models.md`)
  - `/model` command with picker UI
  - Model aliases and fallbacks
  - Provider allowlisting
  
- ✅ **30+ Provider Support**
  - Anthropic, OpenAI, OpenRouter, LiteLLM, Vercel AI Gateway
  - Cloud providers: Bedrock, Cloudflare, Together AI
  - Regional: Moonshot (Kimi), MiniMax, GLM, Zhipu, Venice
  
- ✅ **OpenCode Zen Integration** (`/docs/providers/opencode.md`)
  - Uses OpenCode curated models
  - API key authentication
  - Config: `opencode/claude-opus-4-6`

#### Architecture:
```
OpenClaw Gateway (Node.js/TypeScript)
├── Auth Profiles (~/.openclaw/agents/<id>/auth-profiles.json)
├── Model Config (agents.defaults.model.primary/fallbacks)
├── Provider Rotation (OAuth → API keys, round-robin)
└── Cooldown System (1min → 5min → 25min → 1hr cap)
```

#### What's Missing (Opportunity for CC-Switch):
- ❌ No local proxy server for failover
- ❌ No centralized provider management UI
- ❌ No model picker UI (CLI only)
- ❌ No cross-provider health checking

---

### 2. II-Agent

**Status**: Active development with cc-switch integration ready

#### Existing CC-Switch Integration:
Located in `/Users/jim/work/ii-agent/cc-switch-preset/`

**Files Ready**:
- `iiAgentProviderPresets.ts` - 11 provider presets
- `iiAgentEndpoints.ts` - Endpoint annotations
- `slots.ts` - Automatic trait derivation
- `IIAGENT_CAPABILITY_MATRIX.md` - Documentation
- `MODEL_PICKER.md` - Model picker spec (5 models per provider)

#### Provider Presets (11 Total):
| Preset | Formats | Universal | Apps |
|--------|---------|-----------|------|
| Anthropic | `["anthropic"]` | ❌ | Claude, OpenCode, OpenClaw |
| OpenAI | `["openai_chat", "openai_responses"]` | ❌ | Codex, OpenCode, OpenClaw |
| DeepSeek | `["anthropic", "openai_chat"]` | ✅ | All apps |
| Zhipu GLM | `["anthropic", "openai_chat"]` | ✅ | All apps |
| Bailian | `["anthropic", "openai_chat"]` | ✅ | All apps |
| Kimi | `["anthropic", "openai_chat"]` | ✅ | All apps |
| MiniMax | `["anthropic", "openai_chat"]` | ✅ | All apps |
| Custom Anthropic | `["anthropic", "openai_chat"]` | ✅ | All apps |
| Custom OpenAI | `["openai_chat", "openai_responses"]` | ❌ | Codex, OpenCode, OpenClaw |

#### Automatic Trait Derivation:
```typescript
// No manual traits needed - derived from transport.formats
formats: ["anthropic"] → claude, opencode, openclaw
formats: ["openai_chat", "openai_responses"] → codex, opencode, openclaw  
formats: ["anthropic", "openai_chat"] → ALL apps (universal)
```

#### Model Picker Spec:
- **Exactly 5 models per provider**
- Providers: OpenRouter, Anthropic, OpenAI, Google, DeepSeek
- Includes: context window, output tokens, capabilities, pricing
- Use case recommendations (coding, chat, analysis)

#### Tauri Backend API (Planned):
```python
GET  /api/v1/providers           # List providers
POST /api/v1/providers           # Add provider
POST /api/v1/providers/{id}/switch  # Switch for app
GET  /api/v1/providers/{id}/models  # Enumerate models
```

---

### 3. OpenCode

**Status**: No proxy support confirmed

#### What Exists:
- Basic HTTP proxy via env vars only (`HTTP_PROXY`, `HTTPS_PROXY`)
- AI SDK provider integration (@ai-sdk/* packages)
- Provider config in `opencode.json`:
  ```json
  {
    "provider": {
      "openrouter": {
        "npm": "@openrouter/ai-sdk-provider",
        "options": { "baseURL": "...", "apiKey": "..." },
        "models": { "model-id": { "name": "..." } }
      }
    }
  }
  ```

#### What's Missing:
- ❌ No provider-level proxy configuration
- ❌ No failover mechanism
- ❌ No model picker UI
- ❌ No health checking
- ❌ No provider rotation

---

## Integration Opportunities

### For CC-Switch → OpenClaw

**What CC-Switch Can Provide**:
1. **Local Proxy Server** - Handle failover before requests reach OpenClaw
2. **Provider Management UI** - Visual configuration vs CLI editing
3. **Model Picker** - GUI for model selection with 5-model catalogs
4. **Health Monitoring** - Real-time provider status dashboard

**Integration Approach**:
```
OpenClaw → CC-Switch Proxy (localhost:PORT) → Multiple Upstream Providers
                    ↓
            Failover Logic + Health Checks
```

**Config Changes Needed**:
```json5
// OpenClaw config
{
  agents: {
    defaults: {
      model: {
        primary: "cc-switch/claude-sonnet-4-5"  // Route through proxy
      }
    }
  },
  env: {
    ANTHROPIC_BASE_URL: "http://localhost:PORT/v1"  // CC-Switch proxy
  }
}
```

---

### For CC-Switch → II-Agent

**Status**: Integration package already exists!

**Next Steps**:
1. **Copy preset files** to CC-Switch:
   ```bash
   cp ii-agent/cc-switch-preset/*.ts src/config/
   cp ii-agent/cc-switch-preset/*.svg src/icons/extracted/
   ```

2. **Register in CC-Switch**:
   - Add icon to `src/icons/extracted/index.ts`
   - Add endpoints to `src/config/capabilities/endpoints.ts`
   - Add presets to provider list

3. **Implement Model Picker**:
   - Create `src/config/modelPicker.ts` (match II-Agent spec)
   - Add Tauri commands for model enumeration
   - Build React UI component

4. **Proxy Integration**:
   - II-Agent proxy → CC-Switch failover router
   - Share model catalogs between projects

---

### For CC-Switch → OpenCode

**What We Already Built**:
- ✅ OpenCode proxy adapter (`src/proxy/providers/opencode.rs`)
- ✅ Model picker catalog (`src/opencode_config.rs`)
- ✅ `-cc` suffix provider management
- ✅ Tauri commands for model management

**Next Steps**:
1. **Complete Proxy Handler**:
   - Add `/v1/chat/completions` endpoint
   - Support AI SDK message format
   - Integrate with failover router

2. **Test with Vite Dev Server**:
   ```bash
   pnpm dev:renderer  # Frontend
   pnpm dev           # Full Tauri
   ```

3. **Document Integration**:
   - Update README with OpenCode support
   - Add configuration examples

---

## Recommended Refactoring Priority

### Phase 1: II-Agent Integration (Easiest)
**Why**: Preset package already exists, clear integration path

**Tasks**:
1. Copy preset files to CC-Switch
2. Register icon and endpoints
3. Test automatic trait derivation
4. Document integration

**Estimated Effort**: 2-4 hours

---

### Phase 2: OpenCode Proxy Completion
**Why**: Core infrastructure built, needs endpoint handling

**Tasks**:
1. Add OpenCode endpoints to proxy handler
2. Implement AI SDK format transformation
3. Test with Vite dev server
4. Add model picker UI component

**Estimated Effort**: 4-8 hours

---

### Phase 3: OpenClaw Integration (Most Complex)
**Why**: Requires understanding OpenClaw's auth profile system

**Tasks**:
1. Study OpenClaw auth rotation (`model-failover.md`)
2. Design proxy integration that respects auth profiles
3. Implement CC-Switch as upstream provider
4. Test failover interaction (CC-Switch + OpenClaw)

**Estimated Effort**: 8-16 hours

---

## Architecture Recommendations

### Unified Model Picker
Create shared model catalog usable by all three projects:

```typescript
// Shared: packages/model-catalog/src/index.ts
export const MODEL_CATALOG = {
  openrouter: [/* 5 models */],
  anthropic: [/* 5 models */],
  // ...
}
```

**Benefits**:
- Consistent models across CC-Switch, II-Agent, OpenClaw
- Single source of truth
- Easy to add new providers

### Proxy Layer Unification
```
┌─────────────┐     ┌──────────────┐     ┌─────────────┐
│  II-Agent   │────▶│  CC-Switch   │────▶│  Upstream   │
│  OpenClaw   │────▶│    Proxy     │     │  Providers  │
│  OpenCode   │────▶│  (Failover)  │     │             │
└─────────────┘     └──────────────┘     └─────────────┘
```

**Benefits**:
- Centralized failover logic
- Shared health checking
- Consistent provider management

---

## Open Issues / Questions

### For OpenClaw:
1. How does auth profile rotation interact with external proxy?
2. Can CC-Switch proxy preserve session stickiness?
3. Should CC-Switch integrate with OpenClaw's cooldown system?

### For II-Agent:
1. Tauri backend API - implement in Rust or Python?
2. Model picker - shared component or separate per project?
3. Preset package - merge into CC-Switch or keep separate?

### For OpenCode:
1. AI SDK format transformation - test coverage needed
2. `-cc` suffix convention - document for users
3. Model picker UI - prioritize which providers first?

---

## Next Actions

### Immediate (This Week):
1. ✅ Complete OpenCode proxy adapter (done)
2. ⏳ Copy II-Agent presets to CC-Switch
3. ⏳ Test OpenCode model picker commands
4. ⏳ Document integration findings

### Short-term (Next Week):
1. Implement II-Agent Tauri commands
2. Build model picker UI component
3. Test OpenCode proxy with Vite dev server
4. Create OpenClaw integration design doc

### Long-term (This Month):
1. Complete OpenClaw proxy integration
2. Unify model catalogs across projects
3. Add health monitoring dashboard
4. Write comprehensive integration docs

---

## References

### OpenClaw:
- Model Failover: `/docs/concepts/model-failover.md`
- Models CLI: `/docs/concepts/models.md`
- Providers: `/docs/providers/`
- OpenCode Zen: `/docs/providers/opencode.md`

### II-Agent:
- CC-Switch Preset: `cc-switch-preset/README.md`
- Integration Guide: `cc-switch-preset/INTEGRATION.md`
- Model Picker: `cc-switch-preset/MODEL_PICKER.md`
- Capability Matrix: `cc-switch-preset/IIAGENT_CAPABILITY_MATRIX.md`

### OpenCode:
- Provider Config: `packages/opencode/src/provider/provider.ts`
- Transform Logic: `packages/opencode/src/provider/transform.ts`
- Proxy Detection: `packages/opencode/src/util/proxied.ts`

### CC-Switch:
- OpenCode Integration: `OPENCODE_INTEGRATION_PLAN.md`
- Proxy Adapters: `src/proxy/providers/`
- Model Picker: `src/opencode_config.rs`
