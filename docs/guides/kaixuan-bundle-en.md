# kaixuan Bundle One-Click Setup Guide (en)

> Applies to: v3.20.4+ (feat: kaixuan bundle)
> In one sentence: once `~/.codex` is set up, a "kaixuan bundle" card appears at the top of the cc-switch UI; clicking "Install/Reinstall" wires both the Kaixuan kxpms.cn gateway and the local 8782 gateway into Codex at once and enables failover.

## 1. Prerequisites

### 1.1 Public gateway (Kaixuan kxpms.cn)

The remote gateway runs as a kx-citus-pg17 + llm-gateway-go container pair, deployed behind 47.97.111.154 (NPS+VPN). All you need is for `https://llm.kxpms.cn/v1/models` to be reachable:

```bash
curl -s -o /dev/null -w "%{http_code}\n" --max-time 5 https://llm.kxpms.cn/v1/models
# Expected: 401 (gateway is up but the key is missing) or 200
```

### 1.2 Local gateway (127.0.0.1:8782)

Run llm-gateway-go locally. Three deployment methods, by priority:

| Method                              | Path                                                                            | Trigger                                                                                |
| ----------------------------------- | ------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| **(a) Custom command**              | `KAIXUAN_GATEWAY_START_CMD` environment variable                                | cc-switch runs `sh -c "$KAIXUAN_GATEWAY_START_CMD"` when you press "Restart 8782"      |
| **(b) Conventional project script** | `~/kaixuan/llm-gateway-local/start.sh`                                          | Executed when the file exists; `chmod +x` if it is not executable                      |
| **(c) Docker fallback**             | `docker run -d --name llm-gateway-local-8782 -p 8782:8782 llm-gateway-go:local` | Used when neither of the above is found; requires a local `llm-gateway-go:local` image |

### 1.3 Verify the local gateway

```bash
curl -s -o /dev/null -w "%{http_code}\n" --max-time 3 http://127.0.0.1:8782/v1/models
# Expected: 200
```

## 2. In cc-switch

### 2.1 One-click install

1. Open cc-switch and switch to the **Codex** app tab.
2. The "kaixuan bundle" card appears at the top.
3. Enter the **primary endpoint API key**:
   - Placeholder form: `$KAIXUAN_KXPMS_KEY`
   - Or plaintext form: `sk-live-xxxxx` (not recommended, it lands in the DB)
4. The **secondary endpoint API key** may be left empty (reuses the same key) or set to an independent key.
5. Click "Install/Reinstall".

The backend then:

- Writes two providers: `kaixuan-kxpms` (primary) and `kaixuan-local-8782` (secondary)
- Adds both to the failover queue with an incrementing `sort_index`
- Enables proxy takeover for that AppType and sets `auto_failover_enabled=true`
- Switches to P1 = `kaixuan-kxpms`

All of these writes happen inside a **single database transaction**. If any step fails the whole batch rolls back, so a failed install never leaves a half-installed state (provider written but not enqueued).

### 2.2 Status display

The card shows two health lights:

| Endpoint     | Healthy                | Unhealthy               |
| ------------ | ---------------------- | ----------------------- |
| `kxpms`      | <Wifi/> + `200 · 35ms` | <WifiOff/> + error text |
| `local-8782` | same                   | same                    |

They are polled every 15 seconds, and refreshed immediately when the window regains focus. When 8782 is unreachable, press the ▶ button on the right of the 8782 row to start it.

## 3. Troubleshooting

### 3.1 Bundle install fails with "bundle unknown"

```bash
# Check whether the backend registered it
sqlite3 ~/.cc-switch/cc-switch.db "SELECT 1;"
# Reopen cc-switch so startup runs, or restart the app
```

### 3.2 Health light stays red although curl works

Check whether the cc-switch process inherited the environment:

```bash
ps eww -p $(pgrep -f "cc-switch") | tr ' ' '\n' | grep -i "kaixuan\|http_proxy"
# macOS GUI apps do not read ~/.zshenv by default; use launchctl setenv (see the cc-switch-env-vars doc)
```

### 3.3 Health light is yellow/red but the endpoint actually works

reqwest defaults to a 200 ms connect timeout and a 500 ms total timeout, so public-network congestion occasionally trips it. Tune the timeout in `src-tauri/src/gateway_health.rs::shared_client`:

```rust
Client::builder()
    .timeout(Duration::from_millis(2000))  // raise it
    .connect_timeout(Duration::from_millis(1000))
```

### 3.4 Local gateway still unreachable after starting it

1. Check whether the start command really ran: the `stdout` / `stderr` returned by `start_local_gateway`
2. Check the container: `docker ps | grep llm-gateway-local-8782`
3. Check the container logs: `docker logs llm-gateway-local-8782`

## 4. Extending: adding another bundle

Add a line in `src-tauri/src/provider_bundle.rs::bundles()`:

```rust
pub fn bundles() -> Vec<BundleSpec> {
    vec![kaixuan_bundle(), my_other_bundle()]
}
```

The UI side `useKaixuanBundles()` already returns every bundle, so no UI change is needed.

## 5. Port override and cross-platform notes

### 5.1 Local gateway on a port other than 8782

```bash
# Export before starting cc-switch; the kxpms endpoint is unaffected
export KAIXUAN_LOCAL_GATEWAY_PORT=8899
launchctl setenv KAIXUAN_LOCAL_GATEWAY_PORT 8899  # needed for the macOS GUI app too
open -a cc-switch
```

The bundle install uses `http://127.0.0.1:8899/v1` as the base_url, and the start entry point uses port 8899. The card shows the actual probed port, so the UI never claims "8782" while probing a different one.

### 5.2 Windows

`start_local_gateway` goes through a shell; on Windows it automatically uses `cmd.exe /C` instead of `/bin/sh`. However, the `~/kaixuan/llm-gateway-local/start.sh` path is meaningless on Windows — Windows users should use the `KAIXUAN_GATEWAY_START_CMD` custom command, or rely on the docker fallback (`docker run -d --name llm-gateway-local-8782 -p 8782:8782 llm-gateway-go:local`, which requires Docker Desktop).

## 6. Relationship to the existing standalone presets

|                  | Old approach                                                                                 | New kaixuan bundle                                                                                        |
| ---------------- | -------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------- |
| UI steps         | Pick the "Kaixuan LLM Gateway" preset and the "Local LLM Gateway (8782)" preset — two clicks | One click on "Install/Reinstall"                                                                          |
| Failover         | Manually add both to the queue                                                               | Automatic enqueue + auto_failover enabled                                                                 |
| Default model    | Two independent `default` toggles                                                            | P1 = kxpms after install                                                                                  |
| Config overwrite | Reinstall overwrites the user's edited API key                                               | When the same id already exists, the user's `settings_config` is preserved (only membership is refreshed) |

The two old standalone presets (`开轩 LLM 网关` / `本地 LLM 网关 (8782)` in `codexProviderPresets.ts`) are kept and can still be used one at a time; they do not conflict.

## 7. Switching between the two endpoints

Each endpoint has its own TOML table id (`kxpms` and `local8782`) plus a top-level `model_provider` pointing at the active one, so switching never collides and both tables stay in the live config. Switching writes the new config without breaking the file; if Codex is already running, cc-switch offers to restart it so the running session picks up the new provider too.

Models from both endpoints appear together in the `/model` picker as `claude-opus-5` (active endpoint) and `claude-opus-5@kxpms` / `claude-opus-5@local8782` (the other endpoint). The `@`-suffixed entries make the other endpoint's catalog _visible_; actual request routing is still decided by the top-level `model_provider`.
