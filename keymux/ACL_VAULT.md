# ACL Key Vault - POSIX-Friendly Key Management

## Overview

ModelMux uses a **POSIX-friendly ACL hierarchy** for API key management:
- Filesystem-based storage (`~/.cc-switch/acl/`)
- Opt-in environment variable support (`XXXXX_API_KEY`)
- POSIX permissions (owner/group/other)
- No complex dependencies

## Directory Structure

```
~/.cc-switch/acl/
├── anthropic/
│   ├── key-1.key           # API key content
│   ├── key-1.meta          # Metadata (quota, permissions)
│   └── key-2.key
├── openai/
│   └── key-1.key
├── google/
│   └── key-1.key
└── deepseek/
    └── key-1.key
```

## File Formats

### Key File (`*.key`)

Plain text API key:
```
sk-ant-0123456789abcdef...
```

### Metadata File (`*.meta`)

INI-style format:
```ini
quota_limit=100.0
quota_used=25.5
permissions=600
```

## Permissions

### POSIX Octal Notation

| Octal | Owner | Group | Other | Use Case |
|-------|-------|-------|-------|----------|
| `0600` | rw | --- | --- | Default (secure) |
| `0640` | rw | r- | --- | Team sharing |
| `0644` | rw | r- | r- | Public read |

### Setting Permissions

```bash
# Secure (default)
chmod 600 ~/.cc-switch/acl/anthropic/key-1.key

# Team sharing
chmod 640 ~/.cc-switch/acl/anthropic/key-1.key
chgrp developers ~/.cc-switch/acl/anthropic/key-1.key
```

## Environment Variables (Opt-In)

ModelMux automatically loads keys from environment variables:

```bash
# Set in .bashrc, .zshrc, or shell
export ANTHROPIC_API_KEY=sk-ant-...
export OPENAI_API_KEY=sk-...
export GOOGLE_API_KEY=...
export DEEPSEEK_API_KEY=...
export MOONSHOT_API_KEY=...
export MINIMAX_API_KEY=...
export OPENROUTER_API_KEY=...
```

**Opt-in**: Only loaded if explicitly set. Empty vars are ignored.

## Usage Examples

### Add Key via Filesystem

```bash
# Create provider directory
mkdir -p ~/.cc-switch/acl/anthropic

# Add key
echo "sk-ant-0123456789abcdef" > ~/.cc-switch/acl/anthropic/key-1.key

# Set secure permissions
chmod 600 ~/.cc-switch/acl/anthropic/key-1.key

# Add metadata (optional)
cat > ~/.cc-switch/acl/anthropic/key-1.meta <<EOF
quota_limit=100.0
quota_used=0.0
permissions=600
EOF
```

### Add Key via Environment

```bash
# In .bashrc or .zshrc
export ANTHROPIC_API_KEY=sk-ant-...

# Or temporary for session
export ANTHROPIC_API_KEY=sk-ant-...
modelmux --port 8888
```

### List Keys

```bash
# Via ModelMux logs
modelmux --port 8888 --verbose

# Output:
#   ACL vault: /home/user/.cc-switch
#   Loaded 5 keys
#     Provider 'anthropic': 2 key(s)
#     Provider 'openai': 1 key(s)
#     Provider 'google': 1 key(s)
#     Provider 'deepseek': 1 key(s)
#   ✓ Environment variable: ANTHROPIC_API_KEY
```

### Check Quota

```bash
# View metadata
cat ~/.cc-switch/acl/anthropic/key-1.meta

# Output:
# quota_limit=100.0
# quota_used=25.5
# permissions=600
```

### Reset Quota

```bash
# Edit metadata file
sed -i 's/quota_used=.*/quota_used=0.0/' ~/.cc-switch/acl/anthropic/key-1.meta
```

## Security Considerations

### File Permissions

**Always use secure permissions**:
```bash
# Keys should be readable only by owner
chmod 600 ~/.cc-switch/acl/*/*.key
chmod 600 ~/.cc-switch/acl/*/*.meta

# ACL root should be accessible only by owner
chmod 700 ~/.cc-switch/acl
```

### Environment Variables

**Risks**:
- Visible in `ps aux` output
- Inherited by child processes
- Logged in shell history

**Mitigations**:
- Use `.env` files with `set -a` (not recommended for production)
- Use secret managers (1Password, pass, etc.)
- Use filesystem-based keys for production

### Best Practices

1. **Use filesystem keys for production**
   ```bash
   # Secure and auditable
   echo "sk-ant-..." > ~/.cc-switch/acl/anthropic/prod.key
   chmod 600 ~/.cc-switch/acl/anthropic/prod.key
   ```

2. **Use env vars for development**
   ```bash
   # Quick testing
   export ANTHROPIC_API_KEY=sk-ant-test-...
   modelmux --port 8888
   ```

3. **Set quotas for cost control**
   ```ini
   # ~/.cc-switch/acl/anthropic/dev.meta
   quota_limit=10.0  # $10 limit
   ```

4. **Rotate keys regularly**
   ```bash
   # Add new key
   echo "sk-ant-new-..." > ~/.cc-switch/acl/anthropic/key-2.key
   
   # Remove old key
   rm ~/.cc-switch/acl/anthropic/key-1.key
   ```

## Integration with CC-Switch

CC-Switch can manage ACL keys via Tauri commands:

```rust
#[tauri::command]
pub async fn muxer_add_key(
    provider: String,
    key: String,
    quota_limit: Option<f64>,
) -> Result<String, String> {
    // Create ~/.cc-switch/acl/{provider}/key-{uuid}.key
    // Set permissions to 0600
    // Return key ID
}
```

## Migration from SQLite Vault

If you were using the SQLite-based key vault:

```bash
# Export keys from SQLite
sqlite3 ~/.cc-switch/cc-switch.db "SELECT provider, key_encrypted FROM api_keys;" > keys.txt

# Import to ACL filesystem
while IFS='|' read -r provider key; do
    mkdir -p ~/.cc-switch/acl/$provider
    echo "$key" > ~/.cc-switch/acl/$provider/migrated-$(date +%s).key
    chmod 600 ~/.cc-switch/acl/$provider/*.key
done < keys.txt
```

## Troubleshooting

### Keys Not Loading

```bash
# Check directory structure
ls -la ~/.cc-switch/acl/

# Check permissions
stat ~/.cc-switch/acl/anthropic/key-1.key

# Check ModelMux logs
modelmux --port 8888 --verbose 2>&1 | grep "Loaded"
```

### Permission Denied

```bash
# Fix ownership
chown -R $USER:$USER ~/.cc-switch/acl

# Fix permissions
chmod 700 ~/.cc-switch/acl
chmod 600 ~/.cc-switch/acl/*/*.key
chmod 600 ~/.cc-switch/acl/*/*.meta
```

### Environment Variables Not Loading

```bash
# Check if set
env | grep API_KEY

# Check in ModelMux process
modelmux --port 8888 --verbose 2>&1 | grep "Environment"
```

## Reference

- **POSIX Permissions**: `chmod(1)`, `chown(1)`, `stat(1)`
- **Environment Variables**: `environ(7)`, `getenv(3)`
- **Security**: `secret-storage(7)`, `credentials(7)`
