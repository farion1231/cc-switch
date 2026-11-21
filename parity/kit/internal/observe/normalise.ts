// Pure normalisation shared by every text channel.
//
// Until the clock seam exists, builds run on the real clock. A timestamp is replaced by <time>
// only when it falls inside the run's own wall-clock window, so a time that was written into a
// fixture, or a constant date in the app, still compares exactly. UUIDs are bound to numbered
// placeholders in order of first appearance, so identity across files and channels is kept.

export interface RunWindow {
  readonly startMs: number;
  readonly endMs: number;
}

const SLACK_MS = 2000;

const inWindow = (ms: number, w: RunWindow) => ms >= w.startMs - SLACK_MS && ms <= w.endMs + SLACK_MS;

// Digit lookarounds instead of \b, because file names such as db_backup_20261121_145026 put an
// underscore, a word character, right before the digits.
const ISO = /(?<!\d)(\d{4})-(\d{2})-(\d{2})[T ](\d{2}):(\d{2}):(\d{2})(\.\d{1,9})?(Z|[+-]\d{2}:?\d{2})?/g;
const COMPACT = /(?<!\d)(\d{4})(\d{2})(\d{2})_(\d{2})(\d{2})(\d{2})(?!\d)/g;
const EPOCH = /(?<![\d.])(\d{13}|\d{10})(?![\d.])/g;
const UUID = /(?<![0-9a-f])[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}(?![0-9a-f])/gi;

export class Binder {
  private readonly ids = new Map<string, string>();
  readonly window: RunWindow;
  readonly home: string;
  constructor(window: RunWindow, home: string) {
    this.window = window;
    this.home = home;
  }

  text(s: string): string {
    let out = s.split(this.home).join("<HOME>");
    out = out.replace(ISO, (m, y, mo, d, h, mi, se, frac, zone) => {
      const iso = `${y}-${mo}-${d}T${h}:${mi}:${se}${frac ?? ""}${zone ? (zone === "Z" ? "Z" : zone.length === 5 ? `${zone.slice(0, 3)}:${zone.slice(3)}` : zone) : "Z"}`;
      const ms = Date.parse(iso);
      return Number.isFinite(ms) && inWindow(ms, this.window) ? "<time>" : m;
    });
    out = out.replace(COMPACT, (m, y, mo, d, h, mi, se) => {
      const ms = Date.parse(`${y}-${mo}-${d}T${h}:${mi}:${se}Z`);
      return Number.isFinite(ms) && inWindow(ms, this.window) ? "<time>" : m;
    });
    out = out.replace(EPOCH, (m) => {
      const n = Number(m);
      const ms = m.length === 13 ? n : n * 1000;
      return inWindow(ms, this.window) ? "<time>" : m;
    });
    out = out.replace(UUID, (m) => {
      const key = m.toLowerCase();
      let v = this.ids.get(key);
      if (!v) {
        v = `<uuid:${this.ids.size + 1}>`;
        this.ids.set(key, v);
      }
      return v;
    });
    return out;
  }
}

/** Glob match on '/'-separated relative paths. `**` crosses directories, `*` does not. */
export function globMatch(glob: string, path: string): boolean {
  const re = new RegExp(
    "^" +
      glob
        .split("**")
        .map((part) => part.split("*").map((s) => s.replace(/[.+?^${}()|[\]\\]/g, "\\$&")).join("[^/]*"))
        .join(".*") +
      "$",
  );
  return re.test(path);
}
