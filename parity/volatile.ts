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
  { glob: "AppData/Local/com.ccswitch.desktop", kind: "tree", reason: "Windows: Tauri's per-identifier folder for webview data", owner: "parity" },
];

/**
 * Windows only: parts of the VM profile that belong to the OS, the runner or the harness's own
 * tools. They are never walked, so their churn cannot show as a difference. Anything the app
 * writes must land outside these.
 */
export const WINDOWS_PROFILE_NOISE: readonly OpaqueRule[] = [
  { glob: "AppData/Local/Temp", kind: "tree", reason: "temporary files of every process, including the WebView2 profile msedgedriver creates per session", owner: "parity" },
  { glob: "AppData/Local/Microsoft", kind: "tree", reason: "OS and Edge state", owner: "parity" },
  { glob: "AppData/Local/Packages", kind: "tree", reason: "OS app packages", owner: "parity" },
  { glob: "AppData/Roaming/Microsoft", kind: "tree", reason: "OS state", owner: "parity" },
  { glob: "AppData/LocalLow", kind: "tree", reason: "OS state", owner: "parity" },
  { glob: "AppData/Local/ConnectedDevicesPlatform", kind: "tree", reason: "OS state", owner: "parity" },
  { glob: "AppData/Local/D3DSCache", kind: "tree", reason: "GPU shader cache", owner: "parity" },
  { glob: ".cargo", kind: "tree", reason: "the harness's Rust toolchain (tauri-driver install)", owner: "parity" },
  { glob: ".rustup", kind: "tree", reason: "the harness's Rust toolchain", owner: "parity" },
  { glob: "NTUSER*", kind: "file", reason: "registry hive files, written by the OS", owner: "parity" },
  { glob: "ntuser*", kind: "file", reason: "registry hive files, written by the OS", owner: "parity" },
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
