import { act, renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useSaveSettingsMutation, useSettingsQuery } from "@/lib/query";
import type { Settings } from "@/types";

const getMock = vi.fn();
const saveMock = vi.fn();
vi.mock("@/lib/api", () => ({
  settingsApi: {
    get: () => getMock(),
    save: (settings: Settings) => saveMock(settings),
  },
}));

const saved: Settings = {
  showInTray: true,
  minimizeToTrayOnClose: true,
  language: "zh",
  webdavSync: { enabled: true, baseUrl: "https://fixture.invalid" },
  s3Sync: { enabled: true, bucket: "synthetic-bucket" },
};

function setup() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  const hook = renderHook(
    () => ({ query: useSettingsQuery(), mutation: useSaveSettingsMutation() }),
    {
      wrapper: ({ children }) => (
        <QueryClientProvider client={client}>{children}</QueryClientProvider>
      ),
    },
  );
  return { ...hook, client };
}

beforeEach(() => {
  getMock.mockReset().mockResolvedValue({ ...saved });
  saveMock.mockReset().mockResolvedValue(true);
});

describe("settings mutation acknowledgement", () => {
  it.each(["failure", "delay"])(
    "publishes acknowledged values and preserves omitted sync fields during refetch %s",
    async (outcome) => {
      const { result, client } = setup();
      await waitFor(() => expect(result.current.query.isSuccess).toBe(true));
      let finishRefetch!: () => void;
      if (outcome === "failure") {
        getMock.mockRejectedValueOnce(new Error("synthetic refetch failure"));
      } else {
        getMock.mockImplementationOnce(
          () =>
            new Promise<Settings>((resolve) => {
              finishRefetch = () => resolve({ ...saved, showInTray: false });
            }),
        );
      }
      let write!: Promise<unknown>;
      act(() => {
        write = result.current.mutation.mutateAsync({
          showInTray: false,
          minimizeToTrayOnClose: true,
          language: "zh",
        });
      });
      await waitFor(() => expect(getMock).toHaveBeenCalledTimes(2));
      expect(client.getQueryData<Settings>(["settings"])).toMatchObject({
        ...saved,
        showInTray: false,
      });
      if (outcome === "delay") {
        expect(client.isMutating()).toBe(1);
        await act(async () => finishRefetch());
      }
      await act(async () => {
        await write;
      });
      expect(client.getQueryData<Settings>(["settings"])?.showInTray).toBe(
        false,
      );
      if (outcome === "failure") {
        expect(client.getQueryState(["settings"])?.status).toBe("error");
      }
    },
  );

  it("keeps the confirmed cache unchanged when the native save rejects", async () => {
    const { result, client } = setup();
    await waitFor(() => expect(result.current.query.isSuccess).toBe(true));
    saveMock.mockRejectedValueOnce(new Error("synthetic write failure"));
    await act(async () => {
      await expect(
        result.current.mutation.mutateAsync({
          ...saved,
          showInTray: false,
        }),
      ).rejects.toThrow("synthetic write failure");
    });
    expect(client.getQueryData(["settings"])).toEqual(saved);
    expect(getMock).toHaveBeenCalledTimes(1);
  });
});
