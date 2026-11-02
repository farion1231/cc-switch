## Summary

- move email-change access into the personal profile section
- remove the standalone email tab/page while preserving a legacy redirect
- remove the delete-row icon and the bug-report action below the security section

## Verification

- `git diff --check` — passed
- targeted ESLint for all changed TypeScript/TSX files — passed
- TypeScript — blocked by the same pre-existing syntax errors on `origin/main` in `src/lib/i18n/translations/en/affiliate.ts`
- i18n check — blocked by the pre-existing repository-wide missing-key baseline

## Safety

- branch created from current `origin/main` (`94f842132`)
- no force push and no unrelated local work included
