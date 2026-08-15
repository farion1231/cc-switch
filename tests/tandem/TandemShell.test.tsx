import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { TaskGatewayProvider } from "@/tandem/api/TaskGatewayProvider";
import { TandemShell } from "@/tandem/components/TandemShell";
import type { TaskGateway, TaskLedger } from "@/tandem/types";

const emptyLedger: TaskLedger = {
  needsAttention: [],
  awaitingAcceptance: [],
  active: [],
  recentResumable: [],
};

const renderShell = (gateway: TaskGateway) => {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  const LegacyConfigApp = () => <div>Legacy configuration root</div>;
  return render(
    <QueryClientProvider client={client}>
      <TaskGatewayProvider gateway={gateway}>
        <TandemShell LegacyConfigApp={LegacyConfigApp} />
      </TaskGatewayProvider>
    </QueryClientProvider>,
  );
};

describe("TandemShell", () => {
  it("starts on the ledger with task navigation primary and no marketing copy", async () => {
    const gateway: TaskGateway = {
      listLedger: vi.fn().mockResolvedValue(emptyLedger),
      createTask: vi.fn(),
      confirmCompleted: vi.fn(),
    };
    renderShell(gateway);

    expect(screen.getByRole("navigation", { name: "主导航" })).toBeVisible();
    expect(screen.getByRole("button", { name: "任务" })).toHaveAttribute(
      "aria-current",
      "page",
    );
    expect(
      await screen.findByRole("heading", { name: "需要你处理 0" }),
    ).toBeVisible();
    expect(
      screen.queryByText(/欢迎|开始使用|重新定义|高效协作/),
    ).not.toBeInTheDocument();
  });

  it("renders the injected legacy app under Agent 配置", async () => {
    const gateway: TaskGateway = {
      listLedger: vi.fn().mockResolvedValue(emptyLedger),
      createTask: vi.fn(),
      confirmCompleted: vi.fn(),
    };
    renderShell(gateway);
    await userEvent.click(screen.getByRole("button", { name: "Agent 配置" }));

    expect(screen.getByText("Legacy configuration root")).toBeVisible();
  });

  it("preserves the last successful ledger cache when returning to 任务", async () => {
    const listLedger = vi.fn().mockResolvedValue(emptyLedger);
    const gateway: TaskGateway = {
      listLedger,
      createTask: vi.fn(),
      confirmCompleted: vi.fn(),
    };
    renderShell(gateway);
    await screen.findByRole("heading", { name: "需要你处理 0" });

    await userEvent.click(screen.getByRole("button", { name: "Agent 配置" }));
    await userEvent.click(screen.getByRole("button", { name: "任务" }));

    expect(
      await screen.findByRole("heading", { name: "需要你处理 0" }),
    ).toBeVisible();
    expect(listLedger).toHaveBeenCalledTimes(1);
  });
});
