import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { TaskGatewayProvider } from "@/tandem/api/TaskGatewayProvider";
import { createTaskGateway } from "@/tandem/api/taskGateway";
import { TaskLedgerPage } from "@/tandem/components/TaskLedgerPage";
import type { CreateTaskInput, TaskLedgerItem } from "@/tandem/types";
import { getTaskFixtures, setTaskFixtures } from "../msw/state";
import { server } from "../msw/server";

const toastError = vi.fn();

vi.mock("sonner", () => ({
  toast: { error: (...args: unknown[]) => toastError(...args) },
}));

const renderPage = () => {
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

const openCreateDialog = async () => {
  const user = userEvent.setup();
  await user.click(await screen.findByRole("button", { name: "新建任务" }));
  return user;
};

const fillValidTask = async (user: ReturnType<typeof userEvent.setup>) => {
  await user.type(screen.getByLabelText("项目名称"), "Tandem UI");
  await user.type(screen.getByLabelText("项目路径"), "/tmp/tandem-ui");
  await user.type(screen.getByLabelText("任务标题"), "Build ledger UI");
  await user.type(
    screen.getByLabelText("原始指令"),
    "Implement the approved ledger",
  );
};

const sectionNamed = (name: string) => screen.getByRole("region", { name });

describe("TaskLedgerPage", () => {
  beforeEach(() => toastError.mockReset());

  it("renders all four ordered sections and keeps concise empty states visible", async () => {
    setTaskFixtures([]);
    renderPage();

    const headings = await screen.findAllByRole("heading", { level: 2 });
    expect(headings.map((heading) => heading.textContent)).toEqual([
      "需要你处理 0",
      "待验收 0",
      "正在推进 0",
      "最近可继续 0",
    ]);
    expect(screen.getAllByText("暂无任务")).toHaveLength(4);
    expect(
      screen.queryByText(/欢迎|开始使用|强大功能/),
    ).not.toBeInTheDocument();
  });

  it("opens the creation dialog with the four required fields", async () => {
    renderPage();
    await openCreateDialog();

    expect(screen.getByLabelText("项目名称")).toBeVisible();
    expect(screen.getByLabelText("项目路径")).toBeVisible();
    expect(screen.getByLabelText("任务标题")).toBeVisible();
    expect(screen.getByLabelText("原始指令")).toBeVisible();
  });

  it("submits a camelCase command payload and inserts the result into 正在推进", async () => {
    let received: unknown;
    server.use(
      http.post(
        "http://tauri.local/create_tandem_task",
        async ({ request }) => {
          received = await request.json();
          const input = (received as { input: CreateTaskInput }).input;
          const returned: TaskLedgerItem = {
            project: {
              id: "project-created",
              name: input.projectName,
              rootPath: input.projectRootPath,
              createdAt: 2_000,
              updatedAt: 2_000,
            },
            task: {
              id: "task-created",
              projectId: "project-created",
              title: input.title,
              originalInstruction: input.originalInstruction,
              status: "active",
              createdAt: 2_000,
              updatedAt: 2_000,
              completedAt: null,
            },
          };
          return HttpResponse.json(returned);
        },
      ),
    );
    renderPage();
    const user = await openCreateDialog();
    await fillValidTask(user);
    await user.click(screen.getByRole("button", { name: "创建任务" }));

    await waitFor(() =>
      expect(received).toEqual({
        input: {
          projectName: "Tandem UI",
          projectRootPath: "/tmp/tandem-ui",
          title: "Build ledger UI",
          originalInstruction: "Implement the approved ledger",
        },
      }),
    );
    expect(
      await within(sectionNamed("正在推进")).findByText("Build ledger UI"),
    ).toBeVisible();
  });

  it("blocks blank fields, overlong titles, and structured credentials client-side", async () => {
    let createCalls = 0;
    server.use(
      http.post("http://tauri.local/create_tandem_task", () => {
        createCalls += 1;
        return HttpResponse.json({}, { status: 500 });
      }),
    );
    renderPage();
    const user = await openCreateDialog();

    await user.click(screen.getByRole("button", { name: "创建任务" }));
    expect(await screen.findByText("请填写所有字段")).toBeVisible();

    await user.type(screen.getByLabelText("项目名称"), "Project");
    await user.type(screen.getByLabelText("项目路径"), "/tmp/project");
    await user.type(screen.getByLabelText("任务标题"), "x".repeat(121));
    await user.type(screen.getByLabelText("原始指令"), "Safe instruction");
    await user.click(screen.getByRole("button", { name: "创建任务" }));
    expect(
      await screen.findByText("任务标题不能超过 120 个字符"),
    ).toBeVisible();

    fireEvent.change(screen.getByLabelText("任务标题"), {
      target: { value: "Valid title" },
    });
    fireEvent.change(screen.getByLabelText("原始指令"), {
      target: { value: "api_key=1234567890123456" },
    });
    await user.click(screen.getByRole("button", { name: "创建任务" }));
    expect(await screen.findByText("请移除结构化明文凭据")).toBeVisible();
    expect(createCalls).toBe(0);
  });

  it("keeps Rust authoritative when client validation is bypassed", async () => {
    const before = getTaskFixtures();
    const rejected = "password=1234567890123456";

    await expect(
      invoke("create_tandem_task", {
        input: {
          projectName: "Bypass",
          projectRootPath: "/tmp/bypass",
          title: "Bypass client",
          originalInstruction: rejected,
        },
      }),
    ).rejects.toThrow("structured plaintext credential");
    expect(getTaskFixtures()).toEqual(before);
  });

  it("makes acceptance completion primary and exposes other completion actions in menus", async () => {
    renderPage();
    await screen.findByRole("region", { name: "待验收" });

    expect(
      within(sectionNamed("待验收")).getByRole("button", {
        name: "确认完成 Task acceptance",
      }),
    ).toBeVisible();
    expect(
      within(sectionNamed("正在推进")).queryByRole("button", {
        name: "确认完成 Task active",
      }),
    ).not.toBeInTheDocument();

    const user = userEvent.setup();
    await user.click(
      within(sectionNamed("正在推进")).getByRole("button", {
        name: "Task active 操作",
      }),
    );
    expect(
      await screen.findByRole("menuitem", { name: "确认完成" }),
    ).toBeVisible();
  });

  it("requires explicit confirmation, invokes completion, and removes the task", async () => {
    renderPage();
    const user = userEvent.setup();
    await screen.findByRole("region", { name: "待验收" });
    const rowAction = within(sectionNamed("待验收")).getByRole("button", {
      name: "确认完成 Task acceptance",
    });
    await user.click(rowAction);

    const dialog = screen.getByRole("alertdialog", { name: "确认任务完成" });
    expect(dialog).toBeVisible();
    expect(within(dialog).getByText("Task acceptance")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "确认完成" }));

    await waitFor(() =>
      expect(
        within(sectionNamed("待验收")).queryByText("Task acceptance"),
      ).not.toBeInTheDocument(),
    );
  });

  it("keeps a task visible and shows a redacted toast when completion fails", async () => {
    const secretInstruction = "password=do-not-echo-12345";
    const [acceptance] = getTaskFixtures().filter(
      ({ task }) => task.status === "awaiting_acceptance",
    );
    acceptance.task.originalInstruction = secretInstruction;
    setTaskFixtures([acceptance]);
    server.use(
      http.post("http://tauri.local/confirm_tandem_task_completed", () =>
        HttpResponse.json("backend rejected private instruction", {
          status: 500,
        }),
      ),
    );
    renderPage();
    const user = userEvent.setup();
    await user.click(
      await screen.findByRole("button", {
        name: "确认完成 Task acceptance",
      }),
    );
    await user.click(screen.getByRole("button", { name: "确认完成" }));

    await waitFor(() => expect(toastError).toHaveBeenCalled());
    expect(
      within(sectionNamed("待验收")).getByText("Task acceptance"),
    ).toBeVisible();
    expect(JSON.stringify(toastError.mock.calls)).not.toContain(
      secretInstruction,
    );
    expect(screen.queryByText(secretInstruction)).not.toBeInTheDocument();
  });
});
