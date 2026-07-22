# CC Switch Domain Language

CC Switch manages reusable AI tool providers while preserving the local state of every environment in which those tools run.

## Language

**Application**:
An AI tool family managed by CC Switch, such as Codex, Claude Code, or Gemini CLI.
_Avoid_: Client, tool instance

**Provider**:
A reusable definition of how an Application reaches a model backend, including routing, credentials, models, and protocol behavior.
_Avoid_: Environment, account, complete config file

**Environment**:
A user-visible place where an Application is installed and owns independent configuration, authentication, and history, such as Windows or a WSL user account.
_Avoid_: Directory, machine profile, instance

**Managed Target**:
CC Switch's registered representation of one Application in one Environment.
_Avoid_: Environment variable, config directory override

**Target Override**:
An explicit Environment-specific replacement for a value inherited from a Provider.
_Avoid_: Provider copy, silent local patch

**Managed Field**:
A configuration value owned by a Provider and eligible for projection into a Managed Target.
_Avoid_: Entire configuration file

**Provider Key**:
An Application-native identifier selected by a Managed Target to activate one Provider route. For Codex, CC Switch generates a readable, collision-resistant `cc_switch_<name>_<id>` key for non-official Providers; the official Provider uses Codex's native route without a custom key.
_Avoid_: Provider display name, generic `custom` bucket

**Managed Provider Table**:
The Application-native routing table selected by a Provider Key and owned by CC Switch while it is active. Reprojection replaces stale CC Switch tables and collapses aliases of the same route while retaining their unknown fields.
_Avoid_: Every provider table in a Target config, user-authored inactive route

**Local Field**:
A configuration value owned by an Environment and preserved across Provider changes.
_Avoid_: Common Provider setting

**Projection**:
The planned application of a Provider's Managed Fields, plus Target Overrides, onto a Managed Target while retaining its Local Fields.
_Avoid_: Full-file synchronization, directory copy

**Drift**:
An external change to a Managed Field after CC Switch last projected it.
_Avoid_: Any local configuration change

**Session Provenance**:
The known or user-confirmed Environment and Provider that originally created a session.
_Avoid_: Current Provider, session bucket

**Unmanaged Environment**:
A registered Environment whose existing Application configuration has not yet been associated with or replaced by a CC Switch Provider.
_Avoid_: Broken Environment, unsupported Environment

## Implemented Boundary

The current Codex adapter claims only an explicit routing/model whitelist: the
active `model_provider`, its active `model_providers` table, model selection and
capability fields, Provider endpoint/protocol fields, and the Provider-scoped
bearer token. Paths, projects, approval/sandbox policy, MCP, response-storage
policy, authentication files, sessions, state databases, and unknown fields are
Target-owned. Adding or linking a Target is read-only; the first Projection
requires explicit activation. Official Codex Providers project to the native
Codex route. Non-official Providers receive a unique readable Provider Key;
generic `custom` is not a Managed Target identity.
