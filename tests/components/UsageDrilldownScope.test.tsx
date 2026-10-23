import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { renderWithQueryClient as render } from "../utils/testQueryClient";
import { ProviderStatsTable } from "@/components/usage/ProviderStatsTable";
import { ProjectStatsTable } from "@/components/usage/ProjectStatsTable";
import type { ModelStats, ProjectStats, ProviderStats } from "@/types/usage";

const api = vi.hoisted(() => ({
  getProviderStats: vi.fn(),
  getProjectStats: vi.fn(),
  getProjectProviderStats: vi.fn(),
  getModelStats: vi.fn(),
}));

vi.mock("@/lib/api/usage", () => ({ usageApi: api }));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { resolvedLanguage: "en", language: "en" },
  }),
}));

const inside: ProviderStats = {
  providerId: "p-1",
  appType: "codex",
  providerName: "Inside provider",
  requestCount: 10,
  totalTokens: 100,
  totalCost: "1",
  successRate: 100,
  avgLatencyMs: 0,
  freshInputTokens: 10,
  cacheCreationTokens: 0,
  cacheReadTokens: 90,
  cacheableInputTokens: 100,
  cacheHitRate: 0.9,
};
const outside: ProviderStats = {
  ...inside,
  providerId: "p-2",
  providerName: "Outside provider",
};
const chosenModel: ModelStats = {
  model: "chosen-model",
  requestCount: 10,
  totalTokens: 100,
  totalCost: "1",
  avgCostPerRequest: "0.1",
  successRate: 100,
  freshInputTokens: 10,
  cacheCreationTokens: 0,
  cacheReadTokens: 90,
  cacheableInputTokens: 100,
  cacheHitRate: 0.9,
};
const outsideModel: ModelStats = { ...chosenModel, model: "outside-model" };
const project: ProjectStats = {
  projectKey: "/workspace/demo",
  projectName: "Demo",
  projectPath: "/workspace/demo",
  requestCount: 10,
  totalTokens: 100,
  totalCost: "1",
  freshInputTokens: 10,
  cacheCreationTokens: 0,
  cacheReadTokens: 90,
  cacheableInputTokens: 100,
  cacheHitRate: 0.9,
  appCount: 1,
  providerCount: 1,
  lastActiveAt: 100,
};

describe("Usage drill-down scope", () => {
  beforeEach(() => {
    api.getProjectStats.mockReset().mockResolvedValue([project]);
    // The legacy project-only endpoint cannot express the parent's provider/model filter.
    api.getProjectProviderStats
      .mockReset()
      .mockResolvedValue([inside, outside]);
    api.getProviderStats
      .mockReset()
      .mockImplementation(
        async (_start, _end, _app, providerApp, providerId) =>
          providerApp === "codex" && providerId === "p-1"
            ? [inside]
            : [inside, outside],
      );
    api.getModelStats
      .mockReset()
      .mockImplementation(
        async (
          _start,
          _end,
          _app,
          _providerApp,
          _providerId,
          _providerName,
          model,
        ) =>
          model === "chosen-model"
            ? [chosenModel]
            : [chosenModel, outsideModel],
      );
  });

  it.each(["tokens", "cache"])(
    "keeps the selected model inside provider %s drill-down",
    async (metric) => {
      const user = userEvent.setup();
      render(
        <ProviderStatsTable
          range={{ preset: "7d" }}
          providerId="p-1"
          providerAppType="codex"
          projectKey="/workspace/demo"
          model="chosen-model"
          refreshIntervalMs={0}
        />,
      );
      await user.click(
        await screen.findByRole("button", {
          name: "usage.providers.expandModels",
        }),
      );
      await screen.findByText("chosen-model");
      if (metric === "cache")
        await user.click(
          screen.getByRole("button", { name: "usage.share.metric.cache" }),
        );
      expect(screen.queryByText("outside-model")).not.toBeInTheDocument();
    },
  );

  it.each(["tokens", "cache"])(
    "keeps the selected provider and model inside project %s drill-down",
    async (metric) => {
      const user = userEvent.setup();
      render(
        <ProjectStatsTable
          range={{ preset: "7d" }}
          providerId="p-1"
          providerAppType="codex"
          model="chosen-model"
          refreshIntervalMs={0}
        />,
      );
      await user.click(await screen.findByRole("button", { name: "Demo" }));
      await screen.findByText("chosen-model");
      if (metric === "cache")
        await user.click(
          screen.getByRole("button", { name: "usage.share.metric.cache" }),
        );
      expect(screen.queryByText("Outside provider")).not.toBeInTheDocument();
      expect(screen.queryByText("outside-model")).not.toBeInTheDocument();
    },
  );
});
