/**
 * Pure decision logic for the routing auto-toggle feature.
 *
 * Implements the decision state machine described in
 * docs/design/routing-auto-toggle.md §4.
 *
 * The function is intentionally side-effect free so it can be unit tested
 * exhaustively against the truth table and reused from UI code.
 */

/**
 * The action the caller should take when the user attempts to switch
 * to a provider.
 *
 * - "direct":         perform the switch immediately, no confirmation.
 * - "confirmEnable":  ask the user to confirm enabling routing first.
 * - "confirmDisable": ask the user to confirm disabling proxy takeover first.
 * - "hardBlock":      refuse the switch; it is not permitted.
 */
export type SwitchAction =
  | "direct"
  | "confirmEnable"
  | "confirmDisable"
  | "hardBlock";

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
 * Decide which action to take for a provider switch.
 *
 * The branch order is significant: the official + proxy-takeover branch is
 * evaluated first and wins even when `needsRouting` is also true.
 */
export function decideSwitchAction(input: SwitchDecisionInput): SwitchAction {
  const { needsRouting, isProxyTakeover, isOfficial, autoEnable, autoDisable } =
    input;

  if (isOfficial && isProxyTakeover) {
    return autoDisable ? "confirmDisable" : "hardBlock";
  }

  if (needsRouting && !isProxyTakeover) {
    return autoEnable ? "direct" : "confirmEnable";
  }

  return "direct";
}
