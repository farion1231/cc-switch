// Adding a custom Codex provider through the two-step add page.
import { parityCase } from "#kit";
import { S } from "#screens";

export default parityCase({
  id: "prv-01.codex.add-custom",
  covers: [["PRV-01", "codex", "add"], ["PRV-02", "codex", "custom"]],
  seed: "fresh",
  async journey({ ui, checkpoint, expect }) {
    await ui.click(S.firstRun.gotIt);
    await ui.click(S.shell.sidebarApp("Codex"));
    await ui.click(S.providers.addButton);
    await ui.click(S.providers.add.customConfig);
    await ui.fill(S.providers.add.name, "Beta Relay");
    await ui.fill(S.providers.add.apiKey, "sk-parity-beta");
    await ui.fill(S.providers.add.requestUrl, "https://beta.relay.example/v1");
    await checkpoint("form-filled");

    await ui.click(S.providers.add.submit);
    await ui.see(S.providers.cardTitle("Beta Relay"));
    await checkpoint("added");
    await expect.visible(S.providers.cardTitle("OpenAI Official"));
  },
});
