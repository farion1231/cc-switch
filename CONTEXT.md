# Provider Configuration

This context defines ownership boundaries between provider records managed by CC Switch and runtime configuration files owned by supported client applications.

## Language

**Provider Stored Configuration**:
The provider record saved by CC Switch. It is the source of truth for credentials assigned to a third-party provider.
_Avoid_: Live configuration, active client configuration

**Live Configuration**:
The active configuration consumed by Codex, Claude, Gemini, or another supported client. It may contain projections and client-owned state, so it is not authoritative for third-party provider credentials.
_Avoid_: Provider stored configuration, provider record

**Third-Party Provider Credential**:
An API key and upstream Base URL explicitly assigned to a third-party provider. It changes only through a user-confirmed credential operation.
_Avoid_: Official login material, OAuth session

**Official Login Material**:
OAuth or account-session data owned by an official client or a managed account. It remains separate from third-party provider credentials.
_Avoid_: Provider API key

**Explicit Credential Import**:
A user-confirmed operation that adopts selected credential differences from Live Configuration into Provider Stored Configuration.
_Avoid_: Automatic backfill, silent synchronization

**Credential Audit Record**:
A redacted record of when a provider credential changed, which source initiated the change, and which credential fields changed. It identifies values by fingerprint and never contains recoverable credential text.
_Avoid_: Rollback snapshot, credential backup

**Provider Rollback Snapshot**:
A short-lived, local copy of a provider's previous stored configuration used only for explicit rollback. It is separate from Credential Audit Records and is not part of cloud synchronization.
_Avoid_: Audit record, Live backup

**Configuration Inconsistency**:
A protected state in which Provider Stored Configuration and Live Configuration could not be brought back into agreement after a failed credential operation. It applies to one supported client and prevents further configuration writes until explicit recovery succeeds.
_Avoid_: Provider outage, request failure
