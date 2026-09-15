# Unified Codex Session History: Feature Overview and Usage Guide (CC Switch)

> Applies to CC Switch v3.16.x and later. This guide is based on the current code; every command and path can be verified by hand. Examples use de-identified data and contain no real session content or API keys.

## What this feature is

"Unified Codex session history" is a switch that CC Switch v3.16.x adds for Codex. You'll find it under **Settings -> General -> the "Codex App Enhancements" group** ("Codex App Enhancements" is the group title; the switch itself is called "Unified Codex session history"). Once enabled, **sessions from your official subscription (ChatGPT login / OpenAI API key) appear in the same history / resume list as sessions from every third-party provider CC Switch manages**—they are no longer split into two lists that can't see each other.

## What problem it solves

Codex classifies sessions by a "provider tag" (a field called `model_provider`), and **the resume / history list only shows sessions whose tag matches your currently active provider**. As a result, sessions are naturally sorted into two separate "drawers":

- Sessions from your official subscription go under Codex's built-in **`openai`** tag;
- Every third-party provider CC Switch manages goes under the **`custom`** tag.

The two drawers can't see each other. If you **switch frequently between official and third-party**, you'll hit this kind of fragmentation: "the session I was just chatting in with the official account disappeared from the history list after I switched to a third-party provider"—it isn't actually gone, it's just been sorted into the other drawer. This split both makes it easy to believe a session was lost, and makes it inconvenient to review and resume all your sessions in one place.

**This switch exists to eliminate that fragmentation**: it makes the official subscription run under the `custom` tag too, so official and third-party sessions merge into one list and everything is easy to find and resume in a single place.

> ✅ **One important premise that runs through this whole guide, please remember it first**: this feature (unify / migrate / restore) **only ever rewrites that one classification tag `model_provider` in your session records, and it automatically makes a backup of the original file before every rewrite**. It never deletes, clears, or overwrites a single line of your conversations. So whenever this guide later mentions "some sessions are no longer visible," it almost always means "they've been sorted into the other drawer," not "the data is gone." If you're truly worried, jump straight to the [symptom reference table](#i-feel-like-my-sessions-are-gone-symptom-reference-table) and [verify the files are still there by hand](#verify-by-hand-your-session-files-are-still-on-disk-the-most-important-section).

## How it works (one-line version)

Think of it as **two drawers + automatic backup**:

- By default, official sessions live in the `openai` drawer and third-party sessions live in the `custom` drawer, invisible to each other;
- The switch makes **the official side use the `custom` drawer too**, merging the two drawers into one shared list;
- You can optionally choose to "move" your **existing official sessions** into the shared drawer as well (this step is called **migration**; it's optional and requires you to opt in by checking a box), and **before anything is moved a backup copy is made first**, so the whole process is **reversible**;
- **Authentication is completely unaffected**—your official subscription still uses your ChatGPT login and still goes through the official backend; only the session's classification tag changes.

For the full mechanism (what gets injected, why it's reversible, how migration / restore guarantee no data loss) see [The core mental model](#the-core-mental-model-two-drawers--automatic-backup) and the [Advanced mechanism appendix](#advanced-mechanism-appendix-for-users-who-want-to-truly-understand-how-it-works) at the end.

## How to use it (at a glance)

1. **Enable**: Settings -> General -> Codex App Enhancements -> turn on "Unified Codex session history" -> in the dialog decide whether to check "Also migrate existing official session history" (check it if you want your **earlier** official sessions merged into the unified list too; leave it unchecked if you only want unification from now on) -> confirm. See [What happens when you enable it](#what-happens-when-you-enable-it-step-by-step).
2. **Disable**: turn the same switch off -> in the dialog keep "restore exactly from backup" checked (it's checked by default) -> confirm, and the official sessions you migrated in will be precisely flipped back to the official list. See [What happens when you disable it](#what-happens-when-you-disable-it-step-by-step).
3. **Feel like a session is gone?** Don't panic—jump to the [symptom reference table](#i-feel-like-my-sessions-are-gone-symptom-reference-table) to locate it by symptom, and use the commands in the [verify by hand](#verify-by-hand-your-session-files-are-still-on-disk-the-most-important-section) section to see for yourself that the files are all there.

---

## The core mental model: two drawers + automatic backup

To understand this feature, you only need to remember two things: **drawers** and **backups**.

### Drawers: how Codex classifies sessions

Every time you start a Codex session, Codex records a tag `model_provider` in the session file header, marking "which provider this session was chatted with." Codex's **resume / history list is filtered precisely by the currently active tag**—it only shows sessions whose tag matches "the provider you're on right now."

- Sessions from your official subscription (ChatGPT login / OpenAI API key) carry the built-in tag **`openai`**.
- Every third-party provider CC Switch manages uses the tag **`custom`**.

So by default, official sessions and third-party sessions are inherently invisible to each other—they live in two different drawers. This is **Codex's own design**, not CC Switch losing anything.

```text
Default state (unified switch off):
   ┌───────────────────────┐     ┌──────────────────────────┐
   │  openai drawer        │     │  custom drawer           │
   │  (official sessions)  │     │  (third-party sessions)  │
   └───────────────────────┘     └──────────────────────────┘
       ▲                             ▲
     visible only while            visible only while
     on the official provider       on a third-party provider

   The two drawers can't see each other.
```

**What the "Unified Codex session history" switch does is make the official subscription run under the `custom` tag too, merging the two drawers into one**, so official and third-party sessions appear in the same resume list. Note: **authentication doesn't change**—your official subscription still uses your ChatGPT login and still goes through the official backend; only the session's "classification tag" changes from `openai` to `custom`.

```text
After the unified switch is on:
   ┌──────────────────────────────────────────────┐
   │             custom shared drawer             │
   │  official sessions  +  third-party sessions  │
   │  (appear in the same history / resume list)  │
   └──────────────────────────────────────────────┘
```

### Backups: a copy is made before every tag change

"Merging the drawers" requires changing the tag of some official sessions from `openai` to `custom` (this step is called **migration**, and it's **optional and requires you to opt in**). And **before any rewrite, CC Switch first copies the original file untouched** to here:

```text
~/.cc-switch/backups/codex-official-history-unify-v1/<timestamp>/
```

This backup is the sole basis for "restore exactly from backup" later. It makes the whole process **reversible**: at any time you can turn off the switch and precisely flip the official sessions you migrated in back to the `openai` drawer.

Remember these two words—**drawer** (a session just gets reclassified) and **backup** (a copy is always made before a change)—and everything that follows will be easy to understand.

---

## What happens when you enable it: step by step

### Step 1: Find the switch

```text
Settings -> General -> Codex App Enhancements
```

In the "Codex App Enhancements" block there are two rows of switches; the **second row** (the blue history icon) is the subject of this guide:

> **Unified Codex session history**

Below it is a line of description text (verbatim):

> When enabled, the official subscription runs under the shared "custom" provider id so official and third-party sessions appear in one history list, optionally migrating existing official sessions in (backed up first). When turning it off, the migrated sessions can be restored from backup. Note: resuming an old session across providers may fail because its encrypted_content reasoning can only be decrypted by the backend that created it.

> **Note**: this single line of description already previews three things—sessions will appear in one list, you can optionally migrate them in with an automatic backup, and resuming across providers "may fail." Here, "fail" means **you can't resume / can't generate a new turn**, not "the record is lost." This is exactly the core misunderstanding we'll dig into below.

### Step 2: Flip the switch from off to on -> a confirmation dialog pops up

The moment you flip the switch on, CC Switch **does not save immediately**; instead it first pops up a confirmation dialog. The dialog text reads as follows (verbatim):

- **Title**: Unified Codex session history
- **Body**:

  > When enabled, the official subscription and third-party providers share one session history list. Note: resuming an old session across providers may fail because its encrypted_content reasoning cannot be decrypted by another backend.
  >
  > You can also migrate your existing official session history into the shared list (originals are backed up to ~/.cc-switch/backups first and can be restored when you turn this off).

- **Checkbox**: Also migrate existing official session history
- **Confirm button**: I understand, enable
- **Cancel button**: Cancel

**This checkbox is unchecked by default.** This is an important fork in the road:

| Your choice | Effect | Where your data is right now |
|---|---|---|
| **Unchecked** (default) | Only switches the tag. **Only official sessions created after enabling** land in the `custom` shared drawer | Your official sessions from **before** enabling keep the `openai` tag, stay exactly where they were, still in `~/.codex/sessions/` |
| **Checked** | In addition to switching the tag, also migrates your **existing official sessions** from the `openai` drawer into the `custom` drawer | After being **copied to backup**, the old sessions' tag is rewritten to `custom`; the original data is covered by the backup |

> **If you want "my earlier official sessions to appear in the unified list too," you must opt in by checking this box.** Otherwise you'll run into "scenario A" in the reference table below—the old sessions look "gone," when in fact they're just sitting in the original drawer.

Click "Cancel" or click outside the dialog: the switch flips straight back to off and nothing happens.
Click "I understand, enable": the switch is saved as on, and CC Switch persists the configuration in the background (and runs the migration if you checked it).

### Step 3 (only if you checked migration): how migration runs + data safety

If you check "Also migrate existing official session history," CC Switch runs this procedure on your existing official sessions:

```text
For each official (openai tag) session file:
   ① First copy the original file untouched into the backup directory   <- data now has its first safety net
   ② Using "write a temp file -> replace the whole thing" atomic style,
      change only the model_provider in the session_meta line at the header
      from "openai" to "custom"                                          <- not a single byte of the conversation body is touched
   ③ Update the index database state_5.sqlite to switch the tag in the same transaction
```

- **Backup location**: `~/.cc-switch/backups/codex-official-history-unify-v1/<timestamp>/`. Each migration produces one timestamped "generation directory," containing `jsonl/` (session copies), `state/` (index DB copy), and `meta.json` (recording which Codex directory this migration belongs to).
- **What's changed**: only the value of the single field `model_provider`. Your conversation content, reasoning content, and all body text are **kept exactly as is**.
- **What's deleted**: **nothing**. The backup is a "copy," the rewrite is an "atomic replacement of the same file," and at no point is any session or index deleted. The file is complete at every moment (either the old content or the new content, never empty or half-written).

After a successful migration, these existing official sessions show up in the unified list. **At this moment your data is**: ① the original copy in the backup directory; ② in the active file, only the classification tag changed, the content intact.

> **Note**: enabling and migration themselves **do not pop a success toast**. Migration runs as a side task on the backend during save; in the UI you'll only see the switch turn on. So "I didn't see a migration-success popup" is normal and does not mean failure.

---

## What happens when you disable it: step by step

### Step 1: Flip the switch from on to off -> probe for backups -> a confirmation dialog pops up

When disabling, CC Switch **first spends a moment probing whether there's a migration backup**, then pops up a confirmation dialog (so the disable dialog has a slight delay, which is normal). The text reads as follows (verbatim):

- **Title**: Turn off unified session history
- **Body**:

  > After turning this off, the official subscription and third-party providers return to separate history lists. Sessions created while it was on cannot be attributed to a provider, so they stay in the third-party history and the official subscription will not see them.

- **Checkbox** (shown conditionally): Restore the official sessions migrated at enable time back to the official history (exact restore from backup)
- **Confirm button**: Turn off
- **Cancel button**: Cancel

> **Key point**: the body says the official subscription **will not see them**—**won't see**, not **delete**. The new sessions you chatted during the unified period are still fully present in the `custom` drawer; after disabling, the official side simply won't see them.

**This restore checkbox is checked by default.** In other words, the default behavior is "restore the official sessions you migrated in back to the official history at the same time you disable." You only need to keep it checked and click "Turn off."

If the checkbox **doesn't appear**, the system has determined there's no backup that needs restoring (either you never checked migration, or no backup was found)—in that case your existing official sessions were never touched, and turning off the switch returns them to the `openai` drawer on their own.

### Step 2: How restore runs (precise flip-back per the backup ledger)

If you keep the box checked and click "Turn off," CC Switch's restore flow goes like this:

```text
① First copy the current state once more into a separate restore-backup directory
   ~/.cc-switch/backups/codex-official-history-unify-restore-v1/<timestamp>/
   (restore itself backs up first, so restore won't lose data either)
② Comb through all migration backup generations, find the session ids "whose tag was originally openai," and assemble a "ledger"
③ Only for sessions that are [both in the ledger AND currently still custom], change the tag back to "openai"
```

Note the **dual condition** in step ③—it must be in the ledger (proving it really was migrated from the official side) AND currently still `custom` (showing you haven't manually changed it). Only when both conditions hold does it get flipped back. This guarantees the restore is both precise and free of collateral damage.

**At this moment your data is**: the migrated-back official sessions have their tag changed back to `openai` and reappear in the official list; meanwhile both the migration backup and the restore backup copies are still on disk.

### Step 3: Read the toast, confirm the result

Only the "disable + check restore" path pops a result toast. The toasts you may see (verbatim):

| Toast you see | Meaning |
|---|---|
| **Official session history restored from backup ({{files}} session files, {{rows}} index rows)** | Restore succeeded. `{{files}}` / `{{rows}}` show the actual numbers |
| **No restorable migration backup for the current Codex directory** | Nothing to restore (**does not mean data is lost**, see scenario E in the reference table) |
| **Unified session history was re-enabled; restore skipped** | You turned the switch back on while restore was queued, so the system deliberately abandoned the restore (see scenario F) |
| **Failed to restore official session history, please try again** | The restore process errored; just retry, the data is not corrupted |
| **Save failed, please try again** | The disable save itself failed; in this case **restore is never triggered** and the switch flips back to its original position |

> **A thoughtful safety design**: if the "disable the switch" save fails, CC Switch **never runs the restore**. Otherwise you'd end up in a torn state of "switch still on, but sessions flipped back to the openai bucket." When the save fails, the switch **automatically flips back to its original position**, so you won't be stuck in a fake state of "looks off but didn't actually save."

---

## "I feel like my sessions are gone?" symptom reference table

The six scenarios below are the situations where users most easily believe "sessions are gone." **The truth in every one is: the data is intact, it just moved drawers or is temporarily out of sight.** Use this table to locate your symptom first, then read the detailed explanation below.

| Scenario | What you see | The data truth | One-line fix |
|---|---|---|---|
| **A** Didn't check migration | Old official sessions not in the unified list | All present, still carry the `openai` tag | Re-enable and check migration, or turn off the switch |
| **B** Cross-provider resume fails | Can't resume / errors out | Files intact, the ciphertext just can't be decrypted across backends | Resume on the original provider; to only read content, read the jsonl directly |
| **C** Proxy takeover / injection refused | No migration and no restore | Migration was safely skipped, files untouched | Exit takeover -> restart and retry; or just turn off the switch |
| **D** New sessions didn't return to official after restore | New sessions from the unified period aren't on the official side | They're in the `custom` drawer, untouched by design | Switch to a third-party provider to see them |
| **E** Toast "no restorable backup" | Restore "failed" | Usually nothing was ever migrated, sessions are in the original drawer | Turn off the switch and the official sessions reappear automatically |
| **F** Toast "switch was re-enabled, restore skipped" | Restore refused | Prevents a torn data state, nothing was changed | Fully turn off the switch first, then restore |

### Scenario A: You enabled the switch but didn't check migration -> old official sessions "disappear"

**Symptom**: you turned on the unified switch, but didn't check "Also migrate existing official session history" in the enable dialog (it's unchecked by default). After enabling, your earlier official sessions seem to be gone from the list.

**The truth**: 100% of your data is present, not a single line moved. The switch only takes effect on official sessions "created after enabling"; your official sessions from **before** enabling still carry the `openai` tag and sit untouched in `~/.codex/sessions/`. You're now on the `custom` drawer, so naturally you can't see the old sessions left in the `openai` drawer—that's the entire reason for the "apparent disappearance."

**What to do** (pick either):
1. **Re-enable the switch and check "Also migrate existing official session history,"** which moves the old sessions to the `custom` drawer and they immediately appear in the unified list (automatic backup before the rewrite).
2. **Or simply turn off the unified switch**, the official side runs on the `openai` drawer again, and the old sessions reappear right where they were.

### Scenario B: Cross-provider resume of an old session fails -> you think "this session is broken / gone"

**Symptom**: after unification, the list shows an old session chatted with "another provider." You switch to your current provider and click "Resume," but it errors out or can't connect.

**The truth**: the session file is intact; what's lost is not data, it's "cross-backend decryption ability." A Codex session stores an encrypted block of reasoning content `encrypted_content`, and **this ciphertext can only be decrypted by the backend that originally generated it**. Using provider B to resume a session generated by provider A means B can't decrypt A's ciphertext -> resume fails. This is **a design limitation of upstream Codex (by design)** and has nothing to do with whether CC Switch touched the file. The text content of the session is readable at any time.

> This is the **only "looks like a real problem" genuine exception** in this whole guide—but note: it just means **you can't resume (can't generate a new turn)**, and **the original file is still fully present**, the conversation text readable at any time.

**What to do**:
- **Resume with "the provider that originally created this session,"** so it can decrypt normally and connect.
- Just want to read the history without continuing? Read that session's `.jsonl` file directly (commands at the end).
- Rule of thumb: **cross-provider is better suited to "starting a new session"; resume old sessions on their original provider whenever possible.**

### Scenario C: You enabled the switch and checked migration, but migration was silently skipped -> you think "migration lost the sessions"

**Symptom**: you enabled the switch and checked migration, but the old official sessions neither entered the unified list nor could be restored when you turned the switch off (or the restore checkbox didn't even appear in the disable dialog, see scenario E). You suspect migration lost the sessions during the process.

**The truth**: migration **never ran**, so it couldn't have lost anything—not a single character of your sessions was changed. CC Switch has a safety gate before migration: it checks whether Codex's live config (`~/.codex/config.toml`) is **actually** routed to the shared `custom` drawer right now, and only migrates if the routing truly went there. The following two situations are judged "not yet unified" (internal reason code `live_not_unified`), so CC Switch **deliberately skips the migration, preserves your switch and migration intent, and migrates later once the conditions are met**:

- **During proxy takeover**: CC Switch's proxy has taken over the live config, and the live config during takeover doesn't carry the unified routing marker.
- **Injection refused**: your `config.toml` already has a manually specified `model_provider`, or there's already a differently-shaped `[model_providers.custom]` table (possibly with a third-party address). To avoid incorrectly routing official traffic to a third-party backend, CC Switch would rather not inject and not migrate.

Skipping migration = touching no session files. **No migration means nothing moved, so there's nothing to lose.** This is "safe deferral," not "failure with data loss."

**What to do**:
- Exit proxy takeover -> **restart CC Switch**: on startup it automatically retries migration (your migration intent is preserved the whole time).
- Check `~/.codex/config.toml`: if there's a conflicting route you wrote by hand, clean up the conflict before enabling the switch.
- If you'd rather not bother: just turn off the switch, the official sessions still display normally on the `openai` drawer, completely intact.

### Scenario D: You turned off the switch and restored, but "the new sessions chatted during the unified period" didn't return to official -> you think "the new sessions are gone"

**Symptom**: during the unified period, you chatted a few more new sessions with the official account. Later you turned off the switch, checked restore, and after restoring you find those new sessions didn't return to the official drawer.

**The truth**: this is **intentional** design; the new sessions are perfectly fine in the `custom` drawer, visible and resumable. Restore is based on "the backup ledger from migration time"—**only sessions that were originally migrated in from the `openai` drawer** are recorded in the backup and get precisely flipped back to `openai`. The sessions you **created during the unified period** are in no backup ledger; and after unification both official and third-party use the `custom` tag, so **CC Switch can't tell whether a new session was chatted with the official account or a third-party**. To avoid wrongly stuffing third-party sessions into the official history, the product decision is: these new sessions all stay in the `custom` (third-party) history and are never moved automatically. The disable dialog's text says this explicitly too—"Sessions created while it was on cannot be attributed to a provider, so they stay in the third-party history."

**What to do**:
- Switch to any third-party provider (the `custom` drawer) to see these sessions in the history list.
- To read content, read the `.jsonl` directly; to resume, follow scenario B's rule (go back to the backend that originally generated it).
- If you really want to manually return **one specific** session to official: there's currently no automatic button (deliberately omitted, to avoid misjudging the direction). Advanced users can, **after backing up** that file first, manually change `model_provider` in the `session_meta` of the first line of its `.jsonl` from `custom` back to `openai` (an advanced operation; always make a copy before editing).

### Scenario E: Restore toast "No restorable migration backup for the current Codex directory" -> you think "restore failed = data is gone"

**Symptom**: you checked restore when turning off the switch, and got the toast "No restorable migration backup for the current Codex directory." You panic: restore failed, is the data completely gone?

**The truth**: "nothing to restore" ≠ "data is lost." On the contrary, it's usually because **there was no migration that needed restoring**. Common reasons:

- **You never checked "migrate existing official sessions" in the first place**: with no migration, there's naturally no migration backup and no sessions to flip back. Your old official sessions have been in the `openai` drawer all along and reappear after you turn off the switch (same as scenario A). (In this case, the disable dialog may **not even show the restore checkbox**—because the system can't find any backup.)
- **You've already restored once**: the session tags have all been flipped back to `openai`, so clicking again naturally finds "no targets still in custom to restore"—this is **idempotent protection, not failure**.
- **You switched Codex directories**: restore only recognizes the backup ledger belonging to the **current** directory; switch directories and it can't find the old directory's ledger. Just switch the directory back.

In all three cases, no session was deleted.

**What to do**: use the end-of-guide commands to count the total session files in `~/.codex/sessions/` and confirm the files are all there; then check whether `~/.cc-switch/backups/` contains a `codex-official-history-unify-v1` directory—if even this directory is absent, you never triggered a migration and the sessions have been in their original drawer all along.

### Scenario F: Restore refused, toast "Unified session history was re-enabled; restore skipped"

**Symptom**: you turned off the switch -> checked restore -> but you were quick and immediately turned the switch back on, then saw the toast "Unified session history was re-enabled; restore skipped."

**The truth**: this is a safeguard against putting your data into a "torn" state, and again no sessions are lost. The restore action is "flip session tags from `custom` back to `openai`," but if the switch is on again at this moment, the live config is routing to `custom`—flipping history back to `openai` on one side while new sessions land in `custom` on the other would artificially tear sessions in two. So when CC Switch detects "the switch is on again," it **deliberately abandons this restore and changes nothing**. Sessions stay as they are, with no deletion or corruption.

**What to do**: to truly restore, **turn the switch off and keep it off** (don't immediately turn it back on), then do disable + check restore; to keep things unified, don't restore, and let the sessions stay in the `custom` shared drawer for normal use.

**The overriding principle: CC Switch's unify / migrate / restore only ever changes a single tag field in a session, and automatically backs up before every rewrite. It never deletes your conversations. Out of sight ≠ gone—look in the other drawer, or use the commands below to confirm with your own eyes.**

---

## Verify by hand: your session files are still on disk (the most important section)

No amount of text beats seeing it for yourself. Below are the **real paths** (taken from the CC Switch source) and how to view session files and backup directories on different systems. **The whole process is read-only and changes nothing; you're strongly encouraged to try it by hand.**

### The simplest way: open it directly in a file manager (no command line at all)

- **macOS (Finder)**: press `Cmd + Shift + G`, paste `~/.codex/sessions` and hit Enter to see a pile of `.jsonl` session files and their modification times; for the backup directory paste `~/.cc-switch/backups`.
- **Windows (File Explorer)**: paste `%USERPROFILE%\.codex\sessions` into the address bar and hit Enter to see the session folders and the `.jsonl` files inside; for the backup directory paste `%USERPROFILE%\.cc-switch\backups`.

**As long as you can see a batch of `.jsonl` files here, that proves your session data is intact on disk.** The file count and modification times are more intuitive than any amount of text.

### Where exactly your session / history files live

| Content | Real path | Notes |
|---|---|---|
| **Session body (the core)** | `~/.codex/sessions/` (includes date-based subdirectories, recursive) | One `.jsonl` text file per session—**this is your conversation content** |
| **Archived sessions** | `~/.codex/archived_sessions/` | Also `.jsonl` |
| **Session index database** | `~/.codex/state_5.sqlite` | The `model_provider` column of the `threads` table is the "drawer tag"—**this is the actual classification source the resume list reads** |
| **Migration backup** (auto-created when migration is enabled) | `~/.cc-switch/backups/codex-official-history-unify-v1/<timestamp>/` | Contains `jsonl/`, `state/`, `meta.json` |
| **Restore backup** (auto-created when you restore) | `~/.cc-switch/backups/codex-official-history-unify-restore-v1/<timestamp>/` | A safety copy taken before restore |

> **Note**: if you've changed the Codex directory in CC Switch, or set `sqlite_home` in `config.toml`, replace `~/.codex` above with your actual directory. Below, `~` = your user home directory.

### macOS / Linux commands

**1. Count the total number of session files (this is the hard evidence of "nothing lost")**

```bash
# Count the total number of session files -- as long as this number matches your expectation, the data is all there
find ~/.codex/sessions ~/.codex/archived_sessions -name '*.jsonl' 2>/dev/null | wc -l

# Show the 10 most recently modified session files
find ~/.codex/sessions -name '*.jsonl' 2>/dev/null -print0 \
  | xargs -0 ls -lt 2>/dev/null | head -10
```

**2. (Auxiliary) See how many sessions are in each "drawer"**

```bash
# Number of session files in the official drawer (openai)
grep -rlE '"model_provider"[[:space:]]*:[[:space:]]*"openai"' ~/.codex/sessions 2>/dev/null | wc -l

# Number of session files in the unified drawer (custom)
grep -rlE '"model_provider"[[:space:]]*:[[:space:]]*"custom"' ~/.codex/sessions 2>/dev/null | wc -l

# See the tag distribution at a glance
grep -rhoE '"model_provider"[[:space:]]*:[[:space:]]*"[^"]*"' ~/.codex/sessions 2>/dev/null | sort | uniq -c
```

> **Important note, don't let this step scare you**: **early versions of Codex did not write the `model_provider` field into the `.jsonl`**, so these old official sessions **can't be counted** by the grep above—but they're still classified as `openai` in the index database `state_5.sqlite` and still show up in the resume list. So **judge "nothing lost" by the total file count from step 1**—the per-drawer grep is only there to help you understand the classification, and counting fewer than the total file count is **completely normal** and never means "a batch was lost."

**3. (Advanced) Query the index database `state_5.sqlite`—the classification the resume list actually reads**

```bash
# Requires sqlite3 to be installed; skip if you don't have it
sqlite3 ~/.codex/state_5.sqlite \
  "SELECT COALESCE(model_provider,'<empty>'), COUNT(*) FROM threads GROUP BY 1;"
```

> This `threads` table is the actual classification source Codex's resume list reads; the `openai` row count ≈ the number of sessions you can see in your official drawer. It may not match step 2's jsonl grep—the reason is exactly what's described above: "old sessions don't write the jsonl field, but they're still openai in the index database." A mismatch between the two is not an anomaly.

**4. Read the content of a specific session directly (confirm the conversation text is still there)**

```bash
# Replace <filename> with one of the .jsonl paths listed by ls above
python3 -m json.tool < "<filename>.jsonl" 2>/dev/null | head -50

# Or just open it in an editor (plain text)
open -e "<filename>.jsonl"      # macOS
```

**5. Look at CC Switch's backup directory (proof that a copy was kept before migration / restore)**

```bash
ls -la ~/.cc-switch/backups/codex-official-history-unify-v1/ 2>/dev/null
ls -la ~/.cc-switch/backups/codex-official-history-unify-restore-v1/ 2>/dev/null
```

### Windows commands (PowerShell)

The session directory is usually at `C:\Users\<your username>\.codex\`, and backups at `C:\Users\<your username>\.cc-switch\backups\`.

```powershell
# 1. Total number of session files (hard evidence of "nothing lost")
(Get-ChildItem "$env:USERPROFILE\.codex\sessions","$env:USERPROFILE\.codex\archived_sessions" -Recurse -Filter *.jsonl -ErrorAction SilentlyContinue).Count

# 2. The 10 most recently modified sessions
Get-ChildItem "$env:USERPROFILE\.codex\sessions" -Recurse -Filter *.jsonl |
  Sort-Object LastWriteTime -Descending | Select-Object -First 10 FullName,LastWriteTime

# 3. (Auxiliary) How many session files in the official (openai) / unified (custom) drawers
(Get-ChildItem "$env:USERPROFILE\.codex\sessions" -Recurse -Filter *.jsonl |
  Select-String -Pattern 'model_provider"\s*:\s*"openai"' -List).Count
(Get-ChildItem "$env:USERPROFILE\.codex\sessions" -Recurse -Filter *.jsonl |
  Select-String -Pattern 'model_provider"\s*:\s*"custom"' -List).Count

# 4. Look at the backup directories
Get-ChildItem "$env:USERPROFILE\.cc-switch\backups\codex-official-history-unify-v1" -ErrorAction SilentlyContinue
Get-ChildItem "$env:USERPROFILE\.cc-switch\backups\codex-official-history-unify-restore-v1" -ErrorAction SilentlyContinue
```

> Same reminder: the step-3 grep counting **fewer** than the total file count is normal (old sessions don't write that field); judge "nothing lost" by the **total file count** from step 1.

---

## Advanced mechanism appendix (for users who want to truly understand how it works)

### 1. The bucketing mechanism (the essence of the drawers)

Codex's resume / history list filters by the currently active `model_provider` id with **exact string matching**. The **first line** of a session's `.jsonl` file is a `type:"session_meta"` record whose `payload.model_provider` is the drawer that session belongs to (`grep -rl` counts a file as long as the tag appears once anywhere in it, so no line-by-line parsing is needed; sessions from old versions that didn't write the field can't be counted). What actually drives the resume list is the `threads.model_provider` column of the index database `state_5.sqlite`. When `config.toml` has no explicit `model_provider`, the official subscription falls into the built-in default id `openai`; all of CC Switch's third-party providers uniformly use `custom`.

### 2. What the switch does (injection, lives only in live)

When enabled, CC Switch injects the following into the official live `config.toml`:

```toml
model_provider = "custom"

[model_providers.custom]
name = "OpenAI"
requires_openai_auth = true
supports_websockets = true
wire_api = "responses"
```

Every field has a purpose: `requires_openai_auth = true` keeps authentication going through the ChatGPT login in `auth.json`, with the base_url defaulting back to the official Codex backend; `name = "OpenAI"` lets Codex's official feature gates (web search, remote compaction, etc.) keep matching; `supports_websockets = true` restores the capability that custom entries lose by default; `wire_api = "responses"` uses the official responses protocol. **The net effect is: authentication is unchanged, only the bucket name changed.**

**Key invariant: this injection can only exist in the live `config.toml`, and is never written into the database's stored configuration.** When you switch away from the official provider and write live back to the database, CC Switch strips this injection precisely (it strips only when the shape exactly matches the injected artifact; a third-party-customized `custom` table is kept as is). Precisely because of this, "turning off the switch + switching once" fully restores live, and the database always holds your original clean official configuration—this is the cornerstone of the whole switch's reversibility.

### 3. The two refusal gates for injection (corresponding to scenario C)

- `config.toml` already has an explicit `model_provider` -> don't override the user's route;
- A differently-shaped `[model_providers.custom]` table already exists (possibly with a third-party `base_url`) -> refuse injection, otherwise ChatGPT OAuth traffic would be routed to the wrong backend.

When injection is refused, live is not unified, and the migration gate (checking whether live's `model_provider` equals `custom` after trim) judges `live_not_unified` -> skip migration, preserve intent, and do it later on the next startup retry. This is "safe deferral," not "failure with data loss."

### 4. The three session classes (which determine the migration / restore boundary)

- **Class A**: existing official sessions migrated in at enable time—the backup is the ledger, and they can be precisely restored back to `openai`;
- **Class B**: created during the unified period—in no backup, and official / third-party can't be distinguished, so they're **never moved automatically** (stay `custom`);
- **Class C**: pure third-party history from before enabling—never touched.

### 5. The safety of migration / restore (data is never truly deleted; where the guarantee comes from)

Four layers of design jointly guarantee that under **all paths, normal and abnormal**, the original session data is never truly deleted.

- **Only change the field, never the body**: migration / restore only switch the `model_provider` value in session metadata between `openai` and `custom`; conversation content, `response_item`, and `encrypted_content` are all kept exactly as is.
- **Always copy a backup before a rewrite**: jsonl uses file copy, the state DB uses a full SQLite copy, both stored in a timestamped generation directory. Migration backups live in `codex-official-history-unify-v1/`, restore backups in the separate `codex-official-history-unify-restore-v1/`—the two are kept apart to keep the ledger clean.
- **Only move, never delete + atomic writes**: all jsonl rewrites go through "temp file + whole-file replacement," and the state DB goes through a transactional `UPDATE`, with no deletion of any session or index at any point. The file is complete at every moment.
- **Pessimistic skip + idempotent and retryable**: when buckets are inconsistent (`live_not_unified`), it would rather not migrate; a single process lock serializes migration and restore to avoid "startup retry / post-save background task / disable-time restore" concurrently rewriting the same batch of files in both directions; the completion marker is bound to the Codex directory and written conditionally to prevent missed migrations; restore uses the "in the ledger + currently still custom" dual condition to prevent wrong changes. Restore scans the union of all backup generations, so even after many switch cycles it can still restore early-migrated sessions; a repeated restore returns `nothing_to_restore`, which is idempotent protection rather than failure.

### 6. Cross-backend encrypted_content (corresponding to scenario B)

The reasoning ciphertext inside a session can only be decrypted by the backend that generated it; upstream Codex by design does not support cross-backend decryption. This is the root cause of "resume failure" and has nothing to do with file integrity—the session `.jsonl` sits fully on disk and `encrypted_content` is intact too. Switching back to the original provider to resume, or starting a new session, both work fine.

---

## References

- [Keep Codex Remote Control and Official Plugins While Using Third-Party APIs: CC Switch Setup Guide](./codex-official-auth-preservation-guide-en.md)
- [Using DeepSeek-Style Chat APIs in Codex: CC Switch Local Routing Guide](./codex-deepseek-routing-guide-en.md)
- The "Codex App Enhancements" section in the CC Switch user manual

---

**One last word for you**: what you see as "sessions disappeared / resume failed" is essentially **the session being moved to another history list (drawer), or the other backend being unable to decrypt the old reasoning content**; the files always sit untouched in `~/.codex/sessions/` (and `state_5.sqlite`). Checking "restore from backup" when you turn off the switch precisely flips the official sessions you migrated in back to the official list; and even if you don't restore, both the original `.jsonl` files and the backup copies under `~/.cc-switch/backups/codex-official-history-unify-*/` are all still there—**the data is never truly lost.**
