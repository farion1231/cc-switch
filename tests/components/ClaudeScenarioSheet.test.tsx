import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { http, HttpResponse } from "msw";
import { toast } from "sonner";
import { ClaudeScenarioSheet } from "@/components/providers/mode/ClaudeScenarioSheet";
import type { ClaudeScenarioView } from "@/types/proxy";
import { server } from "../msw/server";
import { createTestQueryClient } from "../utils/testQueryClient";

vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    info: vi.fn(),
    warning: vi.fn(),
    error: vi.fn(),
  },
}));

const TAURI_ENDPOINT = "http://tauri.local";

const MODELS = [
  { id: "ccs-claude-kimi--k3", label: "k3（Kimi）" },
  { id: "ccs-claude-zhipu--air", label: "air（Zhipu）" },
];

function mockScenarios(view: ClaudeScenarioView) {
  server.use(
    http.post(`${TAURI_ENDPOINT}/get_claude_stack_scenarios`, () =>
      HttpResponse.json(view),
    ),
  );
}

function renderSheet() {
  const queryClient = createTestQueryClient();
  return render(
    <QueryClientProvider client={queryClient}>
      <ClaudeScenarioSheet app="claude" open onOpenChange={vi.fn()} />
    </QueryClientProvider>,
  );
}

describe("ClaudeScenarioSheet", () => {
  beforeEach(() => {
    vi.mocked(toast.success).mockClear();
    // jsdom 里下拉打开时会调
    Element.prototype.scrollIntoView = vi.fn();
  });

  it("binds a picked model to its scenario and saves every row", async () => {
    const user = userEvent.setup();
    mockScenarios({ scenarios: {}, models: MODELS });
    const calls: unknown[] = [];
    server.use(
      http.post(
        `${TAURI_ENDPOINT}/set_claude_stack_scenarios`,
        async ({ request }) => {
          calls.push(await request.json());
          return HttpResponse.json(null);
        },
      ),
    );
    renderSheet();

    // 四档 + 子代理 + 辅助 / 压缩七个下拉都在。
    for (const row of [
      "haiku",
      "sonnet",
      "opus",
      "fable",
      "subagent",
      "auxiliary",
      "compaction",
    ]) {
      expect(
        await screen.findByLabelText(`mode.scenarios.${row}`),
      ).toBeInTheDocument();
    }

    // 没绑时是「跟随默认」；改绑 haiku → zhipu 的模型、辅助请求 → kimi 的模型。
    await user.click(screen.getByLabelText("mode.scenarios.haiku"));
    await user.click(
      await screen.findByRole("option", { name: "air（Zhipu）" }),
    );
    await user.click(screen.getByLabelText("mode.scenarios.auxiliary"));
    await user.click(await screen.findByRole("option", { name: "k3（Kimi）" }));

    await user.click(screen.getByRole("button", { name: "common.save" }));
    await expect.poll(() => calls.length).toBeGreaterThan(0);
    expect(calls[0]).toEqual({
      appType: "claude",
      scenarios: {
        haiku: "ccs-claude-zhipu--air",
        sonnet: null,
        opus: null,
        fable: null,
        subagent: null,
        auxiliary: "ccs-claude-kimi--k3",
        compaction: null,
      },
    });
    expect(toast.success).toHaveBeenCalledWith(
      "mode.scenarios.saved",
      expect.anything(),
    );
  });

  it("marks a binding whose model is no longer published", async () => {
    mockScenarios({
      scenarios: { haiku: "ccs-claude-gone--g-1" },
      models: MODELS,
    });
    renderSheet();

    expect(await screen.findByText("mode.scenarios.stale")).toBeInTheDocument();
    // 失效的绑定照原样列出来，换成别的或跟随默认都行。
    expect(
      screen.getByRole("combobox", { name: "mode.scenarios.haiku" }),
    ).toHaveTextContent("ccs-claude-gone--g-1");
  });

  it("explains when there is nothing to bind yet", async () => {
    mockScenarios({ scenarios: {}, models: [] });
    renderSheet();

    expect(await screen.findByText("mode.scenarios.empty")).toBeInTheDocument();
    expect(
      screen.getByRole("combobox", { name: "mode.scenarios.haiku" }),
    ).toBeDisabled();
    expect(screen.getByRole("button", { name: "common.save" })).toBeDisabled();
  });
});
