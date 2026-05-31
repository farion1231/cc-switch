import { describe, expect, it } from "vitest";
import {
  decideSwitchAction,
  type SwitchAction,
  type SwitchDecisionInput,
} from "../switchDecision";

/**
 * Typed expectation helper: keeps the expected literal constrained to the
 * SwitchAction union without relying on a (non-existent) type argument on
 * vitest's `toBe`.
 */
function expectAction(actual: SwitchAction, expected: SwitchAction): void {
  expect(actual).toBe(expected);
}

/**
 * Helper to build a fully-specified input with sensible defaults so each
 * test only states the fields it cares about.
 */
function makeInput(
  overrides: Partial<SwitchDecisionInput> = {},
): SwitchDecisionInput {
  return {
    needsRouting: false,
    isProxyTakeover: false,
    isOfficial: false,
    autoEnable: false,
    autoDisable: false,
    ...overrides,
  };
}

describe("decideSwitchAction — §4 state machine", () => {
  it("official + takeover + autoDisable=false → confirmDisable", () => {
    expectAction(
      decideSwitchAction(
        makeInput({
          isOfficial: true,
          isProxyTakeover: true,
          autoDisable: false,
        }),
      ),
      "confirmDisable",
    );
  });

  it("official + takeover + autoDisable=true → directDisable (remembered)", () => {
    expectAction(
      decideSwitchAction(
        makeInput({
          isOfficial: true,
          isProxyTakeover: true,
          autoDisable: true,
        }),
      ),
      "directDisable",
    );
  });

  it("needsRouting + !takeover + autoEnable=false → confirmEnable", () => {
    expectAction(
      decideSwitchAction(
        makeInput({
          needsRouting: true,
          isProxyTakeover: false,
          autoEnable: false,
        }),
      ),
      "confirmEnable",
    );
  });

  it("needsRouting + !takeover + autoEnable=true → directEnable (remembered)", () => {
    expectAction(
      decideSwitchAction(
        makeInput({
          needsRouting: true,
          isProxyTakeover: false,
          autoEnable: true,
        }),
      ),
      "directEnable",
    );
  });

  it("needsRouting + takeover (already routed) → direct", () => {
    // Routing is already active, so no enable confirmation is needed.
    expectAction(
      decideSwitchAction(
        makeInput({
          needsRouting: true,
          isProxyTakeover: true,
          // autoEnable value must not matter here.
          autoEnable: false,
        }),
      ),
      "direct",
    );
  });

  it("!needsRouting + !official → direct", () => {
    expectAction(
      decideSwitchAction(
        makeInput({
          needsRouting: false,
          isOfficial: false,
        }),
      ),
      "direct",
    );
  });

  it("official + !takeover → direct (not blocked when proxy not taking over)", () => {
    expectAction(
      decideSwitchAction(
        makeInput({
          isOfficial: true,
          isProxyTakeover: false,
          // Even with autoDisable=false, no block because no takeover.
          autoDisable: false,
        }),
      ),
      "direct",
    );
  });

  describe("branch precedence: official+takeover wins over needsRouting", () => {
    it("official + takeover + needsRouting + autoDisable=false → confirmDisable", () => {
      expectAction(
        decideSwitchAction(
          makeInput({
            isOfficial: true,
            isProxyTakeover: true,
            needsRouting: true,
            autoDisable: false,
            autoEnable: true,
          }),
        ),
        "confirmDisable",
      );
    });

    it("official + takeover + needsRouting + autoDisable=true → directDisable (remembered)", () => {
      expectAction(
        decideSwitchAction(
          makeInput({
            isOfficial: true,
            isProxyTakeover: true,
            needsRouting: true,
            autoDisable: true,
            autoEnable: true,
          }),
        ),
        "directDisable",
      );
    });

    it("official + !takeover + needsRouting → direct (official never enables routing)", () => {
      // Safety invariant: an official-class provider is never routed. A
      // contradictory config that is both official (broad) and needsRouting
      // must NOT reach confirmEnable.
      expectAction(
        decideSwitchAction(
          makeInput({
            isOfficial: true,
            isProxyTakeover: false,
            needsRouting: true,
            autoEnable: true,
          }),
        ),
        "direct",
      );
    });
  });

  describe("exhaustive truth table (all 32 combinations)", () => {
    const bools = [false, true] as const;

    function expected(input: SwitchDecisionInput): SwitchAction {
      // Independent reference implementation: isOfficial dominates needsRouting.
      if (input.isOfficial) {
        if (input.isProxyTakeover) {
          return input.autoDisable ? "directDisable" : "confirmDisable";
        }
        return "direct";
      }
      if (input.needsRouting && !input.isProxyTakeover) {
        return input.autoEnable ? "directEnable" : "confirmEnable";
      }
      return "direct";
    }

    for (const needsRouting of bools) {
      for (const isProxyTakeover of bools) {
        for (const isOfficial of bools) {
          for (const autoEnable of bools) {
            for (const autoDisable of bools) {
              const input: SwitchDecisionInput = {
                needsRouting,
                isProxyTakeover,
                isOfficial,
                autoEnable,
                autoDisable,
              };
              it(`matches reference for ${JSON.stringify(input)}`, () => {
                expect(decideSwitchAction(input)).toBe(expected(input));
              });
            }
          }
        }
      }
    }
  });
});
