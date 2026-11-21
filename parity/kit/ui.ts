/**
 * The UI driver as cases see it: verbs over opaque targets.
 *
 * - A target is built only in parity/screens from a role and an accessible name. A case cannot
 *   pass a selector, a test id or coordinates.
 * - There is no sleep, no eval and no raw driver handle here.
 * - Every acting verb waits until its target is actionable (exactly one match, visible, enabled,
 *   on top at its centre, and still for two reads), acts through the platform's input, then waits
 *   until the UI tree is quiet.
 */
import type { Target } from "./internal/target.ts";

export type { Target };

/** Declares that a step is expected to change nothing, and why. */
export interface Inert {
  readonly inert: string;
}

export interface Ui {
  click(t: Target, opts?: Inert): Promise<void>;
  /** Clear the field, then type the text as key events. */
  fill(t: Target, text: string, opts?: Inert): Promise<void>;
  /** A chord in the neutral form `Mod+,`. `Mod` is Cmd on macOS and Ctrl elsewhere. */
  press(chord: string, opts?: Inert): Promise<void>;
  /** Wait until exactly one match is visible. Never acts. */
  see(t: Target): Promise<void>;
  /** Wait until nothing matches. Never acts. */
  gone(t: Target): Promise<void>;
}
