// The Usage page on a fresh HOME, then the same page a day later. The page's ranges are relative
// to today, so it is the first page whose content depends on the clock.
import { parityCase } from "#kit";
import { S } from "#screens";

export default parityCase({
  id: "usg-01.shell.usage-page",
  covers: [["USG-01", "shell", "empty"]],
  seed: "fresh",
  async journey({ ui, checkpoint, advanceClock }) {
    await ui.click(S.firstRun.gotIt);
    await ui.gone(S.firstRun.dialog);
    await ui.click(S.shell.section("Usage"));
    await checkpoint("usage");

    await advanceClock({ days: 1 });
    await checkpoint("next-day");
  },
});
