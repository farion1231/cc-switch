#!/usr/bin/env node
// parity record|check [--app <binary>] [--runs 3] [--baseline <dir>] [--out <dir>] <case file>...
//
// record  runs each case --runs times on fresh state. The recording is written only when every
//         run completed, every intent line held, and all runs observed exactly the same thing.
// check   runs each case once and compares every checkpoint and channel with the recording.
import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import type { CaseSpec } from "../../case.ts";
import { WINDOWS_PROFILE_NOISE } from "../../../volatile.ts";
import { launchLinux, launchMacos, launchWindows } from "../driver/app.ts";
import { scanHome, signatures } from "../observe/channels.ts";
import { runCase, type CaseRun, type HostOptions } from "../host/run.ts";
import { firstDifference } from "../observe/compare.ts";
import { checkpointDir, readRecording, recordedCheckpoints, writeRecording, type Os } from "../observe/store.ts";

const PARITY = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");

function parseArgs(argv: string[]) {
  const flags: Record<string, string> = {};
  const rest: string[] = [];
  for (let i = 0; i < argv.length; i++) {
    if (argv[i].startsWith("--")) flags[argv[i].slice(2)] = argv[++i];
    else rest.push(argv[i]);
  }
  return { flags, rest };
}

async function loadCase(file: string): Promise<CaseSpec> {
  const mod = (await import(pathToFileURL(resolve(file)).href)) as { default?: CaseSpec };
  const spec = mod.default;
  if (!spec || typeof spec.journey !== "function") throw new Error(`${file} does not default-export a parityCase`);
  const expected = basename(file).replace(/\.case\.ts$/, "");
  if (spec.id !== expected) throw new Error(`${file}: case id "${spec.id}" must equal the file name "${expected}"`);
  return spec;
}

function hostFor(os: Os, app: string, flags: Record<string, string>): HostOptions {
  if (os === "linux") {
    const home = "/home/parity";
    return {
      home,
      exclude: [],
      reset: () => {
        rmSync(home, { recursive: true, force: true });
        mkdirSync(home, { recursive: true, mode: 0o755 });
      },
      sentinels: [process.env.HOME ?? "/tmp/sentinel-home"],
      launch: (h) => launchLinux({ binary: app, home: h }),
    };
  }
  if (os === "windows") {
    const home = process.env.USERPROFILE;
    if (!home) throw new Error("USERPROFILE is not set");
    if (process.env.PARITY_DISPOSABLE_HOST !== "1") throw new Error("Windows cases use the real profile; set PARITY_DISPOSABLE_HOST=1 only on a disposable VM");
    const exclude = WINDOWS_PROFILE_NOISE.map((r) => r.glob);
    let initial: Map<string, string> | null = null;
    return {
      home,
      exclude,
      // The first reset takes the profile as it is. Later resets delete what was added since.
      reset: () => {
        const now = scanHome(home, exclude);
        if (!initial) {
          initial = signatures(now);
          return;
        }
        const base = initial;
        const added = [...now.keys()].filter((p) => !base.has(p)).sort((a, b) => b.length - a.length);
        for (const p of added) rmSync(join(home, p), { recursive: true, force: true });
        const changed = [...now].filter(([p, s]) => base.has(p) && base.get(p) !== s.sig && s.kind !== "dir").map(([p]) => p);
        if (changed.length) throw new Error(`reset cannot restore profile files an earlier case changed: ${changed.join(", ")}`);
      },
      sentinels: [],
      launch: () => launchWindows({ binary: app, driver: flags.driver ?? "tauri-driver", edgeDriver: flags.edge ?? "msedgedriver" }),
    };
  }
  if (os === "macos") {
    const real = process.env.HOME;
    if (!real) throw new Error("HOME is not set");
    // A fixed path, so paths written into files are the same on every hosted runner.
    const home = join(real, "parity-home");
    return {
      home,
      exclude: [],
      reset: () => {
        rmSync(home, { recursive: true, force: true });
        mkdirSync(home, { recursive: true, mode: 0o755 });
      },
      sentinels: [".cc-switch", ".claude", ".codex", ".gemini", ".config/opencode", "Library/Application Support/com.ccswitch.desktop"].map((d) => join(real, d)),
      launch: (h) => launchMacos({ app, helper: flags.helper ?? "parity-helper", home: h }),
    };
  }
  throw new Error(`host for ${os} is not implemented yet`);
}

function saveRun(out: string, run: CaseRun, label: string): void {
  const dir = join(out, run.caseId, label);
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(dir, { recursive: true });
  run.checkpoints.forEach((c, i) => {
    const d = join(dir, checkpointDir(i, c.name));
    mkdirSync(d, { recursive: true });
    for (const [ch, text] of Object.entries(c.channels)) writeFileSync(join(d, `${ch}.snap`), `${text}\n`);
  });
  writeFileSync(join(dir, "run.json"), JSON.stringify({ ...run, checkpoints: run.checkpoints.map((c) => c.name) }, null, 2));
}

function summary(run: CaseRun): string {
  return `${run.ms} ms, ${run.steps.length} steps, ${run.checkpoints.length} checkpoints`;
}

/** Differences between two runs of the same case, by checkpoint and channel. */
function runDifferences(a: CaseRun, b: CaseRun): string[] {
  const out: string[] = [];
  const names = (r: CaseRun) => r.checkpoints.map((c) => c.name).join(", ");
  if (names(a) !== names(b)) return [`checkpoints differ: [${names(a)}] vs [${names(b)}]`];
  a.checkpoints.forEach((c, i) => {
    for (const [ch, text] of Object.entries(c.channels)) {
      const d = firstDifference(text, b.checkpoints[i].channels[ch] ?? "");
      if (d) out.push(`${checkpointDir(i, c.name)} ${ch}:\n${d}`);
    }
  });
  return out;
}

async function main() {
  const { flags, rest } = parseArgs(process.argv.slice(2));
  const mode = rest.shift();
  if ((mode !== "record" && mode !== "check") || rest.length === 0) {
    console.error("usage: main.ts record|check [--app <binary>] [--runs n] [--baseline dir] [--out dir] <case file>...");
    process.exit(2);
  }
  const os = (flags.os ?? "linux") as Os;
  const app = flags.app ?? "/app/cc-switch";
  const baseline = resolve(flags.baseline ?? join(PARITY, "baseline"));
  const out = resolve(flags.out ?? join(PARITY, ".out", os, "cases"));
  const host = hostFor(os, app, flags);
  let failed = 0;
  const report: unknown[] = [];

  for (const file of rest) {
    const spec = await loadCase(file);
    if (mode === "record") {
      const runs: CaseRun[] = [];
      const n = Number(flags.runs ?? 3);
      for (let i = 0; i < n; i++) {
        const r = await runCase(spec, host);
        saveRun(out, r, `record-${i + 1}`);
        console.log(`${spec.id} record run ${i + 1}/${n}: ${summary(r)}${r.errors.length ? ` ERROR ${r.errors.join("; ")}` : ""}`);
        runs.push(r);
      }
      const problems: string[] = [];
      runs.forEach((r, i) => {
        r.errors.forEach((e) => problems.push(`run ${i + 1}: ${e}`));
        r.intentFailures.forEach((e) => problems.push(`run ${i + 1}: intent: ${e}`));
      });
      for (let i = 1; i < runs.length; i++) runDifferences(runs[0], runs[i]).forEach((d) => problems.push(`run 1 vs run ${i + 1} differ at ${d}`));
      if (problems.length) {
        failed++;
        console.log(`${spec.id}: recording REFUSED\n${problems.join("\n")}`);
        if (runs.some((r) => r.tree)) console.log(`UI tree at the failure:\n${runs.find((r) => r.tree)?.tree}`);
      } else {
        writeRecording(baseline, spec.id, runs[0].checkpoints, os);
        console.log(`${spec.id}: recorded ${runs[0].checkpoints.length} checkpoints from ${runs.length} identical runs`);
      }
      report.push({ case: spec.id, mode, problems, ms: runs.map((r) => r.ms) });
    } else {
      const r = await runCase(spec, host);
      saveRun(out, r, "check");
      const problems = [...r.errors, ...r.intentFailures.map((e) => `intent: ${e}`)];
      const recorded = recordedCheckpoints(baseline, spec.id);
      if (recorded.length === 0) problems.push("no recording");
      const actual = r.checkpoints.map((c, i) => checkpointDir(i, c.name));
      if (recorded.length && recorded.join() !== actual.join()) problems.push(`checkpoints differ: recorded [${recorded.join(", ")}], ran [${actual.join(", ")}]`);
      r.checkpoints.forEach((c, i) => {
        for (const [ch, text] of Object.entries(c.channels)) {
          const want = readRecording(baseline, spec.id, i, c.name, ch, os);
          if (want === null) continue;
          const d = firstDifference(want, text);
          if (d) problems.push(`${checkpointDir(i, c.name)} ${ch}:\n${d}`);
        }
      });
      if (problems.length) failed++;
      console.log(`${spec.id}: ${problems.length ? "FAIL" : "pass"} (${summary(r)})${problems.length ? `\n${problems.join("\n")}` : ""}`);
      if (r.tree) console.log(`UI tree at the failure:\n${r.tree}`);
      report.push({ case: spec.id, mode, problems, ms: r.ms });
    }
  }
  mkdirSync(out, { recursive: true });
  writeFileSync(join(out, `report-${mode}.json`), JSON.stringify(report, null, 2));
  process.exit(failed ? 1 : 0);
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
