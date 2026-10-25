import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { http, HttpResponse } from "msw";
import type { Settings } from "@/types";
import type { ProxyStackSubagentModel } from "@/types/proxy";
import { StackSettingsSheet } from "@/components/providers/mode/StackSettingsSheet";
import { server } from "../msw/server";
import { getSettings, setSettings } from "../msw/state";
import { createTestQueryClient } from "../utils/testQueryClient";

const TAURI_ENDPOINT = "http://tauri.local";

const MODELS: ProxyStackSubagentModel[] = [
  {
    id: "ccs-deepseek/deepseek-v4-flash",
    providerId: "deepseek",
    displayName: "DeepSeek V4 Flash（DeepSeek）",
    efforts: ["low", "high"],
    defaultEffort: "high",
  },
  {
    id: "ccs-kimi/kimi-k3",
    providerId: "kimi",
    displayName: "Kimi K3（Kimi）",
    efforts: ["medium"],
  },
];

const MODEL = "mode.stackSettings.subagentModel";
const EFFORT = "mode.stackSettings.subagentEffort";
const CLASSIC = "settings.codexStackClassicSubagents";
const AUTO_REVIEW = "mode.stackSettings.autoReviewModel";

// 真实的 save_settings 整份反序列化：前端不带的键（undefined）就是没设。MSW 默认是合并
// 写入，这里按真实语义把两个子代理键整体替换，并记下每次保存的请求体。
let saved: Settings[] = [];

function renderSheet(
  settings: Partial<Settings> = {},
  { routeOfficial = false }: { routeOfficial?: boolean } = {},
) {
  setSettings(settings);
  saved = [];
  server.use(
    http.post(`${TAURI_ENDPOINT}/save_settings`, async ({ request }) => {
      const { settings: next } = (await request.json()) as {
        settings: Settings;
      };
      saved.push(next);
      setSettings({
        ...next,
        codexStackSubagentModel: next.codexStackSubagentModel,
        codexStackSubagentReasoningEffort:
          next.codexStackSubagentReasoningEffort,
        codexStackAutoReviewModel: next.codexStackAutoReviewModel,
      });
      return HttpResponse.json(true);
    }),
  );
  return render(
    <QueryClientProvider client={createTestQueryClient()}>
      <StackSettingsSheet
        open
        onOpenChange={vi.fn()}
        models={MODELS}
        routeOfficial={routeOfficial}
      />
    </QueryClientProvider>,
  );
}

beforeEach(() => {
  // Radix Select 打开时会滚动到选中项，jsdom 没有实现
  Element.prototype.scrollIntoView = vi.fn();
});

describe("StackSettingsSheet", () => {
  it("treats the classic tools as on when the setting was never saved", async () => {
    renderSheet();
    expect(await screen.findByRole("switch", { name: CLASSIC })).toBeChecked();
  });

  it("saves the picked sub-agent model and warns while classic tools are off", async () => {
    const user = userEvent.setup();
    renderSheet({ codexStackClassicTools: false });

    const model = await screen.findByRole("combobox", { name: MODEL });
    expect(model).toHaveTextContent("mode.stackSettings.followMain");
    // 跟随主模型时没有强度可选
    expect(screen.getByRole("combobox", { name: EFFORT })).toBeDisabled();

    await user.click(model);
    await user.click(
      await screen.findByRole("option", {
        name: "DeepSeek V4 Flash（DeepSeek）",
      }),
    );

    await waitFor(() =>
      expect(getSettings().codexStackSubagentModel).toBe(
        "ccs-deepseek/deepseek-v4-flash",
      ),
    );
    expect(getSettings().codexStackSubagentReasoningEffort).toBeUndefined();
    expect(
      await screen.findByTestId("stack-subagent-classic-off"),
    ).toBeInTheDocument();
  });

  it("offers only the picked model's efforts and drops one the next model lacks", async () => {
    const user = userEvent.setup();
    renderSheet({
      codexStackSubagentModel: "ccs-deepseek/deepseek-v4-flash",
      codexStackClassicTools: true,
    });

    const effort = await screen.findByRole("combobox", { name: EFFORT });
    await waitFor(() => expect(effort).not.toBeDisabled());
    expect(
      screen.queryByTestId("stack-subagent-classic-off"),
    ).not.toBeInTheDocument();
    await user.click(effort);
    const options = await screen.findAllByRole("option");
    expect(options.map((option) => option.textContent)).toEqual([
      "mode.stackSettings.effortModelDefaultNamed",
      "low",
      "high",
    ]);
    await user.click(screen.getByRole("option", { name: "low" }));
    await waitFor(() =>
      expect(getSettings().codexStackSubagentReasoningEffort).toBe("low"),
    );

    // Kimi 没有 low：换过去时清掉强度
    await user.click(screen.getByRole("combobox", { name: MODEL }));
    await user.click(
      await screen.findByRole("option", { name: "Kimi K3（Kimi）" }),
    );
    await waitFor(() =>
      expect(getSettings().codexStackSubagentModel).toBe("ccs-kimi/kimi-k3"),
    );
    expect(getSettings().codexStackSubagentReasoningEffort).toBeUndefined();
  });

  it("explains a saved model that is no longer aggregated", async () => {
    renderSheet({ codexStackSubagentModel: "ccs-gone/old-model" });

    expect(
      await screen.findByTestId("stack-subagent-unavailable"),
    ).toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: MODEL })).toHaveTextContent(
      "mode.stackSettings.unavailableOption",
    );
  });

  it("goes back to following the main model", async () => {
    const user = userEvent.setup();
    renderSheet({
      codexStackSubagentModel: "ccs-kimi/kimi-k3",
      codexStackSubagentReasoningEffort: "medium",
    });

    const model = await screen.findByRole("combobox", { name: MODEL });
    await waitFor(() => expect(model).toHaveTextContent("Kimi K3（Kimi）"));
    await user.click(model);
    await user.click(
      await screen.findByRole("option", {
        name: "mode.stackSettings.followMain",
      }),
    );

    await waitFor(() =>
      expect(getSettings().codexStackSubagentModel).toBeUndefined(),
    );
    expect(getSettings().codexStackSubagentReasoningEffort).toBeUndefined();
    // 两个键都不带：后端按缺省（不设）处理
    const last = saved.at(-1)!;
    expect("codexStackSubagentModel" in last).toBe(false);
    expect("codexStackSubagentReasoningEffort" in last).toBe(false);
  });

  it("moves the classic sub-agent toggle here and warns when config.toml forces v2", async () => {
    const user = userEvent.setup();
    server.use(
      http.post(`${TAURI_ENDPOINT}/codex_forces_multi_agent_v2`, () =>
        HttpResponse.json(true),
      ),
    );
    renderSheet({ codexStackClassicTools: false });

    const toggle = await screen.findByRole("switch", { name: CLASSIC });
    // 设置读回来之前按默认（开）显示
    await waitFor(() =>
      expect(toggle).toHaveAttribute("aria-checked", "false"),
    );
    // 关着时不提示
    expect(
      screen.queryByText("settings.codexStackClassicSubagentsForcedV2"),
    ).not.toBeInTheDocument();

    await user.click(toggle);
    await waitFor(() =>
      expect(getSettings().codexStackClassicTools).toBe(true),
    );
    expect(
      await screen.findByText("settings.codexStackClassicSubagentsForcedV2"),
    ).toBeInTheDocument();
  });
  it("labels the default reviewer by the default provider", async () => {
    const { unmount } = renderSheet({}, { routeOfficial: true });
    expect(
      await screen.findByRole("combobox", { name: AUTO_REVIEW }),
    ).toHaveTextContent("mode.stackSettings.autoReviewDefaultOfficial");
    unmount();

    renderSheet();
    expect(
      await screen.findByRole("combobox", { name: AUTO_REVIEW }),
    ).toHaveTextContent("mode.stackSettings.autoReviewDefaultMain");
  });

  it("saves the picked Auto Review model and goes back to the default", async () => {
    const user = userEvent.setup();
    renderSheet({ codexStackSubagentModel: "ccs-kimi/kimi-k3" });

    const reviewer = await screen.findByRole("combobox", {
      name: AUTO_REVIEW,
    });
    await user.click(reviewer);
    await user.click(
      await screen.findByRole("option", {
        name: "DeepSeek V4 Flash（DeepSeek）",
      }),
    );
    await waitFor(() =>
      expect(getSettings().codexStackAutoReviewModel).toBe(
        "ccs-deepseek/deepseek-v4-flash",
      ),
    );
    // 子代理设置原样带着
    expect(getSettings().codexStackSubagentModel).toBe("ccs-kimi/kimi-k3");

    await user.click(screen.getByRole("combobox", { name: AUTO_REVIEW }));
    await user.click(
      await screen.findByRole("option", {
        name: "mode.stackSettings.autoReviewDefaultMain",
      }),
    );
    await waitFor(() =>
      expect(getSettings().codexStackAutoReviewModel).toBeUndefined(),
    );
    expect("codexStackAutoReviewModel" in saved.at(-1)!).toBe(false);
  });

  it("explains a saved Auto Review model that is no longer aggregated", async () => {
    renderSheet({ codexStackAutoReviewModel: "ccs-gone/old-model" });

    expect(
      await screen.findByTestId("stack-auto-review-unavailable"),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("combobox", { name: AUTO_REVIEW }),
    ).toHaveTextContent("mode.stackSettings.unavailableOption");
    expect(
      screen.queryByTestId("stack-subagent-unavailable"),
    ).not.toBeInTheDocument();
  });
});
