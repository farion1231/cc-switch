// Recordings on disk: baseline/<case-id>/<nn>-<checkpoint>/<channel>[.<os>].snap. A recording
// without an OS suffix holds for every OS that has no suffixed file of its own.
import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";

export type Os = "linux" | "macos" | "windows";

export interface CheckpointObservation {
  readonly name: string;
  readonly channels: Readonly<Record<string, string>>;
}

export const checkpointDir = (index: number, name: string) => `${String(index + 1).padStart(2, "0")}-${name}`;

export function readRecording(root: string, caseId: string, index: number, checkpoint: string, channel: string, os: Os): string | null {
  const dir = join(root, caseId, checkpointDir(index, checkpoint));
  for (const f of [`${channel}.${os}.snap`, `${channel}.snap`]) {
    const p = join(dir, f);
    if (existsSync(p)) return readFileSync(p, "utf8").replace(/\n$/, "");
  }
  return null;
}

export function recordedCheckpoints(root: string, caseId: string): string[] {
  const dir = join(root, caseId);
  return existsSync(dir) ? readdirSync(dir).filter((d) => /^\d{2}-/.test(d)).sort() : [];
}

/** Replaces the case's recording with these observations. */
export function writeRecording(root: string, caseId: string, observations: readonly CheckpointObservation[]): void {
  const dir = join(root, caseId);
  rmSync(dir, { recursive: true, force: true });
  observations.forEach((o, i) => {
    const d = join(dir, checkpointDir(i, o.name));
    mkdirSync(d, { recursive: true });
    for (const [channel, text] of Object.entries(o.channels)) writeFileSync(join(d, `${channel}.snap`), `${text}\n`);
  });
}
