// Switching Claude Code from one custom provider to another, starting from data v4 wrote.
import { parityCase } from "#kit";
import { S } from "#screens";

export default parityCase({
  id: "mod-01.claude.direct-switch",
  covers: [["MOD-01", "claude"]],
  seed: { seed: "claude.two-custom-providers" },
  async journey({ ui, checkpoint, expect }) {
    await ui.click(S.shell.sidebarApp("Claude Code"));
    await checkpoint("alpha-in-use");

    await ui.click(S.providers.cardButton("Beta Relay", "Switch"));
    await checkpoint("beta-in-use");

    // Intent: Beta's endpoint and key are what the app's own config now holds, and Alpha can be
    // switched back to.
    expect.file("~/.claude/settings.json").at("env.ANTHROPIC_BASE_URL").toBe("https://beta.relay.example");
    await expect.visible(S.providers.cardButton("Alpha Relay", "Switch"));
  },
});
