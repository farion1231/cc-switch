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
  it("official + takeover + autoDisable=false → hardBlock", () => {
    expectAction(
      decideSwitchAction(
        makeInput({
          isOfficial: true,
          isProxyTakeover: true,
          autoDisable: false,
        }),
      ),
      "hardBlock",
    );
  });

  it("official + takeover + autoDisable=true → confirmDisable", () => {
    expectAction(
      decideSwitchAction(
        makeInput({
          isOfficial: true,
          isProxyTakeover: true,
          autoDisable: true,
        }),
      ),
      "confirmDisable",
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

  it("needsRouting + !takeover + autoEnable=true → direct", () => {
    expectAction(
      decideSwitchAction(
        makeInput({
          needsRouting: true,
          isProxyTakeover: false,
          autoEnable: true,
        }),
      ),
      "direct",
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
    it("official + takeover + needsRouting + autoDisable=false → hardBlock", () => {
      expectAction(
        decideSwitchAction(
          makeInput({
            isOfficial: true,
            isProxyTakeover: true,
            needsRouting: true,
            autoDisable: false,
            // autoEnable=true would point the second branch at "direct";
            // precedence must still yield the first branch.
            autoEnable: true,
          }),
        ),
        "hardBlock",
      );
    });

    it("official + takeover + needsRouting + autoDisable=true → confirmDisable", () => {
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
        "confirmDisable",
      );
    });
  });

  describe("exhaustive truth table (all 32 combinations)", () => {
    const bools = [false, true] as const;

    function expected(input: SwitchDecisionInput): SwitchAction {
      // Independent reference implementation of §4, used to cross-check
      // every combination of the five boolean inputs.
      if (input.isOfficial && input.isProxyTakeover) {
        return input.autoDisable ? "confirmDisable" : "hardBlock";
      }
      if (input.needsRouting && !input.isProxyTakeover) {
        return input.autoEnable ? "direct" : "confirmEnable";
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
