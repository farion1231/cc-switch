import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ScriptsTab } from "@/components/codex-workbench/ScriptsTab";

const mocks = vi.hoisted(() => ({
  openScriptsDir: vi.fn(),
}));

const queryResult = {
  data: undefined,
  error: null,
  isLoading: false,
  isFetching: false,
};

const mutationResult = {
  mutate: vi.fn(),
  mutateAsync: vi.fn(),
  error: null,
  isPending: false,
};

vi.mock("@/lib/query/codexWorkbench", () => ({
  useCodexUserScriptsQuery: () => ({ ...queryResult, data: [] }),
  useCodexScriptMarketQuery: () => ({ ...queryResult, data: null }),
  useRefreshCodexScriptMarket: () => mutationResult,
  useInstallCodexMarketScript: () => mutationResult,
  useSetCodexUserScriptEnabled: () => mutationResult,
  useDeleteCodexUserScript: () => mutationResult,
  useImportCodexUserScript: () => mutationResult,
  useOpenCodexScriptsDir: () => ({
    ...mutationResult,
    mutateAsync: mocks.openScriptsDir,
  }),
}));

describe("ScriptsTab", () => {
  beforeEach(() => {
    mocks.openScriptsDir.mockReset().mockResolvedValue(undefined);
  });

  it("opens the scripts directory through the backend command", async () => {
    render(<ScriptsTab />);

    fireEvent.click(
      screen.getByRole("button", {
        name: /codexWorkbench\.scripts\.openFolder|打开脚本目录/,
      }),
    );

    await waitFor(() => expect(mocks.openScriptsDir).toHaveBeenCalledOnce());
  });
});
