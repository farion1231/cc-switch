# Remote Access Client Credential Contract (English)

> This is an **external contract** addressed to third-party integrators. The
> Chinese version is [remote-client-auth-zh.md](./remote-client-auth-zh.md); the
> two carry the same content.

## What This Contract Covers

CC Switch's remote mode exposes the local API to other devices on the LAN or over
Tailscale. Any third-party client that calls those APIs must hold and protect a
long-lived credential of its own.

The **server half** is implemented and covered by tests in this repository: the
pairing endpoints issue credentials and the middleware compares digests only. The
**client half** has no code here at all — this repository has no `[[bin]]`, `src-tauri/src/bin`
does not exist, and nothing in the tree reads or writes a keychain or keyring. The
client half is therefore delivered as this document only, with explicit no code
verification: this repository does not implement, compile, or test client-side
storage behaviour, and only pins this document's fixed wording (see
`src-tauri/tests/contract_docs_gate.rs`).

This contract provides no reference client and no correctness-only reference
snippet. A snippet that validates correctness but does not own secure storage is
exactly the kind of thing readers copy uncritically.

## Two Client Carriers

The contract splits clients by whether the carrier can set its own HTTP header.
The two carriers have different storage strategies.

### 1. Programmatic clients (CLI / fetch / curl / server-side programs)

These carriers can set an `Authorization` header, so the credential travels as:

```
Authorization: Bearer <clientToken>
```

The server accepts no other scheme for it. The one-shot polling credential issued
during pairing travels under a different scheme, `Authorization: Pairing <pollToken>`;
the two are not interchangeable.

**Storage policy**: once the plaintext token is issued, outside the client
process's memory it must live in secure storage. On macOS a client should store the plaintext token in the Keychain, and the implementation is to invoke `/usr/bin/security` (this repository introduces no keyring-type crate and therefore does not provide that implementation). Other platforms are left to the integrator's own equivalent; this contract does not mandate one.

### 2. Browser contexts

A browser has no Keychain capability: a web page cannot reach the macOS Keychain,
and no browser API can write to it. The credential should never reach the page
in the first place — the embedded Web UI is third-party content, so handing it a
token hands that token to every script it runs. Such carriers ride the
server-issued **session cookie** instead:

- navigation, subresources and `EventSource` carry it automatically, with no help
  from page script;
- the cookie carries `HttpOnly` (unreadable by page script), `SameSite=Strict`,
  `Path=/` and `Max-Age`;
- the cookie's validity is decided by whether its record is still in the credential
  store; `POST /pairing/revoke` removes the record and it stops working.
- this cookie is accepted on **every** protected route, not only the browser-only
  ones (`/api/events`, `/api/icon` and the like): once a remote browser holds it,
  `fetch('/api/providers')` or `fetch('/api/switch')` from the page rides the same
  credential tier as a programmatic client sending `Authorization: Bearer`. Within
  a single request there is **no fallback between carriers** — an `Authorization`
  header that is present must be the one that verifies; an invalid Bearer plus a
  valid cookie is still 401.

So the **Keychain policy does not apply to browser carriers and must not be
required of them**: the credential is carried by the cookie mechanism, and an
integrator needs no reason — and has none — to persist a plaintext token on the
browser side.

## Prohibited (Both Carriers)

The rule is absolute: a client must never place credentials in a URL, commit them to
a repository, or write them into shell history.

Concretely, all three of these locations are credential leaks under this contract:

- **URLs**: query parameters put the credential into browser history, proxy logs
  and `Referer`. The pairing endpoints read only the `Authorization` header and
  query parameters take no part in authentication — by the time a token appears
  in a URL it has already leaked.
- **Repositories**: `.env` files, config files, sample code, commit history.
- **Shell history**: command-line arguments and inline environment assignments are both recorded.

Credentials should flow only between memory and the secure storage described
above. Logs, error messages and crash reports must not contain the plaintext token
either.

## Binding Scope and Two-Phase Confirmation

Which addresses the server listens on is decided by a three-tier binding scope
(listed narrow to wide):

| Tier | Listen addresses | Who can connect |
|---|---|---|
| `loopback` | `127.0.0.1:<port>` | this machine only |
| `tailscale` | `127.0.0.1:<port>` + the Tailscale IP | this machine and the tailnet |
| `allInterfaces` | `0.0.0.0:<port>` | anything that can reach this machine's LAN / Tailscale address |

**Narrowing** (moving left) applies immediately. **Widening** (moving right) is
two-phase:

1. `POST /api/remote/config/bind-scope/request` (body `{ "scope": "..." }`) only
   registers a pending request and returns
   `{ "confirmationId": "...", "expiresAt": <unix seconds> }`; configuration and
   listeners do not change;
2. `POST /api/remote/config/bind-scope/confirm` (body `{ "confirmationId": "..." }`)
   is what commits, persists and rebinds the listeners.
   `POST /api/remote/config/bind-scope/reject` cancels a pending request.

All three endpoints are `Protected`: **requests from non-loopback sources still
require a pairing credential**. A confirmation id is single-use, expires after
five minutes, and can only be redeemed by **the same authenticated session that
requested it** (Bearer and cookie are distinct session digests). The server has
**no** endpoint that directly sets the binding scope.

Honest boundary statement: against a fully compromised, already-paired client,
two-phase confirmation raises the bar and leaves an audit trail — it is not a
hard boundary. It stops a single line of renderer code from widening the scope;
it does not stop an attacker holding a valid credential from walking through both
steps. The only hard boundary would be "you must go back to that machine and
click a native dialog", which this design deliberately did not choose.

## Boundaries

- This document describes what **integrators must do**; it is not a description
  of server behaviour. The server's behaviour is defined by the code and by
  `ROUTE_SCOPE_TABLE`.
- Revocation (`POST /pairing/revoke`) only removes the record from the server-side
  credential store. Clearing the client-side plaintext copy is the integrator's
  job — this contract offers no remote wipe of client storage, and no endpoint
  can do it.
- This repository does not verify that a client actually follows the policies
  above. Apart from a rejected request, nothing detects an integrator violating
  this contract, so storage and transport security are the integrator's own
  responsibility.

## Related

- Server-side implementation: `src-tauri/src/remote/auth/`, `src-tauri/src/remote/pairing/`
- The static gate for this contract: `src-tauri/tests/contract_docs_gate.rs`