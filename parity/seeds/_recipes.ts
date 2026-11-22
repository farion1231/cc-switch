// Steps several seed recipes share.
import type { Target, Ui } from "#kit";
import { S } from "#screens";

/**
 * Adds a provider through the Custom Configuration form of the app that is open. `endpointField`
 * is the form's endpoint field, which apps label differently.
 */
export async function addCustomProvider(ui: Ui, p: { name: string; apiKey: string; endpointField: Target; endpoint: string }): Promise<void> {
  await ui.click(S.providers.addButton);
  await ui.click(S.providers.add.customConfig);
  await ui.fill(S.providers.add.name, p.name);
  await ui.fill(S.providers.add.apiKey, p.apiKey);
  await ui.fill(p.endpointField, p.endpoint);
  await ui.click(S.providers.add.submit);
  await ui.see(S.providers.card(p.name));
}
