import { renderHook, waitFor, act } from "@testing-library/react";
import { useCodexCommonConfig } from "@/components/providers/forms/hooks/useCodexCommonConfig";

const api = vi.hoisted(() => ({
  getCommonConfigSnippet: vi.fn(async (_app: string): Promise<string> => ""),
  setCommonConfigSnippet: vi.fn(async () => true),
  updateTomlCommonConfigSnippet: vi.fn(async (config: string) => config),
  extractCommonConfigSnippet: vi.fn(async () => 'name = "desktop"'),
}));
vi.mock("@/lib/api", () => ({ configApi: api }));

describe("Codex Desktop common configuration isolation", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.clear();
  });
  it("does not migrate the CLI's legacy snippet into Desktop", async () => {
    localStorage.setItem(
      "cc-switch:codex-common-config-snippet",
      "cli_only = true",
    );
    const { result } = renderHook(() =>
      useCodexCommonConfig({
        appId: "codex-desktop",
        codexConfig: "",
        onConfigChange: vi.fn(),
      }),
    );
    await waitFor(() =>
      expect(api.getCommonConfigSnippet).toHaveBeenCalledWith("codex-desktop"),
    );
    await waitFor(() => expect(result.current.isLoading).toBe(false));
    expect(api.setCommonConfigSnippet).not.toHaveBeenCalled();
    expect(result.current.commonConfigSnippet).not.toContain("cli_only");
    expect(localStorage.getItem("cc-switch:codex-common-config-snippet")).toBe(
      "cli_only = true",
    );
  });
  it("reloads the target snippet when the selected app changes", async () => {
    api.getCommonConfigSnippet.mockImplementation(
      async (app) => `name = "${app}"`,
    );
    const { result, rerender } = renderHook(
      ({ appId }: { appId: "codex" | "codex-desktop" }) =>
        useCodexCommonConfig({
          appId,
          codexConfig: "",
          onConfigChange: vi.fn(),
        }),
      { initialProps: { appId: "codex" } },
    );
    await waitFor(() =>
      expect(result.current.commonConfigSnippet).toBe('name = "codex"'),
    );
    act(() => rerender({ appId: "codex-desktop" }));
    await waitFor(() =>
      expect(result.current.commonConfigSnippet).toBe('name = "codex-desktop"'),
    );
  });
});
