import type { QueryClient } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { invalidateUniversalProviderCaches } from "@/lib/query/mutations";

describe("invalidateUniversalProviderCaches", () => {
  it("refreshes provider lists and Hermes live/model state", async () => {
    const invalidateQueries = vi.fn().mockResolvedValue(undefined);
    const queryClient = { invalidateQueries } as unknown as QueryClient;

    await invalidateUniversalProviderCaches(queryClient);

    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: ["providers"],
    });
    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: ["hermes", "liveProviderIds"],
    });
    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: ["hermes", "modelConfig"],
    });
  });
});
