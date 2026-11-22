// Seeds on disk: baseline/_seeds/<name>/<os>/ holds seed.json and, under home/, every file the
// recipe left under HOME, byte for byte. git keeps no permission bits beyond the executable one,
// so seed.json lists each entry's mode and restoring applies it.
import { createHash } from "node:crypto";
import { chmodSync, copyFileSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { basename, dirname, join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import type { SeedSpec } from "../../case.ts";
import { scanHome } from "../observe/channels.ts";
import { globMatch } from "../observe/normalise.ts";
import type { Os } from "../observe/store.ts";

/** Left out of a seed: diagnostics, and copies of the database the app makes by itself. */
const DROP = [".cc-switch/logs", ".cc-switch/logs/**", ".cc-switch/backups/*.db"];

const POSIX = process.platform !== "win32";

export interface SeedManifest {
  /** sha256 of the recipe file the seed was built from. */
  readonly recipe: string;
  /** Path under HOME to kind and mode ("-" where the OS has no permission bits). */
  readonly entries: Readonly<Record<string, { readonly kind: "dir" | "file"; readonly mode: string }>>;
}

export const seedDir = (baseline: string, name: string, os: Os) => join(baseline, "_seeds", name, os);

export function recipeHash(file: string): string {
  return createHash("sha256").update(readFileSync(file, "utf8").replace(/\r\n/g, "\n")).digest("hex");
}

export async function loadSeed(file: string): Promise<SeedSpec> {
  const mod = (await import(pathToFileURL(resolve(file)).href)) as { default?: SeedSpec };
  const spec = mod.default;
  if (!spec || typeof spec.build !== "function") throw new Error(`${file} does not default-export a paritySeed`);
  const expected = basename(file).replace(/\.seed\.ts$/, "");
  if (spec.name !== expected) throw new Error(`${file}: seed name "${spec.name}" must equal the file name "${expected}"`);
  return spec;
}

export function readManifest(dir: string): SeedManifest | null {
  const p = join(dir, "seed.json");
  return existsSync(p) ? (JSON.parse(readFileSync(p, "utf8")) as SeedManifest) : null;
}

/**
 * Copies what the recipe left under HOME into the seed directory: every directory and file that
 * is new or changed since `pristine`, except opaque trees (webview storage, caches) and `DROP`.
 * The database is copied with its side files, and the app has quit, so the copy is consistent.
 */
export function writeSeed(dir: string, home: string, exclude: readonly string[], pristine: ReadonlyMap<string, string>, recipe: string): number {
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(join(dir, "home"), { recursive: true });
  const entries: Record<string, { kind: "dir" | "file"; mode: string }> = {};
  for (const [path, st] of scanHome(home, exclude)) {
    if (pristine.get(path) === st.sig || DROP.some((g) => globMatch(g, path)) || st.kind === "opaque-dir") continue;
    if (st.kind === "link") throw new Error(`seed would contain a symbolic link: ${path}`);
    if (st.kind === "dir") {
      mkdirSync(join(dir, "home", path), { recursive: true });
    } else {
      mkdirSync(dirname(join(dir, "home", path)), { recursive: true });
      copyFileSync(join(home, path), join(dir, "home", path));
    }
    entries[path] = { kind: st.kind === "dir" ? "dir" : "file", mode: st.mode };
  }
  const manifest: SeedManifest = { recipe, entries };
  writeFileSync(join(dir, "seed.json"), `${JSON.stringify(manifest, null, 2)}\n`);
  return Object.keys(entries).length;
}

/** Puts a seed into HOME with the modes it was built with, directories before their contents. */
export function restoreSeed(dir: string, home: string): void {
  const manifest = readManifest(dir);
  if (!manifest) throw new Error(`no seed at ${dir}`);
  const paths = Object.keys(manifest.entries).sort();
  for (const path of paths) {
    const { kind, mode } = manifest.entries[path];
    const target = join(home, path);
    if (kind === "dir") mkdirSync(target, { recursive: true });
    else {
      mkdirSync(dirname(target), { recursive: true });
      copyFileSync(join(dir, "home", path), target);
    }
  }
  // Modes last, deepest first, so a directory without write permission is filled before it closes.
  if (POSIX) for (const path of paths.reverse()) {
    const { mode } = manifest.entries[path];
    if (mode !== "-") chmodSync(join(home, path), parseInt(mode, 8));
  }
}
