# SSH remote provider management

Manage Claude Code, Codex and Gemini CLI configurations on an SSH server from
CC Switch. Direct configuration writes and routing through the local gateway
are two available ways to use a local provider remotely.

## Requirements

- An OpenSSH client (`ssh`) on the machine running CC Switch.
- A reachable remote account with a POSIX shell and writable client config
  directories. Remote Windows shells are not supported.
- For the gateway, key-based SSH authentication and an SSH server that permits
  reverse TCP forwarding. Test the SSH alias in a terminal first.

## Inspect, import and switch

1. Select Claude Code, Codex or Gemini CLI, then open **Remote SSH config**.
2. Choose a concrete Host alias from `~/.ssh/config`, including supported
   Include files, or enter a host, username and port manually. A manual
   password is used for the connection and is not saved in the database.
3. Connect to inspect the remote configuration. **Sync locally** imports it as
   a local provider without changing the remote files.
4. Select a local provider and confirm the switch. Provider fields are patched
   while remote MCP servers, project trust and other unmanaged settings are
   retained. Codex retains its remote login and custom provider identifier.
5. Reopen the remote client to load the new configuration. **Stop processes**
   can stop the current user's matching client processes, including IDE
   extension processes. It interrupts their conversations and may force-kill
   processes that do not exit. It does not launch them again automatically.

| Client      | Remote files                                                                          |
| ----------- | ------------------------------------------------------------------------------------- |
| Claude Code | `~/.claude/settings.json`                                                             |
| Codex       | `~/.codex/config.toml`, `~/.codex/auth.json`, and a managed model catalog when needed |
| Gemini CLI  | `~/.gemini/.env`, `~/.gemini/settings.json`                                           |

Writes use a temporary file and rename for each file. There is no automatic
remote backup or transaction spanning multiple files; keep a separate backup
before applying changes. Importing a provider is not a full config backup.
Connection establishment has a timeout, but a stalled remote command does not
yet have an overall timeout or a cancel action.

## Route through the local gateway

Enable **Use local gateway** after connecting with key-based SSH. CC Switch
starts its local routing service and a token-authenticated loopback gateway.
An `ssh -R` tunnel exposes the gateway on the remote loopback interface and
writes the corresponding gateway address and token into the client config.
Reopen the remote client once to load this configuration.

Each host and client can follow its local route or pin a provider. Subsequent
provider switches are handled locally. A pinned provider disables failover;
aggregation model selection cannot bypass that pin. Following a local route
retains its resolved aggregation model selection. Providers that require an
official subscription login on the local machine cannot be used remotely.

Keep this machine online and CC Switch running. The tunnel supervisor reports
connection status and retries disconnected tunnels; **Reconnect** also allows
an explicit retry. Enabled routes are restored at startup and keep the local
routing service running. On shutdown, their tunnels stop. To return to direct
connections, disable the gateway and choose the provider to write remotely,
then reopen the client.

Gateway traffic appears in local usage accounting with its remote host label.
This does not import usage from remote session files. Gateway targets, tokens
and route records belong to this machine and are excluded from cloud sync;
full database backups retain them. Password connections cannot enable the
gateway, and passwords are excluded from serialized targets.

## Verification

Local tests cover host parsing, field preservation, remote shell write/read
scripts, gateway authentication and routing, sync exclusions, and navigation
for all three clients. Native Windows SSH/ASKPASS behavior and interactive
client workflows require platform-specific manual verification.

The real SSH tunnel integration test is opt-in. It creates a temporary
loopback reverse tunnel and issues remote `curl` requests against a stub local
router; it does not alter remote client configuration. Use a dedicated SSH
test account whose config alias is available locally and that has `curl`:

```sh
cd src-tauri
CC_SWITCH_GATEWAY_TEST_HOST=your-test-alias \
  cargo test reverse_tunnel_reaches_local_listener -- --ignored --nocapture
```

Use `CC_SWITCH_TEST_HOME` pointing at a temporary directory for the full
backend test suite, as described in `CONTRIBUTING.md`.
