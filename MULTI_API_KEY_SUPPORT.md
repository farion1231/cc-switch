# Multi-API Key Support (#1006)

## Issue Description

**Issue #1006**: "目前一个链接只能配一个 api key，管理不便。希望大佬能增加一下多 api 的管理和测试"

**Translation**: "Currently only one API key can be configured per provider, which is inconvenient for management. Hope to add multi-API key management and testing."

## User Requirements

1. **Multiple API Keys per Provider** - Store multiple keys for rotation
2. **Key Testing** - Validate keys before use
3. **Automatic Rotation** - Switch keys on rate limit or error
4. **Key Status Display** - Show which keys are valid/invalid

## Design

### Data Structure Changes

#### Provider Configuration (TypeScript Frontend)

```typescript
// src/types/provider.ts
export interface Provider {
  id: string;
  name: string;
  settings_config: {
    env: {
      // Single key (legacy, for backward compatibility)
      ANTHROPIC_AUTH_TOKEN?: string;
      
      // Multiple keys (new)
      ANTHROPIC_AUTH_TOKENS?: string[];  // Array of keys
    };
  };
  meta?: {
    apiKeyRotation?: {
      enabled: boolean;
      currentKeyIndex: number;
      keyStatuses: KeyStatus[];
    };
  };
}

export interface KeyStatus {
  key: string;           // Masked: "sk-ant...abc123"
  isValid?: boolean;     // Last test result
  lastTested?: number;   // Timestamp
  usageCount?: number;   // How many times used
  errorCount?: number;   // Consecutive errors
  cooldownUntil?: number;// Cooldown timestamp
}
```

#### Provider Configuration (Rust Backend)

```rust
// src/provider.rs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provider {
    pub id: String,
    pub name: String,
    #[serde(rename = "settingsConfig")]
    pub settings_config: Value,
    // ... existing fields ...
    
    /// API key rotation metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<ProviderMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderMeta {
    // ... existing fields ...
    
    /// API key rotation configuration
    #[serde(rename = "apiKeyRotation", skip_serializing_if = "Option::is_none")]
    pub api_key_rotation: Option<ApiKeyRotationConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyRotationConfig {
    /// Enable automatic rotation
    pub enabled: bool,
    
    /// Current key index (for round-robin)
    pub current_key_index: usize,
    
    /// Key statuses (validity, usage count, etc.)
    pub key_statuses: Vec<KeyStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyStatus {
    /// Masked key (for display)
    pub key: String,
    
    /// Last test result
    #[serde(rename = "isValid", skip_serializing_if = "Option::is_none")]
    pub is_valid: Option<bool>,
    
    /// Last test timestamp
    #[serde(rename = "lastTested", skip_serializing_if = "Option::is_none")]
    pub last_tested: Option<i64>,
    
    /// Usage count
    #[serde(rename = "usageCount", skip_serializing_if = "Option::is_none")]
    pub usage_count: Option<usize>,
    
    /// Consecutive error count
    #[serde(rename = "errorCount", skip_serializing_if = "Option::is_none")]
    pub error_count: Option<usize>,
    
    /// Cooldown until timestamp
    #[serde(rename = "cooldownUntil", skip_serializing_if = "Option::is_none")]
    pub cooldown_until: Option<i64>,
}
```

### API Changes

#### Tauri Commands (New)

```rust
// src/commands/provider.rs

/// Test all API keys for a provider
#[tauri::command]
pub async fn test_provider_api_keys(
    state: State<'_, AppState>,
    app: String,
    provider_id: String,
) -> Result<Vec<KeyTestResult>, String> {
    ProviderService::test_all_api_keys(state.inner(), &app, &provider_id)
        .await
        .map_err(|e| e.to_string())
}

/// Test a specific API key
#[tauri::command]
pub async fn test_api_key(
    state: State<'_, AppState>,
    app: String,
    provider_id: String,
    key_index: usize,
) -> Result<KeyTestResult, String> {
    ProviderService::test_api_key_at_index(state.inner(), &app, &provider_id, key_index)
        .await
        .map_err(|e| e.to_string())
}

/// Add a new API key to provider
#[tauri::command]
pub async fn add_api_key(
    state: State<'_, AppState>,
    app: String,
    provider_id: String,
    api_key: String,
) -> Result<bool, String> {
    ProviderService::add_api_key(state.inner(), &app, &provider_id, &api_key)
        .map_err(|e| e.to_string())
}

/// Remove an API key from provider
#[tauri::command]
pub async fn remove_api_key(
    state: State<'_, AppState>,
    app: String,
    provider_id: String,
    key_index: usize,
) -> Result<bool, String> {
    ProviderService::remove_api_key(state.inner(), &app, &provider_id, key_index)
        .map_err(|e| e.to_string())
}

/// Get key rotation status
#[tauri::command]
pub async fn get_key_rotation_status(
    state: State<'_, AppState>,
    app: String,
    provider_id: String,
) -> Result<Option<ApiKeyRotationConfig>, String> {
    ProviderService::get_key_rotation_status(state.inner(), &app, &provider_id)
        .map_err(|e| e.to_string())
}

/// Enable/disable key rotation
#[tauri::command]
pub async fn set_key_rotation_enabled(
    state: State<'_, AppState>,
    app: String,
    provider_id: String,
    enabled: bool,
) -> Result<bool, String> {
    ProviderService::set_key_rotation_enabled(state.inner(), &app, &provider_id, enabled)
        .map_err(|e| e.to_string())
}
```

### Key Rotation Logic

#### Runtime Key Selection

```rust
// src/services/provider/key_rotation.rs

pub struct KeyRotationManager {
    db: Arc<Database>,
}

impl KeyRotationManager {
    /// Get next valid API key (round-robin with cooldown)
    pub async fn get_next_key(&self, app_type: &str, provider_id: &str) -> Result<String, AppError> {
        let provider = self.db.get_provider(app_type, provider_id)?;
        let keys = self.extract_api_keys(&provider)?;
        let mut rotation = self.get_rotation_config(&provider)?;
        
        // Find next valid key (skip keys in cooldown)
        let start_index = rotation.current_key_index;
        let mut attempts = 0;
        
        while attempts < keys.len() {
            let index = (start_index + attempts) % keys.len();
            let key_status = rotation.key_statuses.get(index);
            
            // Check if key is in cooldown
            if let Some(status) = key_status {
                if let Some(cooldown_until) = status.cooldown_until {
                    if chrono::Utc::now().timestamp_millis() < cooldown_until {
                        attempts += 1;
                        continue; // Skip this key, try next
                    }
                }
            }
            
            // Found valid key
            rotation.current_key_index = index;
            self.save_rotation_config(provider_id, &rotation).await?;
            
            return Ok(keys[index].clone());
        }
        
        // All keys in cooldown - return first key anyway (will fail, but that's expected)
        Ok(keys[start_index % keys.len()].clone())
    }
    
    /// Record key usage/error for rotation statistics
    pub async fn record_key_result(
        &self,
        provider_id: &str,
        key_index: usize,
        success: bool,
        error_msg: Option<&str>,
    ) -> Result<(), AppError> {
        let mut rotation = self.get_rotation_config_by_id(provider_id)?;
        
        // Ensure key_statuses has enough entries
        while rotation.key_statuses.len() <= key_index {
            rotation.key_statuses.push(KeyStatus::default());
        }
        
        let status = &mut rotation.key_statuses[key_index];
        status.last_tested = Some(chrono::Utc::now().timestamp_millis());
        
        if success {
            status.is_valid = Some(true);
            status.error_count = Some(0);
            status.cooldown_until = None;
            status.usage_count = Some(status.usage_count.unwrap_or(0) + 1);
        } else {
            status.error_count = Some(status.error_count.unwrap_or(0) + 1);
            
            // Apply exponential backoff cooldown for errors
            let error_count = status.error_count.unwrap_or(1);
            let cooldown_minutes = match error_count {
                1 => 1,    // 1 minute
                2 => 5,    // 5 minutes
                3 => 25,   // 25 minutes
                _ => 60,   // 1 hour cap
            };
            
            status.cooldown_until = Some(
                chrono::Utc::now().timestamp_millis() + (cooldown_minutes * 60 * 1000)
            );
            
            // Mark invalid on billing errors
            if let Some(msg) = error_msg {
                if msg.contains("billing") || msg.contains("credit") || msg.contains("insufficient") {
                    status.is_valid = Some(false);
                }
            }
        }
        
        self.save_rotation_config(provider_id, &rotation).await
    }
}
```

### Frontend UI Changes

#### Provider Form (Add Multi-Key Section)

```tsx
// src/components/providers/forms/ClaudeFormFields.tsx

function ApiKeyRotationSection({ provider, onChange }: ApiKeyRotationSectionProps) {
  const [keys, setKeys] = useState<string[]>(
    provider.settings_config.env?.ANTHROPIC_AUTH_TOKENS || 
    (provider.settings_config.env?.ANTHROPIC_AUTH_TOKEN ? [provider.settings_config.env.ANTHROPIC_AUTH_TOKEN] : [])
  );
  
  const [rotationEnabled, setRotationEnabled] = useState(
    provider.meta?.apiKeyRotation?.enabled || false
  );
  
  const addKey = () => {
    setKeys([...keys, ""]);
  };
  
  const removeKey = (index: number) => {
    setKeys(keys.filter((_, i) => i !== index));
  };
  
  const updateKey = (index: number, value: string) => {
    const newKeys = [...keys];
    newKeys[index] = value;
    setKeys(newKeys);
  };
  
  const testAllKeys = async () => {
    // Invoke Tauri command to test all keys
    const results = await test_provider_api_keys(provider.app, provider.id);
    // Display results in UI
  };
  
  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-medium">API Keys</h3>
        <Button onClick={addKey} size="sm">
          <Plus className="w-4 h-4 mr-1" />
          Add Key
        </Button>
      </div>
      
      {/* Key List */}
      <div className="space-y-2">
        {keys.map((key, index) => (
          <div key={index} className="flex items-center gap-2">
            <PasswordInput
              value={key}
              onChange={(v) => updateKey(index, v)}
              placeholder={`API Key ${index + 1}`}
              className="flex-1"
            />
            <Button
              variant="outline"
              size="sm"
              onClick={() => testKey(index)}
            >
              Test
            </Button>
            {keys.length > 1 && (
              <Button
                variant="ghost"
                size="sm"
                onClick={() => removeKey(index)}
              >
                <X className="w-4 h-4" />
              </Button>
            )}
          </div>
        ))}
      </div>
      
      {/* Rotation Settings */}
      {keys.length > 1 && (
        <div className="border-t pt-4">
          <div className="flex items-center justify-between mb-2">
            <label className="text-sm font-medium">Automatic Key Rotation</label>
            <Switch
              checked={rotationEnabled}
              onCheckedChange={setRotationEnabled}
            />
          </div>
          <p className="text-xs text-muted-foreground">
            Automatically rotate between keys on rate limits. Keys in cooldown will be skipped.
          </p>
          
          {/* Key Status Table */}
          <KeyStatusTable providerId={provider.id} app={provider.app} />
        </div>
      )}
      
      {/* Test All Button */}
      {keys.length > 0 && (
        <Button onClick={testAllKeys} variant="outline" className="w-full">
          Test All Keys
        </Button>
      )}
    </div>
  );
}
```

#### Key Status Table Component

```tsx
// src/components/providers/KeyStatusTable.tsx

interface KeyStatus {
  key: string;           // Masked
  isValid?: boolean;
  lastTested?: number;
  usageCount?: number;
  errorCount?: number;
  cooldownUntil?: number;
}

export function KeyStatusTable({ providerId, app }: KeyStatusTableProps) {
  const { data: rotation } = useQuery({
    queryKey: ['keyRotation', providerId, app],
    queryFn: () => get_key_rotation_status(app, providerId),
  });
  
  if (!rotation) return null;
  
  return (
    <Table>
      <TableHeader>
        <TableRow>
          <TableHead>Key</TableHead>
          <TableHead>Status</TableHead>
          <TableHead>Usage</TableHead>
          <TableHead>Errors</TableHead>
          <TableHead>Cooldown</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {rotation.key_statuses.map((status, index) => (
          <TableRow key={index}>
            <TableCell className="font-mono">
              {maskKey(status.key)}
            </TableCell>
            <TableCell>
              {status.isValid === true ? (
                <Badge variant="success">Valid</Badge>
              ) : status.isValid === false ? (
                <Badge variant="destructive">Invalid</Badge>
              ) : (
                <Badge variant="secondary">Unknown</Badge>
              )}
            </TableCell>
            <TableCell>{status.usageCount || 0}</TableCell>
            <TableCell>{status.errorCount || 0}</TableCell>
            <TableCell>
              {status.cooldownUntil && status.cooldownUntil > Date.now() ? (
                <CountdownTimer targetTime={status.cooldownUntil} />
              ) : (
                <span className="text-green-600">Ready</span>
              )}
            </TableCell>
          </TableRow>
        ))}
      </TableBody>
    </Table>
  );
}
```

## Implementation Phases

### Phase 1: Backend Support (Rust)
- [ ] Add `ApiKeyRotationConfig` and `KeyStatus` structs
- [ ] Add `ProviderMeta.api_key_rotation` field
- [ ] Implement `KeyRotationManager` service
- [ ] Add Tauri commands for key management
- [ ] Integrate key rotation into provider selection

**Estimated**: 8-12 hours

### Phase 2: Frontend UI (TypeScript/React)
- [ ] Add multi-key input section to provider forms
- [ ] Create `KeyStatusTable` component
- [ ] Add key testing UI
- [ ] Add rotation toggle and settings
- [ ] Update provider save/load logic

**Estimated**: 6-10 hours

### Phase 3: Testing & Documentation
- [ ] Write unit tests for key rotation
- [ ] Test with rate limit scenarios
- [ ] Update user documentation
- [ ] Add migration guide for existing users

**Estimated**: 4-6 hours

## Backward Compatibility

### Migration Strategy

1. **Existing Single-Key Providers**: No change required
   - Single `ANTHROPIC_AUTH_TOKEN` continues to work
   - UI shows single key input by default

2. **Automatic Conversion**: When user adds second key
   - Convert `ANTHROPIC_AUTH_TOKEN` → `ANTHROPIC_AUTH_TOKENS[0]`
   - Enable rotation automatically
   - Show multi-key UI

3. **Legacy Support**: Always check both fields
   ```rust
   fn extract_api_keys(provider: &Provider) -> Vec<String> {
       // Try multi-key first
       if let Some(keys) = provider.settings_config
           .get("env")
           .and_then(|v| v.get("ANTHROPIC_AUTH_TOKENS"))
           .and_then(|v| v.as_array()) {
               return keys.iter()
                   .filter_map(|v| v.as_str().map(String::from))
                   .collect();
       }
       
       // Fall back to single key
       if let Some(key) = provider.settings_config
           .get("env")
           .and_then(|v| v.get("ANTHROPIC_AUTH_TOKEN"))
           .and_then(|v| v.as_str()) {
               return vec![key.to_string()];
       }
       
       vec![]
   }
   ```

## Testing Scenarios

### Test Case 1: Add Multiple Keys
```
1. Open provider form
2. Click "Add Key"
3. Enter 3 different API keys
4. Click "Test All Keys"
5. Verify all 3 keys show "Valid" status
```

### Test Case 2: Automatic Rotation
```
1. Configure 2 keys with rotation enabled
2. Make requests until key 1 hits rate limit
3. Verify key 2 is automatically selected
4. Verify key 1 enters cooldown
5. After cooldown, verify key 1 is available again
```

### Test Case 3: Billing Error
```
1. Configure key with insufficient credits
2. Make request (fails with billing error)
3. Verify key marked as "Invalid"
4. Verify key skipped in rotation
5. Add new valid key
6. Verify new key used instead
```

## Related Issues

- #1006 - Multi-API key management (this feature)
- #961 - Per-provider concurrency limits (complementary)
- #1085 - Model family routing (can use different keys per family)

## References

- OpenClaw Auth Profiles: `/docs/concepts/model-failover.md`
- CC-Switch Provider Meta: `src/provider.rs`
- Key Rotation Design: `MULTI_API_KEY_SUPPORT.md`
