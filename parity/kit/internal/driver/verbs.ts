// The verb engine: turns Ui calls into waits, platform input and settles. It never sleeps for a
// fixed time to let the app "catch up"; every wait is for an observable condition with a deadline.
import type { Inert, Ui } from "../../ui.ts";
import { describe, specOf, type Target } from "../target.ts";
import type { AppSession } from "./app.ts";
import { TREE_CMD, type ResolveResult, type TreeResult } from "./probe.ts";

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

export class StepError extends Error {
  readonly tree: string | null;
  constructor(message: string, tree: string | null) {
    super(message);
    this.tree = tree;
  }
}

export interface StepLog {
  readonly verb: string;
  readonly target?: string;
  readonly ms: number;
}

/** Reads the tree until it is the same three reads in a row. Returns the settled tree. */
export async function settleUi(app: AppSession, timeoutMs = 15000): Promise<TreeResult> {
  const start = Date.now();
  let last = "", same = 0;
  let tree: TreeResult | null = null;
  while (Date.now() - start < timeoutMs) {
    tree = await app.probe<TreeResult>(TREE_CMD);
    const key = JSON.stringify(tree);
    if (key === last) {
      if (++same >= 2) return tree;
    } else {
      same = 0;
      last = key;
    }
    await sleep(250);
  }
  throw new StepError(`the UI did not settle within ${timeoutMs} ms`, tree?.text ?? null);
}

/**
 * The run's view of acting steps. `before` runs once the step's target is actionable and before
 * it acts; `after` runs once the step has acted and the UI has settled, with the reason when the
 * step declared itself inert.
 */
export interface StepHooks {
  before(): Promise<void>;
  after(step: { verb: string; target?: string; inert?: string }): Promise<void>;
}

export function makeUi(app: AppSession, steps: StepLog[], hooks: StepHooks, deadlineMs = 10000): Ui {
  const treeText = async () => (await app.probe<TreeResult>(TREE_CMD).catch(() => null))?.text ?? null;

  // Waits until the target resolves to one actionable element, still across two reads.
  type Ok = Extract<ResolveResult, { count: 1 }>;
  const actionable = async (t: Target, element = false): Promise<Ok> => {
    const spec = specOf(t);
    const start = Date.now();
    let last: ResolveResult | null = null;
    let prev: ResolveResult | null = null;
    while (Date.now() - start < deadlineMs) {
      const r = await app.probe<ResolveResult>({ cmd: "resolve", target: spec, element, act: true });
      if (r.state === "ok" && prev?.state === "ok" && prev.x === r.x && prev.y === r.y && prev.w === r.w && prev.h === r.h) return r as Ok;
      prev = r;
      last = r;
      await sleep(100);
    }
    const why =
      !last ? "never resolved"
      : last.state === "none" ? `no match${last.why ? ` (${last.why})` : ""}`
      : last.state === "ambiguous" ? `${last.count} matches`
      : last.state === "covered" ? `covered by ${last.coveredBy}`
      : last.state;
    throw new StepError(`${describe(spec)} is not actionable after ${deadlineMs} ms: ${why}`, await treeText());
  };

  const step = async <R>(verb: string, t: Target | string | undefined, ready: () => Promise<R>, act: (r: R) => Promise<void>, opts?: Inert) => {
    const start = Date.now();
    const target = t === undefined ? undefined : typeof t === "string" ? t : describe(specOf(t));
    const r = await ready();
    await hooks.before();
    await act(r);
    await settleUi(app);
    await hooks.after({ verb: `ui.${verb}`, target, inert: opts?.inert });
    steps.push({ verb, target, ms: Date.now() - start });
  };

  const waitFor = async (t: Target, want: "visible" | "gone") => {
    const spec = specOf(t);
    const start = Date.now();
    let last: ResolveResult | null = null;
    while (Date.now() - start < deadlineMs) {
      last = await app.probe<ResolveResult>({ cmd: "resolve", target: spec });
      if (want === "gone" && last.state === "none") return;
      if (want === "visible" && last.count === 1 && last.state !== "zero-size") return;
      await sleep(100);
    }
    throw new StepError(`${describe(spec)} did not become ${want} within ${deadlineMs} ms (last: ${last?.state}, ${last?.count} matches)`, await treeText());
  };

  return {
    click: (t, opts) => step("click", t, () => actionable(t), async (r) => {
      await app.click(r.x, r.y);
    }, opts),
    fill: (t, text, opts) => step("fill", t, () => actionable(t, true), async (r) => {
      await app.click(r.x, r.y);
      await app.chord([app.mod, "a"]);
      await app.chord(["Backspace"]);
      await app.type(r.element, text);
    }, opts),
    press: (chord, opts) => step("press", chord, async () => undefined, async () => {
      await app.chord(chord.split("+").map((k) => (k === "Mod" ? app.mod : k)));
    }, opts),
    see: async (t) => {
      const start = Date.now();
      await waitFor(t, "visible");
      steps.push({ verb: "see", target: describe(specOf(t)), ms: Date.now() - start });
    },
    gone: async (t) => {
      const start = Date.now();
      await waitFor(t, "gone");
      steps.push({ verb: "gone", target: describe(specOf(t)), ms: Date.now() - start });
    },
  };
}

/** Resolves a target once, for intent checks. */
export async function isVisible(app: AppSession, t: Target): Promise<boolean> {
  const r = await app.probe<ResolveResult>({ cmd: "resolve", target: specOf(t) });
  return r.count === 1 && r.state !== "zero-size";
}
