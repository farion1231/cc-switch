// The channel table. Each channel captures one boundary of the app from outside the process and
// renders it as normalised text, which is what recordings store and what the comparison diffs.
// Adding a channel means adding a row here; recordings, reports and comparisons read the rows.
import { createHash } from "node:crypto";
import { lstatSync, readdirSync, readFileSync, readlinkSync } from "node:fs";
import { join } from "node:path";
import { OPAQUE } from "../../../volatile.ts";
import type { AppSession } from "../driver/app.ts";
import type { TreeResult } from "../driver/probe.ts";
import { settleUi } from "../driver/verbs.ts";
import { globMatch, type Binder } from "./normalise.ts";

export interface CaptureContext {
  readonly app: AppSession;
  readonly home: string;
  readonly binder: Binder;
  /** HOME as it was after reset, by path and signature. Unchanged entries are left out. */
  readonly pristine: ReadonlyMap<string, string>;
  /** Globs under HOME that belong to the OS or the runner, never walked. */
  readonly exclude: readonly string[];
}

export interface Channel {
  readonly name: string;
  /** Report order: a likely cause prints before its effects. */
  readonly order: number;
  /** Captures and normalises. The returned text is the recording format. */
  capture(ctx: CaptureContext): Promise<string>;
}

// --- files: the HOME tree, as a change from its state after reset -----------------------------
//
// On Linux and macOS HOME starts empty, so the change is the whole tree. On Windows HOME is the
// disposable VM's profile, so only what changed since reset is shown.

interface Stat {
  readonly kind: "dir" | "file" | "link" | "opaque" | "opaque-dir";
  readonly mode: string;
  /** Kind and mode for directories, plus size and mtime for everything else. */
  readonly sig: string;
}

/** Permission bits mean something only on POSIX hosts; Windows reports 666 for everything. */
const POSIX = process.platform !== "win32";

/** Lists HOME with stat signatures only. Cheap enough to repeat for the quiet check. */
export function scanHome(home: string, exclude: readonly string[]): Map<string, Stat> {
  const out = new Map<string, Stat>();
  const visit = (rel: string) => {
    let names: string[];
    try { names = readdirSync(rel ? join(home, rel) : home); } catch { return; }
    for (const name of names.sort()) {
      const path = rel ? `${rel}/${name}` : name;
      if (exclude.some((g) => globMatch(g, path))) continue;
      let st;
      try { st = lstatSync(join(home, path)); } catch { continue; }
      const mode = POSIX ? (st.mode & 0o777).toString(8) : "-";
      const opaque = OPAQUE.find((r) => globMatch(r.glob, path));
      if (st.isSymbolicLink()) {
        out.set(path, { kind: "link", mode, sig: `link|${st.size}|${st.mtimeMs}` });
      } else if (st.isDirectory()) {
        if (opaque?.kind === "tree") {
          out.set(path, { kind: "opaque-dir", mode, sig: `opaque-dir|${mode}` });
          continue;
        }
        out.set(path, { kind: "dir", mode, sig: `dir|${mode}` });
        visit(path);
      } else {
        const kind = opaque ? "opaque" : "file";
        out.set(path, { kind, mode, sig: `${kind}|${mode}|${st.size}|${st.mtimeMs}` });
      }
    }
  };
  visit("");
  return out;
}

export function signatures(scan: ReadonlyMap<string, Stat>): Map<string, string> {
  return new Map([...scan].map(([p, s]) => [p, s.sig]));
}

interface Entry {
  readonly path: string;
  readonly line: string;
  readonly body?: string;
}

function describeEntry(home: string, path: string, st: Stat): Entry {
  if (st.kind === "link") return { path, line: `link ${path} -> ${readlinkSync(join(home, path))}` };
  if (st.kind !== "file") return { path, line: `${st.kind} ${path} ${st.mode}` };
  const bytes = readFileSync(join(home, path));
  let text: string | null = null;
  if (!bytes.includes(0)) {
    try { text = new TextDecoder("utf-8", { fatal: true }).decode(bytes); } catch { text = null; }
  }
  if (text === null) return { path, line: `binary ${path} ${st.mode} ${bytes.length} sha256:${createHash("sha256").update(bytes).digest("hex")}` };
  const bom = text.startsWith("\uFEFF");
  const crlf = (text.match(/\r\n/g) ?? []).length;
  const lf = (text.match(/\n/g) ?? []).length - crlf;
  const eol = crlf && lf ? "mixed" : crlf ? "crlf" : "lf";
  const final = text.endsWith("\n") ? "" : " no-final-newline";
  return { path, line: `file ${path} ${st.mode} ${eol}${bom ? " bom" : ""}${final}`, body: text.replace(/^\uFEFF/, "") };
}

export const files: Channel = {
  name: "files",
  order: 3,
  async capture({ home, binder, pristine, exclude }) {
    // Quiet: two scans half a second apart agree, so a write in progress is not captured.
    let prev = "";
    for (let i = 0; i < 20; i++) {
      const scan = scanHome(home, exclude);
      const key = JSON.stringify([...scan].map(([p, s]) => [p, s.sig]));
      if (key === prev) {
        const entries: Entry[] = [];
        for (const [path, st] of scan) if (pristine.get(path) !== st.sig) entries.push(describeEntry(home, path, st));
        for (const path of pristine.keys()) if (!scan.has(path)) entries.push({ path, line: `removed ${path}` });
        return renderFiles(entries, binder);
      }
      prev = key;
      await new Promise((r) => setTimeout(r, 500));
    }
    throw new Error("HOME kept changing for 10 s");
  },
};

/** Paths are normalised first, then sorted, so a renamed timestamped file keeps its place. */
function renderFiles(entries: readonly Entry[], binder: Binder): string {
  return entries
    .map((e) => ({ path: binder.text(e.path), line: binder.text(e.line), body: e.body }))
    .sort((a, b) => (a.path < b.path ? -1 : a.path > b.path ? 1 : 0))
    .map((e) => {
      if (e.body === undefined) return e.line;
      const body = binder.text(e.body).replace(/\r\n/g, "\n").replace(/\n$/, "");
      return `${e.line}\n${body.split("\n").map((l) => `  | ${l}`).join("\n")}`;
    })
    .join("\n");
}

// --- ui: the accessibility tree of the main window, focus, and announcements -----------------

export const ui: Channel = {
  name: "ui",
  order: 5,
  async capture({ app, binder }) {
    const tree: TreeResult = await settleUi(app);
    const announcements = await app.probe<string[]>({ cmd: "announcements" });
    const lines = [
      `window main ${JSON.stringify(tree.title)} scheme=${tree.scheme}`,
      `focused: ${tree.focused ?? "(none)"}`,
      "tree:",
      ...tree.text.split("\n").filter(Boolean).map((l) => `  ${l}`),
      "announcements since the previous checkpoint:",
      ...announcements.map((a) => `  ${JSON.stringify(a)}`),
    ];
    return binder.text(lines.join("\n"));
  },
};

export const CHANNELS: readonly Channel[] = [files, ui];
