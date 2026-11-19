# Claude Desktop account history

On macOS, open **Desktop history** from the Sessions menu or the Desktop history note. Choose an existing account/organization in the normal Claude or managed Claude-3p data root. The last-known account label comes from Desktop metadata; it is not proof of a current login.

The history list starts with active Code records. **Include archived history** also shows archived records, honoring both `isArchived` and the legacy version-1 `archived-sessions.idx`. Account-scoped `deleted_*` markers exclude records across organizations. Malformed or unreadable archive/deletion state produces a warning instead of exposing possibly deleted history.

Each record links its prior, pre-clear, unarchived, and current CLI transcript references to files in the configured Claude projects directory. Missing files are labeled missing, and duplicate matches are labeled ambiguous. The installed Desktop format includes `preClearCliSessionId`, which the reference reader did not handle. SSH, WSL, and VM references are not treated as local files.

Available links open individual files in CC Switch's existing transcript reader, with export available and destructive/resume actions disabled. This does not concatenate transcripts, repair native Desktop history, switch accounts, or launch a CLI session. Generic CLI history remains separately accessible.

## Validation

The format was checked read-only against Claude Desktop 1.46388.4 on macOS arm64. Synthetic tests cover profile separation, archives and account tombstones, full transcript lineage, missing/ambiguous links, owner checks, malformed state, symlink rejection, and the read-only reader. Live Desktop files were not changed. Windows and Linux UI support is not claimed.

Format sources and license notices are in [desktop-format-provenance.md](desktop-format-provenance.md).
