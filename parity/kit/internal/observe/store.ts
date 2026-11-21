// Recordings on disk: baseline/<case-id>/<nn>-<checkpoint>/<channel>[.<os>].snap. A recording
// without an OS suffix holds for every OS that has no suffixed file of its own.
import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";

export type Os = "linux" | "macos" | "windows";

export interface CheckpointObservation {
  readonly name: string;
  readonly channels: Readonly<Record<string, string>>;
}

/** Recordings are LF text. A checkout that converted them to CRLF must not read as a difference. */
const readSnap = (p: string) => readFileSync(p, "utf8").replace(/\r\n/g, "\n").replace(/\n$/, "");

export const checkpointDir = (index: number, name: string) => `${String(index + 1).padStart(2, "0")}-${name}`;

export function readRecording(root: string, caseId: string, index: number, checkpoint: string, channel: string, os: Os): string | null {
  const dir = join(root, caseId, checkpointDir(index, checkpoint));
  for (const f of [`${channel}.${os}.snap`, `${channel}.snap`]) {
    const p = join(dir, f);
    if (existsSync(p)) return readSnap(p);
  }
  return null;
}

export function recordedCheckpoints(root: string, caseId: string): string[] {
  const dir = join(root, caseId);
  return existsSync(dir) ? readdirSync(dir).filter((d) => /^\d{2}-/.test(d)).sort() : [];
}

/** The OS whose recordings are the shared, unsuffixed files. */
export const PRIMARY_OS: Os = "linux";

/**
 * Writes a case's recording. The primary OS writes the shared files and drops checkpoints that
 * no longer exist. Any other OS writes `<channel>.<os>.snap` only where it differs from the
 * shared file, and deletes its own file where it no longer differs.
 */
export function writeRecording(root: string, caseId: string, observations: readonly CheckpointObservation[], steps: string, os: Os): void {
  const dir = join(root, caseId);
  const dirs = observations.map((o, i) => checkpointDir(i, o.name));
  if (os === PRIMARY_OS && existsSync(dir)) {
    for (const d of readdirSync(dir)) if (/^\d{2}-/.test(d) && !dirs.includes(d)) rmSync(join(dir, d), { recursive: true, force: true });
  }
  mkdirSync(dir, { recursive: true });
  writeVariant(dir, "steps", steps, os);
  observations.forEach((o, i) => {
    const d = join(dir, dirs[i]);
    mkdirSync(d, { recursive: true });
    for (const [channel, text] of Object.entries(o.channels)) writeVariant(d, channel, text, os);
  });
}

/** The primary OS writes the shared file; another OS writes its own file only where it differs. */
function writeVariant(dir: string, name: string, text: string, os: Os): void {
  if (os === PRIMARY_OS) {
    writeFileSync(join(dir, `${name}.snap`), `${text}\n`);
    return;
  }
  const shared = join(dir, `${name}.snap`);
  const own = join(dir, `${name}.${os}.snap`);
  if (existsSync(shared) && readSnap(shared) === text) rmSync(own, { force: true });
  else writeFileSync(own, `${text}\n`);
}

/** The recorded step prints of a case for this OS, or null. */
export function readSteps(root: string, caseId: string, os: Os): string | null {
  for (const f of [`steps.${os}.snap`, "steps.snap"]) {
    const p = join(root, caseId, f);
    if (existsSync(p)) return readSnap(p);
  }
  return null;
}
