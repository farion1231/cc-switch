import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { InFlightSection } from "@/components/usage/InFlightSection";
import type { InFlightRequest } from "@/types/proxy";
import { createTestQueryClient } from "../utils/testQueryClient";

const entriesRef = vi.hoisted(() => ({
  value: [] as InFlightRequest[],
}));

vi.mock("@/lib/query/failover", () => ({
  useInFlightRequests: () => ({ data: entriesRef.value }),
}));

function renderSection(props: Partial<Parameters<typeof InFlightSection>[0]> = {}) {
  return render(
    <QueryClientProvider client={createTestQueryClient()}>
      <InFlightSection active {...props} />
    </QueryClientProvider>,
  );
}

function entry(overrides: Partial<InFlightRequest>): InFlightRequest {
  return {
    id: 1,
    appType: "claude",
    providerId: "p1",
    providerName: "Relay A",
    model: "claude-sonnet-4-5",
    startedAt: new Date(Date.now() - 65_000).toISOString(),
    streaming: false,
    ...overrides,
  };
}

beforeEach(() => {
  entriesRef.value = [];
});

describe("InFlightSection", () => {
  it("renders nothing when no requests are in flight", () => {
    const { container } = renderSection();
    expect(container).toBeEmptyDOMElement();
  });

  it("renders nothing on pages past the first", () => {
    // 翻页说明用户在看历史；把实时行怼在脸上没有意义
    entriesRef.value = [entry({})];
    renderSection({ active: false });
    expect(document.querySelector("li")).toBeNull();
  });

  it("shows provider, model and a running elapsed time", () => {
    entriesRef.value = [entry({ startedAt: new Date(Date.now() - 65_000).toISOString() })];
    renderSection();

    expect(screen.getByText("Relay A")).toBeInTheDocument();
    expect(screen.getByText("claude-sonnet-4-5")).toBeInTheDocument();
    // 65 秒前开始的请求按分:秒显示，而不是一个静止的 0s
    expect(screen.getByText(/1m0/)).toBeInTheDocument();
  });

  it("shows a placeholder instead of an empty cell when the model is not resolved yet", () => {
    entriesRef.value = [entry({ model: null })];
    renderSection();

    expect(screen.getByText("模型解析中…")).toBeInTheDocument();
  });

  it("marks streaming requests", () => {
    entriesRef.value = [entry({ streaming: true })];
    renderSection();

    expect(screen.getByText("流式")).toBeInTheDocument();
  });

  it("honours the same filters as the log table below it", () => {
    entriesRef.value = [
      entry({ id: 1, appType: "claude", providerName: "Relay A", model: "m-a" }),
      entry({ id: 2, appType: "codex", providerName: "Relay A", model: "m-a" }),
      entry({ id: 3, appType: "claude", providerName: "Relay B", model: "m-a" }),
      entry({ id: 4, appType: "claude", providerName: "Relay A", model: "m-b" }),
    ];
    renderSection({ appType: "claude", providerName: "Relay A", model: "m-a" });

    const rows = screen.getAllByRole("listitem");
    expect(rows).toHaveLength(1);
    expect(screen.getByText("Relay A")).toBeInTheDocument();
  });

  it("shows a dash rather than a fake 0s when the start time is unparseable", () => {
    entriesRef.value = [entry({ startedAt: "not-a-timestamp" })];
    renderSection();

    expect(screen.getByText("—")).toBeInTheDocument();
  });

  it("shows how many requests are in flight", () => {
    entriesRef.value = [entry({ id: 1 }), entry({ id: 2 })];
    renderSection();

    expect(screen.getByText(/2 个请求正在进行/)).toBeInTheDocument();
  });
});
