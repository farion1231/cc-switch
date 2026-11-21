// The pinned clock. A parity build reads the app's time from the file named by
// CC_SWITCH_PARITY_CLOCK; the time stays where the harness puts it until the harness moves it.
import { mkdirSync, renameSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";

/** Every case starts here, on every OS and every day. */
export const CLOCK_ORIGIN_MS = Date.UTC(2031, 2, 14, 9, 0, 0);

export const CLOCK_ENV = "CC_SWITCH_PARITY_CLOCK";

export class PinnedClock {
  #ms = CLOCK_ORIGIN_MS;
  readonly file: string;
  constructor(file: string) {
    this.file = file;
  }

  get ms(): number {
    return this.#ms;
  }

  /** Replaces the file in one rename, so the app reads the old instant or the new one. */
  set(ms: number): void {
    this.#ms = ms;
    mkdirSync(dirname(this.file), { recursive: true });
    const tmp = `${this.file}.next`;
    writeFileSync(tmp, `${new Date(ms).toISOString()}\n`);
    // Windows refuses the rename while the app has the file open for a read; that lasts microseconds.
    for (let i = 0; ; i++) {
      try {
        renameSync(tmp, this.file);
        return;
      } catch (e) {
        const code = (e as NodeJS.ErrnoException).code;
        if (i >= 40 || (code !== "EPERM" && code !== "EBUSY" && code !== "EACCES")) throw e;
        Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 25);
      }
    }
  }

  reset(): void {
    this.set(CLOCK_ORIGIN_MS);
  }
}
