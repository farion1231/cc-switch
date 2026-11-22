// Switching Codex from one custom provider to another, starting from data v4 wrote.
import { parityCase } from "#kit";
import { S } from "#screens";

export default parityCase({
  id: "mod-01.codex.direct-switch",
  covers: [["MOD-01", "codex"]],
  seed: { seed: "codex.two-custom-providers" },
  async journey({ ui, checkpoint, expect }) {
    await ui.click(S.shell.sidebarApp("Codex"));
    await checkpoint("alpha-in-use");

    await ui.click(S.providers.cardButton("Beta Relay", "Switch"));
    await checkpoint("beta-in-use");

    // Intent: Beta's endpoint and key are what the app's own config now holds, and Alpha can be
    // switched back to.
    expect.file("~/.codex/config.toml").toContain('base_url = "https://beta.relay.example/v1"');
    await expect.visible(S.providers.cardButton("Alpha Relay", "Switch"));
  },
});
