// Runs one case once: reset HOME, launch, journey with checkpoints, teardown, sentinel check.
// Every step is idempotent and reset comes first, so a crashed case cannot poison the next one.
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import type { CaseSpec, Duration, Expect, HomePath, Journey, SeedSpec } from "../../case.ts";
import type { AppSession } from "../driver/app.ts";
import { createHash } from "node:crypto";
import { TREE_CMD, type TreeResult } from "../driver/probe.ts";
import { isVisible, makeUi, settleUi, StepError, type StepHooks, type StepLog } from "../driver/verbs.ts";
import { CHANNELS, files, scanHome, signatures } from "../observe/channels.ts";
import { Binder } from "../observe/normalise.ts";
import { CLOCK_ENV, PinnedClock, SEED_ORIGIN_MS } from "./clock.ts";
import { restoreSeed } from "./seeds.ts";
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
  /** The pinned clock's file, outside HOME and outside anything observed. */
  readonly clockFile: string;
  readonly launch: (home: string, env?: Record<string, string>) => Promise<AppSession>;
}

export interface CaseRun {
  readonly caseId: string;
  readonly checkpoints: CheckpointObservation[];
  /** One line per step: what it did and a print of what was observable after it. */
  readonly prints: string[];
  /** Hand-written intent lines that failed. A recording is refused when any did. */
  readonly intentFailures: string[];
  /** The journey could not complete. */
  readonly errors: string[];
  readonly steps: StepLog[];
  readonly ms: number;
  tree?: string | null;
  driverLog?: string;
}

const sha12 = (s: string) => createHash("sha256").update(s).digest("hex").slice(0, 12);

function durationMs(d: Duration): number {
  const ms = (((d.days ?? 0) * 24 + (d.hours ?? 0)) * 60 + (d.minutes ?? 0)) * 60_000 + (d.seconds ?? 0) * 1000;
  if (!Number.isFinite(ms) || ms <= 0) throw new Error(`advanceClock needs a positive duration, got ${JSON.stringify(d)}`);
  return ms;
}

const durationText = (d: Duration) =>
  (["days", "hours", "minutes", "seconds"] as const).filter((k) => d[k]).map((k) => `${d[k]}${k[0]}`).join(" ");

/**
 * With the clock pinned nothing the app writes or shows may carry the real time. The binder turns
 * every timestamp inside the run's real window into <time>, so any <time> left is a read that
 * bypassed the clock seam.
 */
function clockLeaks(checkpoint: string, channels: Record<string, string>): string[] {
  const out: string[] = [];
  for (const [ch, text] of Object.entries(channels)) {
    const lines = text.split("\n").filter((l) => l.includes("<time>"));
    if (lines.length) out.push(`clock leak at ${checkpoint} ${ch}: ${lines.length} line(s) carry the real time, first: ${lines[0].trim()}`);
  }
  return out;
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

const newRun = (caseId: string): CaseRun => ({ caseId, checkpoints: [], prints: [], intentFailures: [], errors: [], steps: [], ms: 0 });

/**
 * Runs one case once. `seed` is the directory of the case's recorded seed, restored after reset;
 * HOME's state before the restore is what the files channel compares with, so the seed's files
 * are part of every checkpoint.
 */
export async function runCase(spec: CaseSpec, host: HostOptions, seed?: string): Promise<CaseRun> {
  if (spec.seed !== "fresh" && !seed) throw new Error(`case ${spec.id}: seed ${spec.seed.seed} is not recorded for this OS`);
  const run = newRun(spec.id);
  host.reset();
  const pristine = signatures(scanHome(host.home, host.exclude));
  if (seed) restoreSeed(seed, host.home);
  const clock = new PinnedClock(host.clockFile);
  clock.reset();
  await session(host, clock, pristine, run, (j) => spec.journey(j));
  return run;
}

/**
 * Runs a seed's recipe from an empty HOME, one day before the cases' start, and leaves its result
 * in HOME for the caller to record. Step prints and the inert-step check apply as in a case.
 */
export async function buildSeed(spec: SeedSpec, host: HostOptions): Promise<{ run: CaseRun; pristine: Map<string, string> }> {
  const run = newRun(`_seed.${spec.name}`);
  host.reset();
  const pristine = signatures(scanHome(host.home, host.exclude));
  const clock = new PinnedClock(host.clockFile);
  clock.set(SEED_ORIGIN_MS);
  await session(host, clock, pristine, run, (j) => spec.build({ ui: j.ui, expect: j.expect, advanceClock: (by) => j.advanceClock(by) }));
  return { run, pristine };
}

async function session(host: HostOptions, clock: PinnedClock, pristine: ReadonlyMap<string, string>, run: CaseRun, body: (j: Journey) => Promise<void>): Promise<void> {
  const before = host.sentinels.map(listing);
  const startMs = Date.now();
  let app: AppSession | null = null;
  try {
    app = await host.launch(host.home, { [CLOCK_ENV]: host.clockFile });
    const pageMs = await app.pageNow();
    if (pageMs !== clock.ms) throw new Error(`the page's clock reads ${new Date(pageMs).toISOString()}, not the pinned ${new Date(clock.ms).toISOString()}; is this a parity build?`);
    await app.probe({ cmd: "install" });
    await settleUi(app);
    const a = app;
    // A print of everything observable: the normalised tree and focus, the normalised HOME, and how
    // many announcements have appeared. An acting step whose print equals the previous one did
    // nothing a user or the agent could see, which fails as an inert step unless it says why. Only
    // the HOME part is recorded; the UI is read to judge the step, not compared.
    //
    // A step's print is taken late: when the next step's target is actionable, at the next
    // checkpoint or clock move, or when the journey ends, and after the UI settles again. By then
    // the case's own waits (see, gone) have passed, so a slow response cannot land on either side
    // of the print.
    type Print = { tree: string; home: string; announcements: number };
    const printNow = async (): Promise<Print> => {
      const binder = new Binder({ startMs, endMs: Date.now() }, host.home);
      const tree = await a.probe<TreeResult>(TREE_CMD);
      const homeText = await files.capture({ app: a, home: host.home, binder, pristine, exclude: host.exclude });
      return {
        tree: sha12(binder.text(JSON.stringify([tree.text, tree.focused]))),
        home: sha12(homeText),
        announcements: await a.probe<number>({ cmd: "announceCount" }),
      };
    };
    const line = (n: number, step: string, p: Print, target?: string, inert?: string) =>
      `${String(n).padStart(2, "0")} ${step}${target ? ` ${target}` : ""} | home ${p.home}${inert ? ` | inert: ${inert}` : ""}`;
    let prev = await printNow();
    run.prints.push(line(0, "launch", prev));
    let pending: { verb: string; target?: string; inert?: string } | null = null;
    const flush = async () => {
      if (!pending) return;
      const { verb, target, inert } = pending;
      pending = null;
      await settleUi(a);
      const now = await printNow();
      const n = run.prints.length;
      run.prints.push(line(n, verb, now, target, inert));
      const same = now.tree === prev.tree && now.home === prev.home && now.announcements === prev.announcements;
      prev = now;
      if (same && !inert) throw new StepError(`inert step ${n}: ${verb} ${target ?? ""} changed nothing observable`, null);
      if (!same && inert) throw new StepError(`step ${n}: ${verb} ${target ?? ""} was declared inert ("${inert}") but changed what is observable`, null);
    };
    const hooks: StepHooks = {
      before: flush,
      after: async (step) => {
        pending = step;
      },
    };
    const journey: Journey = {
      ui: makeUi(a, run.steps, hooks),
      expect: makeExpect(host.home, a, run.intentFailures),
      async checkpoint(name) {
        if (run.checkpoints.some((c) => c.name === name)) throw new Error(`checkpoint "${name}" is used twice`);
        await flush();
        const binder = new Binder({ startMs, endMs: Date.now() }, host.home);
        const channels: Record<string, string> = {};
        for (const ch of CHANNELS) channels[ch.name] = await ch.capture({ app: a, home: host.home, binder, pristine, exclude: host.exclude });
        run.checkpoints.push({ name, channels });
        run.errors.push(...clockLeaks(name, channels));
      },
      async advanceClock(by) {
        await flush();
        clock.set(clock.ms + durationMs(by));
        // The app polls the clock file and passes the new instant to every page.
        const deadline = Date.now() + 5000;
        while ((await a.pageNow()) !== clock.ms) {
          if (Date.now() > deadline) throw new StepError(`the page's clock did not reach ${new Date(clock.ms).toISOString()} within 5 s`, null);
          await new Promise((r) => setTimeout(r, 50));
        }
        await settleUi(a);
        prev = await printNow();
        run.prints.push(line(run.prints.length, "clock", prev, `+${durationText(by)}`));
      },
    };
    await body(journey);
    await flush();
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
}
