// Gemini CLI with two custom providers: Alpha Relay, added first and so in use, and Beta Relay.
import { paritySeed } from "#kit";
import { S } from "#screens";
import { addCustomProvider } from "./_recipes.ts";

export default paritySeed({
  name: "gemini.two-custom-providers",
  async build({ ui, advanceClock }) {
    await ui.click(S.firstRun.gotIt);
    await ui.click(S.shell.sidebarApp("Gemini CLI"));
    await addCustomProvider(ui, { name: "Alpha Relay", apiKey: "sk-parity-alpha", endpointField: S.providers.add.apiEndpoint, endpoint: "https://alpha.relay.example" });
    // A minute apart, the two have distinct creation times and so a fixed order.
    await advanceClock({ minutes: 1 });
    await addCustomProvider(ui, { name: "Beta Relay", apiKey: "sk-parity-beta", endpointField: S.providers.add.apiEndpoint, endpoint: "https://beta.relay.example" });
  },
});
