import { describe, expect, it } from "vitest";
import { isValidCodexLiveConfig, isValidCodexLiveEndpoint } from "./codexLive";

describe("Codex Live provider endpoints", () => {
  it("accepts relative call and sideband paths", () => {
    expect(isValidCodexLiveEndpoint("/live/", false)).toBe(true);
    expect(isValidCodexLiveEndpoint("live/{call_id}", true)).toBe(true);
    expect(
      isValidCodexLiveConfig({
        createEndpoint: "realtime/calls",
        sidebandEndpoint: "realtime/calls/{call_id}",
      }),
    ).toBe(true);
  });

  it("rejects absolute, traversing, and malformed sideband paths", () => {
    expect(isValidCodexLiveEndpoint("https://example.com/live", false)).toBe(
      false,
    );
    expect(isValidCodexLiveEndpoint("live/../admin", false)).toBe(false);
    expect(isValidCodexLiveEndpoint("live/{call_id}?token=x", true)).toBe(
      false,
    );
    expect(isValidCodexLiveEndpoint("live/no-call-id", true)).toBe(false);
    expect(isValidCodexLiveEndpoint("live/{call_id}/{call_id}", true)).toBe(
      false,
    );
  });
});
