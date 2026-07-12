import type { PropsWithChildren } from "react";
import { act, renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { pluginKeys, usePluginMutation, usePlugins } from "@/hooks/usePlugins";

const listMock = vi.hoisted(() => vi.fn());
const installMock = vi.hoisted(() => vi.fn());

vi.mock("@/lib/api/plugins", () => ({
  pluginsApi: {
    list: listMock,
    install: installMock,
  },
}));

function setup() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  const wrapper = ({ children }: PropsWithChildren) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
  return { client, wrapper };
}

describe("plugin queries", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    listMock.mockResolvedValue([]);
    installMock.mockResolvedValue({
      success: true,
      requiresRestart: true,
      commandSummary: "claude plugin install ponytail@ponytail",
    });
  });

  it("keeps client and discovery queries independent", async () => {
    const { wrapper } = setup();
    const { result } = renderHook(() => usePlugins("codex", true), {
      wrapper,
    });

    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(listMock).toHaveBeenCalledWith("codex", true);
  });

  it("passes Claude scope and invalidates plugin state after install", async () => {
    const { client, wrapper } = setup();
    const invalidate = vi.spyOn(client, "invalidateQueries");
    const { result } = renderHook(() => usePluginMutation(), { wrapper });

    await act(async () => {
      await result.current.mutateAsync({
        action: "install",
        app: "claude",
        pluginId: "ponytail@ponytail",
        scope: "project",
        projectPath: "/repo",
      });
    });

    expect(installMock).toHaveBeenCalledWith(
      "claude",
      "ponytail@ponytail",
      "project",
      "/repo",
    );
    expect(invalidate).toHaveBeenCalledWith({ queryKey: pluginKeys.all });
  });
});
