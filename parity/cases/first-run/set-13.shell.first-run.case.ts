// First launch on an empty HOME: the welcome notice, what confirming it writes, and the official
// providers seeded for Claude Code.
import { parityCase } from "#kit";
import { S } from "#screens";

export default parityCase({
  id: "set-13.shell.first-run",
  covers: [["SET-13", "shell", "first-run-notice"], ["PRV-10", "claude"]],
  seed: "fresh",
  async journey({ ui, checkpoint, expect }) {
    await ui.see(S.firstRun.dialog);
    await checkpoint("welcome");
    expect.file("~/.cc-switch/settings.json").at("firstRunNoticeConfirmed").toBeAbsent();

    await ui.click(S.firstRun.gotIt);
    await ui.gone(S.firstRun.dialog);
    await checkpoint("notice-confirmed");
    expect.file("~/.cc-switch/settings.json").at("firstRunNoticeConfirmed").toBe(true);
    await expect.visible(S.providers.cardTitle("Claude Official"));
  },
});
