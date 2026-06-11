/**
 * Pure decision logic for the routing auto-toggle feature. Side-effect free so
 * it can be exhaustively unit tested against the full truth table
 * (`switchDecision.test.ts`) and reused from UI code.
 */

/**
 * The action the caller should take when the user attempts to switch
 * to a provider.
 *
 * - "direct":         perform the switch immediately, no routing change.
 * - "directEnable":   silently enable routing then switch (user already "remembered").
 * - "directDisable":  silently disable routing then switch (user already "remembered").
 * - "confirmEnable":  ask the user to confirm enabling routing first.
 * - "confirmDisable": ask the user to confirm disabling proxy takeover first.
 */
export type SwitchAction =
  | "direct"
  | "directEnable"
  | "directDisable"
  | "confirmEnable"
  | "confirmDisable";

/**
 * Inputs to the switch decision state machine.
 *
 * - needsRouting:    the target provider requires routing to be enabled.
 * - isProxyTakeover: a proxy is currently taking over (routing is active).
 * - isOfficial:      the target provider is the official provider.
 * - autoEnable:      auto-enable routing without confirmation is allowed.
 * - autoDisable:     auto-disable proxy takeover without a hard block is allowed.
 */
export interface SwitchDecisionInput {
  needsRouting: boolean;
  isProxyTakeover: boolean;
  isOfficial: boolean;
  autoEnable: boolean;
  autoDisable: boolean;
}

/**
 * `isOfficial` always dominates `needsRouting`: an official-class provider is
 * never routed through the proxy (account-ban safety), so it never reaches the
 * enable path even if a contradictory config also looks "needs routing".
 */
export function decideSwitchAction(input: SwitchDecisionInput): SwitchAction {
  const { needsRouting, isProxyTakeover, isOfficial, autoEnable, autoDisable } =
    input;

  if (isOfficial) {
    // Official under takeover → disable routing before switching; otherwise
    // just switch. Either way, never enable routing for an official provider.
    if (isProxyTakeover) {
      return autoDisable ? "directDisable" : "confirmDisable";
    }
    return "direct";
  }

  if (needsRouting && !isProxyTakeover) {
    return autoEnable ? "directEnable" : "confirmEnable";
  }

  return "direct";
}
