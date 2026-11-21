// Runs one case once: reset HOME, launch, journey with checkpoints, teardown, sentinel check.
// Every step is idempotent and reset comes first, so a crashed case cannot poison the next one.
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import type { CaseSpec, Expect, HomePath, Journey } from "../../case.ts";
import type { AppSession } from "../driver/app.ts";
import { isVisible, makeUi, settleUi, StepError, type StepLog } from "../driver/verbs.ts";
import { CHANNELS, scanHome, signatures } from "../observe/channels.ts";
import { Binder } from "../observe/normalise.ts";
import type { CheckpointObservation } from "../observe/store.ts";

export interface HostOptions {
  /** The app's HOME. A fixed path per OS, so paths written into files compare across runs. */
  readonly home: string;
  /** Brings HOME back to its state before the first case. Throws if that is impossible. */
  readonly reset: () => void;
  /** Globs under HOME that belong to the OS or the runner, never walked. */
  readonly exclude: readonly string[];
  /** Places outside HOME the app must not touch. Each must be unchanged after the case. */
  readonly sentinels: readonly string[];
  readonly launch: (home: string) => Promise<AppSession>;
}

export interface CaseRun {
  readonly caseId: string;
  readonly checkpoints: CheckpointObservation[];
  /** Hand-written intent lines that failed. A recording is refused when any did. */
  readonly intentFailures: string[];
  /** The journey could not complete. */
  readonly errors: string[];
  readonly steps: StepLog[];
  readonly ms: number;
  tree?: string | null;
  driverLog?: string;
}

function listing(path: string): string[] {
  if (!existsSync(path)) return [];
  const out: string[] = [];
  const visit = (rel: string) => {
    for (const name of readdirSync(join(path, rel)).sort()) {
      const p = rel ? `${rel}/${name}` : name;
      const st = statSync(join(path, p), { throwIfNoEntry: false });
      out.push(`${p} ${st?.size ?? "?"} ${st?.mtimeMs ?? "?"}`);
      if (st?.isDirectory()) visit(p);
    }
  };
  visit("");
  return out;
}

function makeExpect(home: string, app: AppSession, failures: string[]): Expect {
  const abs = (p: HomePath) => join(home, p.slice(2));
  const fail = (msg: string) => failures.push(msg);
  return {
    file(path) {
      return {
        toExist: () => { if (!existsSync(abs(path))) fail(`expected ${path} to exist`); },
        toBeAbsent: () => { if (existsSync(abs(path))) fail(`expected ${path} to be absent`); },
        toContain: (text) => {
          if (!existsSync(abs(path))) return fail(`expected ${path} to contain ${JSON.stringify(text)}, but it does not exist`);
          if (!readFileSync(abs(path), "utf8").includes(text)) fail(`expected ${path} to contain ${JSON.stringify(text)}`);
        },
        at(key) {
          const read = (): { found: boolean; value?: unknown } => {
            if (!existsSync(abs(path))) return { found: false };
            let v: unknown = JSON.parse(readFileSync(abs(path), "utf8"));
            for (const k of key.split(".")) {
              if (v === null || typeof v !== "object" || !(k in (v as object))) return { found: false };
              v = (v as Record<string, unknown>)[k];
            }
            return { found: true, value: v };
          };
          return {
            toBe: (value) => {
              const r = read();
              if (!r.found || JSON.stringify(r.value) !== JSON.stringify(value)) fail(`expected ${path} at ${key} to be ${JSON.stringify(value)}, got ${r.found ? JSON.stringify(r.value) : "nothing"}`);
            },
            toBeAbsent: () => { if (read().found) fail(`expected ${path} to have no ${key}`); },
          };
        },
      };
    },
    async visible(target) {
      if (!(await isVisible(app, target))) fail(`expected ${String(target)} to be visible`);
    },
  };
}

export async function runCase(spec: CaseSpec, host: HostOptions): Promise<CaseRun> {
  if (spec.seed !== "fresh") throw new Error(`case ${spec.id}: seeds other than "fresh" are not implemented yet`);
  const run: CaseRun = { caseId: spec.id, checkpoints: [], intentFailures: [], errors: [], steps: [], ms: 0 };
  host.reset();
  const pristine = signatures(scanHome(host.home, host.exclude));
  const before = host.sentinels.map(listing);
  const startMs = Date.now();
  let app: AppSession | null = null;
  try {
    app = await host.launch(host.home);
    await app.probe({ cmd: "install" });
    await settleUi(app);
    const a = app;
    const journey: Journey = {
      ui: makeUi(a, run.steps),
      expect: makeExpect(host.home, a, run.intentFailures),
      async checkpoint(name) {
        if (run.checkpoints.some((c) => c.name === name)) throw new Error(`checkpoint "${name}" is used twice`);
        const binder = new Binder({ startMs, endMs: Date.now() }, host.home);
        const channels: Record<string, string> = {};
        for (const ch of CHANNELS) channels[ch.name] = await ch.capture({ app: a, home: host.home, binder, pristine, exclude: host.exclude });
        run.checkpoints.push({ name, channels });
      },
    };
    await spec.journey(journey);
  } catch (e) {
    run.errors.push(e instanceof Error ? e.message : String(e));
    if (e instanceof StepError) run.tree = e.tree;
  } finally {
    if (app) run.driverLog = (await app.close()).log;
  }
  const after = host.sentinels.map(listing);
  host.sentinels.forEach((s, i) => {
    const added = after[i].filter((x) => !before[i].includes(x));
    const removed = before[i].filter((x) => !after[i].includes(x));
    if (added.length || removed.length) run.errors.push(`outside-sandbox write in ${s}: +${JSON.stringify(added)} -${JSON.stringify(removed)}`);
  });
  (run as { ms: number }).ms = Date.now() - startMs;
  return run;
}
