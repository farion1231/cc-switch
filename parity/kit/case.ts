/**
 * Public surface of the parity kit. This file, ui.ts and the screen map are everything a case may
 * import.
 *
 * A case is one user journey. It can act on the UI, act as the user outside the window, take
 * checkpoints that observe every channel at once and compare them with the v4 recording, and
 * state its intent in a few hand-written expectations. A case says nothing about the OS, the
 * clock or the route.
 *
 * A checkpoint observes what the app writes and sends, not what its window shows; the agentic
 * end-to-end tests judge the UI. State the app keeps only in its own database reaches a recording
 * when the case makes the app write it out, for example by switching to it or by exporting it.
 */
import type { Target, Ui } from "./ui.ts";

export type AppId =
  | "claude" | "claude-desktop" | "codex" | "gemini" | "grokbuild" | "opencode" | "openclaw" | "hermes"
  | "pi" | "mcode" | "kimi" | "dsh" | "agy" | "omp" | "copilot" | "workbuddy";

/** One cell of the coverage matrix: a feature id from the feature list, an app (or "shell"), and an optional aspect. */
export type Cell = readonly [feature: string, app: AppId | "shell", aspect?: string];

/**
 * Starting user data. "fresh" is an empty HOME, which is the first-run journey. A named seed is
 * the HOME a seed recipe (`parity/seeds/<name>.seed.ts`) left behind on v4.
 */
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
  /**
   * Moves the app's time forward. Time starts at the same instant in every run and stands still
   * otherwise, so anything that depends on time passing happens only when a case says so.
   *
   * Some ids come from the time (prompts, deep-link imports, backups), so two of them made at the
   * same instant are the same id and the second replaces the first, as in v4 when both land in
   * one millisecond or second. A case that makes more than one advances the clock between them
   * and checks that every one of them exists.
   */
  advanceClock(by: Duration): Promise<void>;
  readonly expect: Expect;
}

export interface Duration {
  readonly days?: number;
  readonly hours?: number;
  readonly minutes?: number;
  readonly seconds?: number;
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

/** What a seed recipe can do: act on the UI, move the clock and state what it expects. */
export interface SeedJourney {
  readonly ui: Ui;
  advanceClock(by: Duration): Promise<void>;
  readonly expect: Expect;
}

/**
 * User data a case starts from, made through the UI. The recipe runs on v4 when the seed is
 * recorded, starting from an empty HOME one day before the cases' start, and what it leaves under
 * HOME is committed with the recordings. A case that names the seed starts from that data, so on
 * v5 it also proves that v5 takes over what v4 wrote. Changing the recipe means recording the seed
 * and every case that uses it again.
 */
export interface SeedSpec {
  /** `<app>.<slug>`, lower case. Must equal the file name. */
  readonly name: string;
  build(j: SeedJourney): Promise<void>;
}

const ID = /^[a-z0-9-]+\.[a-z0-9-]+\.[a-z0-9-]+$/;
const SEED = /^[a-z0-9-]+\.[a-z0-9-]+$/;

/** The only way to declare a seed. Default-export the result. */
export function paritySeed(spec: SeedSpec): SeedSpec {
  if (!SEED.test(spec.name)) throw new Error(`seed name "${spec.name}" must be <app>.<slug> in lower case`);
  return Object.freeze(spec);
}

/** The only way to declare a case. Default-export the result. */
export function parityCase(spec: CaseSpec): CaseSpec {
  if (!ID.test(spec.id)) throw new Error(`case id "${spec.id}" must be <feature>.<app|shell>.<slug> in lower case`);
  if (spec.covers.length === 0) throw new Error(`case ${spec.id} covers nothing`);
  return Object.freeze(spec);
}
