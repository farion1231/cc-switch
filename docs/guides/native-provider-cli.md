# Native provider CLI

`cc-switch-cli` is a console entry point built from the same Rust source as the
desktop. It inspects an existing desktop configuration and switches saved Claude
Code, Codex, and Gemini CLI providers through `ProviderService::switch`.

Initialize your providers in the matching desktop version first. The CLI does
not import first-run configurations, seed providers, or migrate databases on
read. It requires the database schema of the source version you build.

## Build

Use the development prerequisites in [CONTRIBUTING.md](../../CONTRIBUTING.md).
From the repository root:

```sh
pnpm install --frozen-lockfile
pnpm build:renderer
cargo build --manifest-path src-tauri/Cargo.toml --bin cc-switch-cli --release --locked
```

The binary is `src-tauri/target/release/cc-switch-cli` (`cc-switch-cli.exe` on
Windows). Add its directory to your `PATH` if desired. This change adds a source
build target; it does not add the CLI to desktop installers or release archives.
The CLI does not open a window or start the desktop proxy, but still builds
against the desktop library and its platform dependencies.

## Daily commands

```sh
cc-switch-cli status
cc-switch-cli --app claude list
cc-switch-cli --app codex current
cc-switch-cli --app codex config
cc-switch-cli --app gemini config PROVIDER_ID
cc-switch-cli --app claude use PROVIDER_ID --dry-run
cc-switch-cli --app claude use "Provider name"
cc-switch-cli --app codex --json list
cc-switch-cli --config-dir "PATH/TO/CC-SWITCH-DATA" status
```

`--app` defaults to `claude`, except `status`, which defaults to all supported
applications. Read commands can inspect all applications. Coexist-mode tools
have no single current provider, so use `list` or `config PROVIDER_ID` for them.
`use` supports only `claude`, `codex`, and `gemini` in this first version.

Provider selection uses an exact ID first, then a unique name (ignoring ASCII
case and surrounding whitespace). Duplicate names require an ID. An unknown
provider returns an error before writing anything. `--dry-run` previews the
selection and live file paths without writing or backing up. Reselecting the
current provider is also a no-op; it does not reapply a drifted live file.

For a custom desktop **data directory**, explicitly pass `--config-dir`. The CLI
does not read Tauri's GUI store. Device settings (including application directory
overrides) and live state still use the machine-local `~/.cc-switch` directory,
just as in the desktop; `--config-dir` changes the shared database location.

## Output and switching

`list` prints IDs and names, marking the current provider with `*`; `--json`
returns an array of provider summaries. Other commands return JSON objects.
Summaries expose only the app, ID, name, current flag, endpoint origin, and model.
The endpoint origin excludes credentials, paths, queries, and fragments. Raw
configuration, authentication fields, and service warning text are not printed.
`config` is a safe summary and path inspection, not a credential export.

`status` describes saved direct/proxy selections. It does not check whether the
desktop or proxy process is running or whether the endpoint is reachable.
Proxy mode reports the saved route through the desktop's current-provider rules.
An unreadable routing state or settings file returns an error.

Before an actual switch, leave local routing for that application in the desktop
and close the desktop so its cached settings cannot conflict with the CLI. The
CLI refuses to change a saved proxy-mode selection. Run one writer at a time;
it does not coordinate with other CLI processes or the running desktop.

Each actual switch creates `~/.cc-switch/backups/cli-<UUID>/` on this machine, containing
a SQLite snapshot plus the previous device settings, live state, and relevant
live files. `manifest.json` maps each `file-N` to its original path; a `null`
saved value records that the file did not exist. These backups contain secrets,
remain local, and are not subject to automatic backup rotation. Keep them private
and manage their retention yourself. On Unix, the backup directory is private
(0700); on Windows it inherits the local device directory's permissions.

If switching fails after backup, the error reports the backup location for
inspection. There is no automatic backup restore command. Normal switching,
backfill, auth preservation, atomic live writes, and recovery follow the existing
desktop service. After switching, follow each client's normal restart guidance.

Success exits with code `0`; operational errors use `1`; invalid command syntax
uses Clap's exit code `2`. Errors are written to stderr.

## Validation

The subprocess tests in `src-tauri/tests/native_cli.rs` use isolated test homes
and synthetic credentials. They cover unchanged files during reads and dry runs,
stale local pointers, saved proxy routes, rejected writes, schema guards, real
switches for all three applications, backup contents, and no-op reselection.
Windows is the locally tested platform for this contribution.
