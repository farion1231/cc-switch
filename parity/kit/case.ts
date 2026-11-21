/**
 * Public surface of the parity kit. This file, ui.ts and the screen map are everything a case may
 * import.
 *
 * A case is one user journey. It can act on the UI, act as the user outside the window, take
 * checkpoints that observe every channel at once and compare them with the v4 recording, and
 * state its intent in a few hand-written expectations. A case says nothing about the OS, the
 * clock or the route.
 */
import type { Target, Ui } from "./ui.ts";

export type AppId =
  | "claude" | "claude-desktop" | "codex" | "gemini" | "grokbuild" | "opencode" | "openclaw" | "hermes"
  | "pi" | "mcode" | "kimi" | "dsh" | "agy" | "omp" | "copilot" | "workbuddy";

/** One cell of the coverage matrix: a feature id from the feature list, an app (or "shell"), and an optional aspect. */
export type Cell = readonly [feature: string, app: AppId | "shell", aspect?: string];

/** Starting user data. "fresh" is an empty HOME, which is the first-run journey. */
export type SeedRef = "fresh" | { readonly seed: string };

export type HomePath = `~/${string}`;

export interface CaseSpec {
  /** `<feature>.<app|shell>.<slug>`, lower case. Must equal the file name. */
  readonly id: string;
  readonly covers: readonly [Cell, ...Cell[]];
  readonly seed: SeedRef;
  journey(j: Journey): Promise<void>;
}

export interface Journey {
  readonly ui: Ui;
  /**
   * Wait until the app is quiet, then capture every channel and compare each with the recording
   * for this case and checkpoint. In record mode the observations become the recording.
   */
  checkpoint(name: string): Promise<void>;
  readonly expect: Expect;
}

export interface Expect {
  file(path: HomePath): {
    toExist(): void;
    toBeAbsent(): void;
    toContain(text: string): void;
    /** JSON files only for now. `path` is dot-separated. */
    at(path: string): { toBe(value: unknown): void; toBeAbsent(): void };
  };
  visible(target: Target): Promise<void>;
}

const ID = /^[a-z0-9-]+\.[a-z0-9-]+\.[a-z0-9-]+$/;

/** The only way to declare a case. Default-export the result. */
export function parityCase(spec: CaseSpec): CaseSpec {
  if (!ID.test(spec.id)) throw new Error(`case id "${spec.id}" must be <feature>.<app|shell>.<slug> in lower case`);
  if (spec.covers.length === 0) throw new Error(`case ${spec.id} covers nothing`);
  return Object.freeze(spec);
}
