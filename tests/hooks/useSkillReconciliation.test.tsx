import type { PropsWithChildren } from "react";
import { act, renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { expect, it, vi } from "vitest";
import {
  useRemoveSkillReconciliationDuplicate,
  useResyncSkillsToApps,
} from "@/hooks/useSkills";
const remove = vi.hoisted(() => vi.fn());
const resync = vi.hoisted(() => vi.fn());
vi.mock("@/lib/api/skills", () => ({
  skillsApi: { removeReconciliationDuplicate: remove, resyncToApps: resync },
}));
const wrapperFor =
  (client: QueryClient) =>
  ({ children }: PropsWithChildren) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );

it.each([false, true])(
  "refreshes once after a reconciliation batch, including partial failure (%s)",
  async (fail) => {
    const client = new QueryClient({
      defaultOptions: { mutations: { retry: false } },
    });
    const invalidate = vi
      .spyOn(client, "invalidateQueries")
      .mockResolvedValue();
    let release!: () => void;
    remove.mockReset().mockImplementationOnce(
      () =>
        new Promise<void>((resolve) => {
          release = resolve;
        }),
    );
    if (fail) remove.mockRejectedValueOnce(new Error("cannot remove"));
    else remove.mockResolvedValueOnce(true);
    const { result } = renderHook(
      () => useRemoveSkillReconciliationDuplicate(),
      { wrapper: wrapperFor(client) },
    );
    let pending!: Promise<unknown>;
    act(() => {
      pending = result.current.mutateAsync([
        { app: "codex", directory: "a" },
        { app: "gemini", directory: "b" },
      ]);
      void pending.catch(() => undefined);
    });
    await waitFor(() => expect(remove).toHaveBeenCalledTimes(1));
    expect(invalidate).not.toHaveBeenCalled();
    await act(async () => {
      release();
      if (fail) await expect(pending).rejects.toThrow("cannot remove");
      else await pending;
    });
    expect(remove).toHaveBeenCalledTimes(2);
    expect(invalidate).toHaveBeenCalledTimes(3);
    for (const key of ["reconciliation", "installed", "unmanaged"]) {
      expect(invalidate).toHaveBeenCalledWith({ queryKey: ["skills", key] });
    }
  },
);

it("invalidates reconciliation after resync rejects", async () => {
  const client = new QueryClient({
    defaultOptions: { mutations: { retry: false } },
  });
  const invalidate = vi.spyOn(client, "invalidateQueries");
  resync.mockRejectedValueOnce(new Error("partial resync"));
  const { result } = renderHook(() => useResyncSkillsToApps(), {
    wrapper: wrapperFor(client),
  });
  await act(async () => {
    await expect(result.current.mutateAsync()).rejects.toThrow(
      "partial resync",
    );
  });
  expect(invalidate).toHaveBeenCalledWith({
    queryKey: ["skills", "reconciliation"],
  });
});
