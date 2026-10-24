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
