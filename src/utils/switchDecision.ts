/**
 * Pure decision logic for the routing auto-toggle feature.
 *
 * `decideSwitchAction` maps the current state (provider requirement, per-app
 * proxy takeover, the two opt-in settings) to one of the `SwitchAction`s below.
 * It is intentionally side-effect free so it can be unit tested exhaustively
 * against the full truth table (see `switchDecision.test.ts`) and reused from
 * UI code.
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
 * Decide which action to take for a provider switch.
 *
 * `isOfficial` always dominates `needsRouting`: an official-class provider is
 * never routed through the proxy (account-ban safety). Under takeover it goes
 * to the disable path; otherwise it just switches directly — it never reaches
 * the enable path, even if a contradictory config also looks "needs routing".
 *
 * The two confirm paths are symmetric: first encounter = confirm dialog (with a
 * "remember" checkbox that sets the corresponding auto-* setting); after
 * remember = silent direct switch.
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
