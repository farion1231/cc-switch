# Continuing native Responses history across backends

This increment builds on the built-in identity / unified history fix. Keeping
the same provider bucket is necessary for sidebar visibility, but backend-scoped
native `web_search_call` handles can still prevent the same-model conversation
from continuing after switching between an API route and official OAuth.

When unified history is enabled, the proxy projects historical web-search
actions into readable assistant activity messages. It handles outgoing request
history and incoming native Responses JSON and SSE. New API search items are
therefore stored in a portable representation before a later official direct
session resumes them. Ordinary messages, function calls/results and encrypted
reasoning remain untouched. Completion events and usage remain available.

The opted-in v2 legacy migration uses the same projection for known shared
`custom` histories while moving them to `openai`. Original rollout files and
SQLite state are backed up; unrelated histories and already-official rollouts
are not rewritten. The backup preserves native records, but this adapter cannot
export hidden search context held only by the issuing backend. Portable messages
record past activity and are not represented as fresh search results.

This is not a guarantee for arbitrary upstreams, account changes or different
model versions. Real desktop continuation still needs acceptance testing with
the selected upstream and model.

The request and response adapters use one request-scoped unified-history snapshot, including retries. Failed or unknown search activity preserves its visible action and explicitly records the status; it is not presented as a successful new search. UTF-8 SSE fragments, event IDs, completion, usage, ordinary function calls/results and encrypted reasoning are covered by focused regressions. The existing Copilot ID stabilization and late-argument repair run before portable response projection; xAI namespace/integer normalization stays scoped to the actual resolved xAI model.

The combined retry regression sends foreign historical reasoning, native search activity, a real function-call/result pair and a current encrypted agent task to a loopback server. An HTTP-200 SSE opaque rejection strips only eligible historical state, projects history on each attempt, preserves function semantics and the current task, and makes at most one recovery attempt. A second task rejection is surfaced rather than hidden or retried indefinitely. No ToolSearch, plaintext-delegation, catalog or Windows repair changes are included.
