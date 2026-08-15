import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { TaskGatewayProvider } from "@/tandem/api/TaskGatewayProvider";
import { createTaskGateway } from "@/tandem/api/taskGateway";
import { TaskLedgerPage } from "@/tandem/components/TaskLedgerPage";
import { foundationJourneyFixtures } from "./fixtures/foundationJourneyFixtures";
import { setTaskFixtures } from "../msw/state";

const renderProductionPage = () => {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <TaskGatewayProvider gateway={createTaskGateway()}>
        <TaskLedgerPage />
      </TaskGatewayProvider>
    </QueryClientProvider>,
  );
};
const section = (name: string) => screen.getByRole("region", { name });

describe("production Task ledger journey", () => {
  it("lists, creates, and explicitly confirms through Tauri/MSW without leaking instructions", async () => {
    setTaskFixtures(foundationJourneyFixtures());
    const user = userEvent.setup();
    const { container } = renderProductionPage();
    expect(await screen.findByText("Resolve foundation alert")).toBeVisible();
    expect(
      within(section("需要你处理")).getByText("Resolve foundation alert"),
    ).toBeVisible();
    expect(
      within(section("待验收")).getByText("Review foundation acceptance"),
    ).toBeVisible();
    expect(
      within(section("正在推进")).getByText("Continue foundation build"),
    ).toBeVisible();
    expect(
      within(section("最近可继续")).getByText("Resume foundation task"),
    ).toBeVisible();
    expect(container).not.toHaveTextContent("PRIVATE-LIST-INSTRUCTION");

    await user.click(screen.getByRole("button", { name: "新建任务" }));
    await user.type(screen.getByLabelText("项目名称"), "Tandem Journey");
    await user.type(screen.getByLabelText("项目路径"), "/tmp/tandem-journey");
    await user.type(
      screen.getByLabelText("任务标题"),
      "Create through production gateway",
    );
    await user.type(
      screen.getByLabelText("原始指令"),
      "PRIVATE-CREATED-INSTRUCTION",
    );
    await user.click(screen.getByRole("button", { name: "创建任务" }));
    expect(
      await within(section("正在推进")).findByText(
        "Create through production gateway",
      ),
    ).toBeVisible();
    expect(container).not.toHaveTextContent("PRIVATE-CREATED-INSTRUCTION");

    await user.click(
      within(section("待验收")).getByRole("button", {
        name: "确认完成 Review foundation acceptance",
      }),
    );
    const dialog = screen.getByRole("alertdialog", { name: "确认任务完成" });
    expect(
      within(dialog).getByText("Review foundation acceptance"),
    ).toBeVisible();
    expect(container).not.toHaveTextContent("PRIVATE-ACCEPTANCE-INSTRUCTION");
    await user.click(within(dialog).getByRole("button", { name: "确认完成" }));
    await waitFor(() =>
      expect(
        within(section("待验收")).queryByText("Review foundation acceptance"),
      ).not.toBeInTheDocument(),
    );
    expect(container).not.toHaveTextContent(
      /PRIVATE-(LIST|CREATED|ACCEPTANCE)-INSTRUCTION/,
    );
  });
});
