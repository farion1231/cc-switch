---
status: accepted
---

# Keep third-party provider credentials database-authoritative

CC Switch treats each third-party provider's API key and upstream Base URL in Provider Stored Configuration as authoritative because automatic Live-to-database backfill can associate client-side state with the wrong provider. When Live Configuration differs, the stored credential wins and normal operations continue with a visible warning; a missing or invalid stored credential blocks only the affected operation until the user explicitly edits it or performs an Explicit Credential Import. Official OAuth and account-session material remains client- or managed-account-owned and is excluded from this rule.
