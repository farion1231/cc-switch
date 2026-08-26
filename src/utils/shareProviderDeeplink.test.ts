import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/api/deeplink", () => ({
  deeplinkApi: {
    generateProviderDeeplink: vi.fn(),
  },
}));
vi.mock("@/lib/clipboard", () => ({
  copyText: vi.fn(),
}));

import { deeplinkApi } from "@/lib/api/deeplink";
import { copyText } from "@/lib/clipboard";
import { shareProviderDeeplink } from "@/utils/shareProviderDeeplink";

describe("shareProviderDeeplink", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("generates the link and copies it to the clipboard", async () => {
    vi.mocked(deeplinkApi.generateProviderDeeplink).mockResolvedValue(
      "ccswitch://v1/import?resource=provider&app=claude",
    );
    vi.mocked(copyText).mockResolvedValue();

    const url = await shareProviderDeeplink("claude", "p-1");

    expect(deeplinkApi.generateProviderDeeplink).toHaveBeenCalledWith(
      "claude",
      "p-1",
    );
    expect(copyText).toHaveBeenCalledWith(
      "ccswitch://v1/import?resource=provider&app=claude",
    );
    expect(url).toBe("ccswitch://v1/import?resource=provider&app=claude");
  });

  it("propagates backend errors without touching the clipboard", async () => {
    vi.mocked(deeplinkApi.generateProviderDeeplink).mockRejectedValue(
      "This provider cannot be shared",
    );

    await expect(shareProviderDeeplink("codex", "p-2")).rejects.toBe(
      "This provider cannot be shared",
    );
    expect(copyText).not.toHaveBeenCalled();
  });
});
