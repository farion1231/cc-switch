// Switching Claude Desktop from one custom provider to another, starting from data v4 wrote.
import { parityCase } from "#kit";
import { S } from "#screens";

export default parityCase({
  id: "mod-01.claude-desktop.direct-switch",
  covers: [["MOD-01", "claude-desktop"]],
  seed: { seed: "claude-desktop.two-custom-providers" },
  async journey({ ui, checkpoint, expect }) {
    await ui.click(S.shell.sidebarApp("Claude Desktop"));
    await checkpoint("alpha-in-use");

    await ui.click(S.providers.cardButton("Beta Relay", "Switch"));
    await checkpoint("beta-in-use");

    // Intent: Alpha can be switched back to. Claude Desktop's config sits at a different path on
    // each OS, so what it holds is judged by the recorded files.
    await expect.visible(S.providers.cardButton("Alpha Relay", "Switch"));
  },
});
