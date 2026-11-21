/**
 * Everything the comparison forgives, in one list. Each rule says why and who owns it. Anything
 * not listed here is compared exactly.
 */

export interface OpaqueRule {
  /** Glob relative to HOME. `**` crosses directories, `*` does not. */
  readonly glob: string;
  /** "file": compare existence and mode only. "tree": compare that the directory exists, not its contents. */
  readonly kind: "file" | "tree";
  readonly reason: string;
  readonly owner: string;
}

export const OPAQUE: readonly OpaqueRule[] = [
  { glob: ".cc-switch/cc-switch.db", kind: "file", reason: "the database is observed through the UI and the files it writes, never by its bytes", owner: "parity" },
  { glob: ".cc-switch/cc-switch.db-*", kind: "file", reason: "SQLite side files", owner: "parity" },
  { glob: ".cc-switch/backups/*.db", kind: "file", reason: "database snapshots taken by the app; their bytes are the database's", owner: "parity" },
  { glob: ".cc-switch/logs/**", kind: "file", reason: "log bodies are diagnostics, not behaviour", owner: "parity" },
  { glob: ".local/share/com.ccswitch.desktop", kind: "tree", reason: "WebKit storage and caches", owner: "parity" },
  { glob: ".cache/com.ccswitch.desktop", kind: "tree", reason: "runtime caches (tray icon)", owner: "parity" },
];

export interface TransientUiRule {
  /** CSS selector, evaluated in the page. Matching subtrees are left out of the UI tree. */
  readonly selector: string;
  readonly reason: string;
  readonly owner: string;
}

export const TRANSIENT_UI: readonly TransientUiRule[] = [
  {
    selector: "[data-sonner-toaster], [data-sonner-toast]",
    reason: "toasts are logged as announcements when they appear; whether one is still on screen at a checkpoint depends on timing",
    owner: "parity",
  },
];
