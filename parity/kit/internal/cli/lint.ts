#!/usr/bin/env node
// parity lint: the rules that keep cases black-box and version-independent.
//
// - A case imports only #kit and #screens.
// - The screen map imports only the target builders (and #kit for types).
// - Nothing outside kit/ reaches into kit/internal.
// - A case has no timers, eval, DOM access, process access or Node modules, so it can only act
//   through the verbs and observe through checkpoints.
import { readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const PARITY = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");

function files(dir: string): string[] {
  const out: string[] = [];
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) out.push(...files(p));
    else if (p.endsWith(".ts")) out.push(p);
  }
  return out;
}

const IMPORT = /^\s*(?:import|export)\s[^;]*?from\s+["']([^"']+)["']|^\s*import\s+["']([^"']+)["']|\bimport\(\s*["']([^"']+)["']\s*\)/gm;
const imports = (src: string) => [...src.matchAll(IMPORT)].map((m) => m[1] ?? m[2] ?? m[3]);

const FORBIDDEN_IN_CASES: readonly [RegExp, string][] = [
  [/\bset(Timeout|Interval|Immediate)\b/, "timers: wait for something observable with see/gone instead"],
  [/\beval\s*\(|\bnew\s+Function\b/, "eval"],
  [/\bdocument\b|\bwindow\b|querySelector|getElementBy/, "DOM access: name targets in the screen map"],
  [/\bprocess\./, "process access"],
  [/\brequire\s*\(/, "require"],
];

const problems: string[] = [];
const rel = (p: string) => relative(PARITY, p);

for (const f of files(join(PARITY, "cases"))) {
  const src = readFileSync(f, "utf8");
  for (const spec of imports(src)) {
    if (spec !== "#kit" && spec !== "#screens") problems.push(`${rel(f)}: imports "${spec}"; a case may import only #kit and #screens`);
  }
  const code = src.replace(/\/\/.*$/gm, "").replace(/\/\*[\s\S]*?\*\//g, "");
  for (const [re, what] of FORBIDDEN_IN_CASES) if (re.test(code)) problems.push(`${rel(f)}: ${what}`);
}

for (const f of files(join(PARITY, "screens"))) {
  for (const spec of imports(readFileSync(f, "utf8"))) {
    if (spec !== "../kit/screen-builders.ts" && spec !== "#kit") problems.push(`${rel(f)}: imports "${spec}"; the screen map may import only the target builders`);
  }
}

for (const dir of ["cases", "screens"]) {
  for (const f of files(join(PARITY, dir))) {
    for (const spec of imports(readFileSync(f, "utf8"))) {
      if (spec.includes("internal")) problems.push(`${rel(f)}: reaches into kit/internal via "${spec}"`);
    }
  }
}

if (problems.length) {
  console.error(`parity lint: ${problems.length} problem(s)\n${problems.join("\n")}`);
  process.exit(1);
}
console.log("parity lint: ok");
