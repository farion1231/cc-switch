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
}

export interface Channel {
  readonly name: string;
  /** Report order: a likely cause prints before its effects. */
  readonly order: number;
  /** Captures and normalises. The returned text is the recording format. */
  capture(ctx: CaptureContext): Promise<string>;
}

// --- files: the whole HOME tree --------------------------------------------------------------

interface Entry {
  readonly path: string;
  readonly line: string;
  readonly body?: string;
}

function walkHome(home: string): Entry[] {
  const out: Entry[] = [];
  const visit = (rel: string) => {
    const abs = rel ? join(home, rel) : home;
    for (const name of readdirSync(abs).sort()) {
      const path = rel ? `${rel}/${name}` : name;
      const st = lstatSync(join(home, path));
      const mode = (st.mode & 0o777).toString(8);
      const opaque = OPAQUE.find((r) => globMatch(r.glob, path));
      if (st.isSymbolicLink()) {
        out.push({ path, line: `link ${path} -> ${readlinkSync(join(home, path))}` });
      } else if (st.isDirectory()) {
        if (opaque?.kind === "tree") {
          out.push({ path, line: `opaque-dir ${path} ${mode}` });
          continue;
        }
        out.push({ path, line: `dir ${path} ${mode}` });
        visit(path);
      } else if (opaque) {
        out.push({ path, line: `opaque ${path} ${mode}` });
      } else {
        const bytes = readFileSync(join(home, path));
        let text: string | null = null;
        if (!bytes.includes(0)) {
          try { text = new TextDecoder("utf-8", { fatal: true }).decode(bytes); } catch { text = null; }
        }
        if (text === null) {
          out.push({ path, line: `binary ${path} ${mode} ${bytes.length} sha256:${createHash("sha256").update(bytes).digest("hex")}` });
        } else {
          const bom = text.startsWith("\uFEFF");
          const crlf = (text.match(/\r\n/g) ?? []).length;
          const lf = (text.match(/\n/g) ?? []).length - crlf;
          const eol = crlf && lf ? "mixed" : crlf ? "crlf" : "lf";
          const endsWithNewline = text.endsWith("\n");
          out.push({ path, line: `file ${path} ${mode} ${eol}${bom ? " bom" : ""}${endsWithNewline ? "" : " no-final-newline"}`, body: text.replace(/^\uFEFF/, "") });
        }
      }
    }
  };
  visit("");
  return out;
}

export const files: Channel = {
  name: "files",
  order: 3,
  async capture({ home, binder }) {
    // Quiet: two walks half a second apart agree, so a write in progress is not captured.
    let prev = "";
    for (let i = 0; i < 20; i++) {
      const entries = walkHome(home);
      const key = JSON.stringify(entries);
      if (key === prev) return renderFiles(entries, binder);
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
