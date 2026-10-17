import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { describe, expect, it, vi } from "vitest";
import ApprovalPrompt from "@/components/settings/ApprovalPrompt";
import type { ApprovalEvent } from "@/hooks/useApprovalEventBridge";
import { emitTauriEvent } from "../msw/tauriMocks";
import { server } from "../msw/server";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

/** 后端 `remote::events::APPROVAL_EVENT` 推过来的一条事件。 */
function emit(overrides: Partial<ApprovalEvent["evidence"]> = {}) {
  act(() => {
    emitTauriEvent("remote-approval-required", {
      requestId: "req-under-test",
      clientName: "cli-under-test",
      userCode: "ABCD23",
      evidence: {
        sourceKind: "tailnet",
        nodeName: "node-under-test",
        nodeStableId: "stable-under-test",
        loginName: "peer-under-test@example.invalid",
        isForeignAccount: false,
        peerAddr: "100.64.0.9:4242",
        ...overrides,
      },
    });
  });
}

// `ApprovalPrompt` 借 Dialog 的遮罩，而 `PairingApprovalDialog` 自带 role="dialog"
// 与可见标题；壳里还有一个只给读屏软件用的 sr-only 标题。于是标题文本出现两次
// （radix 的标准做法：内容自己有 h2 时，外壳标题只做无障碍兜底），断言用
// `findAllByText` 而不是按 role 查 —— 既避开歧义，也顺带证明内容真的渲染出来了。
const approvalTitles = () => screen.queryAllByText("remote.approval.title");

describe("ApprovalPrompt", () => {
  it("opens the approval dialog when the backend pushes an approval event", async () => {
    render(<ApprovalPrompt />);

    // 事件没来时不许凭空弹框。
    expect(approvalTitles()).toHaveLength(0);

    emit();

    await waitFor(() => expect(approvalTitles().length).toBeGreaterThan(0));
    expect(
      screen.getByRole("button", { name: "remote.approval.approve" }),
    ).toBeInTheDocument();
  });

  it("renders the Tailnet branch and the foreign-account default reject", async () => {
    render(<ApprovalPrompt />);
    emit({ isForeignAccount: true });

    await waitFor(() => expect(approvalTitles().length).toBeGreaterThan(0));

    // Tailnet 分支的身份字段
    expect(screen.getByText("remote.approval.evidence.tailnet")).toBeInTheDocument();
    expect(screen.getByText("node-under-test")).toBeInTheDocument();
    expect(screen.getByText("stable-under-test")).toBeInTheDocument();
    expect(
      screen.getByText("peer-under-test@example.invalid"),
    ).toBeInTheDocument();

    // is_foreign_account → 默认动作是拒绝
    expect(
      screen.getByText("remote.approval.defaultAction.reject"),
    ).toBeInTheDocument();
  });

  it("renders the Loopback branch when the source kind says so", async () => {
    render(<ApprovalPrompt />);
    emit({
      sourceKind: "loopback",
      nodeName: null,
      nodeStableId: null,
      loginName: null,
    });

    await waitFor(() => expect(approvalTitles().length).toBeGreaterThan(0));

    expect(
      screen.getByText("remote.approval.evidence.loopback"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("remote.approval.evidence.loopbackDescription"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("remote.approval.defaultAction.approve"),
    ).toBeInTheDocument();
  });

  it("closes the prompt when rejecting a stale request fails", async () => {
    // 用户报的就是这条链路上死住的弹框：配对码过期后后端回 Err，而关闭弹框的
    // 唯一入口 `onResolved(() => setEvent(null))` 只在成功分支被调用。这里走完整
    // 链路断言 —— 事件 → 弹框 → 拒绝失败 → 弹框真的从 DOM 里消失。
    server.use(
      http.post("http://tauri.local/reject_pairing", () =>
        HttpResponse.json(
          { error: "pairing request not found" },
          { status: 500 },
        ),
      ),
    );

    render(<ApprovalPrompt />);
    emit();
    await waitFor(() => expect(approvalTitles().length).toBeGreaterThan(0));

    fireEvent.click(
      screen.getByRole("button", { name: "remote.approval.reject" }),
    );

    await waitFor(() => expect(approvalTitles()).toHaveLength(0));
  });

  it("is mounted by the desktop entry point (App.tsx)", () => {
    const appTsx = readFileSync(
      resolve(process.cwd(), "src/App.tsx"),
      "utf-8",
    );

    // 挂在 App 这一层而不是某个设置页：审批事件跟当前停在哪个页面无关，
    // 挂在 RemoteSettings 里等于「不在远程设置页就收不到审批」。
    expect(appTsx).toMatch(/import\s+ApprovalPrompt\s+from/);
    expect(appTsx).toMatch(/<ApprovalPrompt\s*\/>/);
  });
});