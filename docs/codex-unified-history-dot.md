# Built-in Codex identity for unified history

Unified history keeps official accounts on the built-in `openai` identity for
native account features such as Dot. Compatible API proxy routes use that identity
with a loopback `openai_base_url`. The existing upstream credential replacement and
official account-header/internal-metadata stripping remain in force; no provider
API key is written into `auth.json`.

API native identity is provisional until the write-locked auth plan confirms a
surviving **file** login. API-only users, managed-account switch-away that clears
the login, and unproven keyring/auto/ephemeral stores retain the custom proxy table
and placeholder. Explicit compaction, request/stream retry or idle timeout settings,
and the legacy OpenAI-name compaction opt-out also retain custom. Thus these
fallback routes cannot promise the same history bucket or Dot. Client settings
are preserved rather than silently dropped. An active custom profile retains its
selector; conflicting profile-owned endpoints/tokens are still rejected before
writing. Strict API direct mode stays separate.

Pause official login and unified history are mutually exclusive. Saves reject a
conflicting pair before changing settings or live files. For older saved conflicts,
unified history has priority; the pause route/forwarding/model checks use the same
effective policy. The UI explains this and permits disabling the active option.
The forwarder snapshots both flags once per incoming request, so retries cannot
pick up a different identity policy halfway through a request.

Legacy `custom` history can move to `openai` only after a separate, unchecked-by-
default opt-in. Original JSONL/SQLite records are backed up under a v2 generation;
old v1 ledgers and unrelated providers remain intact. The operation lock guards
the live/profile check and migration, directory/target markers prevent repeated
or reverse migration, and rollout rewriting preserves mtime and detects concurrent
file changes. A custom fallback live route does not trigger the migration. Disabling
never automatically moves v2 history back. Old v1 official-history restores remain
separate and refuse to run while unified history is enabled.

This PR changes identity and visibility, not encrypted reasoning or tool semantics.
A dependent PR (#8641) projects backend-scoped native search records into historical
activity text. It cannot export hidden backend search state. Neither backend tests
nor prior private-build acceptance establish real desktop Dot/account entitlement
or arbitrary cross-backend continuation on this focused branch.
