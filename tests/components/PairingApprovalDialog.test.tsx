import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import fs from "node:fs";
import path from "node:path";
import { describe, expect, it, vi } from "vitest";
import PairingApprovalDialog from "@/components/settings/PairingApprovalDialog";
import type {
  ApprovalEvent,
  ApprovalEvidence,
} from "@/hooks/useApprovalEventBridge";

const DIALOG_TSX = path.resolve(
  __dirname,
  "..",
  "..",
  "src",
  "components",
  "settings",
  "PairingApprovalDialog.tsx",
);

// Comments are stripped before the scan so that prose explaining the backend
// contract (which is written in Chinese) does not count as user-facing copy.
// Deleting those comments is not a way past this gate: the only compliant fix
// is routing visible text through i18next `t()`.
const stripComments = (source: string) =>
  source.replace(/\/\*[\s\S]*?\*\//g, "").replace(/(^|[^:])\/\/.*$/gm, "$1");

const tMock = vi.fn((key: string) => key);
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: tMock }),
}));

const invokeMock = vi.fn(async (..._args: unknown[]) => undefined);
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

const tailnetEvidence: ApprovalEvidence = {
  sourceKind: "tailnet",
  nodeName: "mac-mini",
  nodeStableId: "n1",
  loginName: "me",
  isForeignAccount: false,
  peerAddr: "100.64.0.9:55",
};

function makeEvent(overrides: Partial<ApprovalEvent> = {}): ApprovalEvent {
  return {
    requestId: "req-1",
    clientName: "laptop",
    userCode: "ABCD-EFGH",
    evidence: tailnetEvidence,
    ...overrides,
  };
}

const approveButton = () =>
  screen.getByRole("button", { name: "remote.approval.approve" });
const rejectButton = () =>
  screen.getByRole("button", { name: "remote.approval.reject" });

describe("PairingApprovalDialog", () => {
  it("renders_tailnet_evidence_fields", () => {
    render(<PairingApprovalDialog event={makeEvent()} />);

    expect(screen.getByText("mac-mini")).toBeInTheDocument();
    expect(screen.getByText("n1")).toBeInTheDocument();
    expect(screen.getByText("me")).toBeInTheDocument();
    expect(screen.getByText("100.64.0.9:55")).toBeInTheDocument();
    expect(
      screen.getByText("remote.approval.evidence.tailnet"),
    ).toBeInTheDocument();
    expect(approveButton()).toBeEnabled();
  });

  it("renders_loopback_evidence_without_identity_fields", () => {
    render(
      <PairingApprovalDialog
        event={makeEvent({
          evidence: {
            sourceKind: "loopback",
            nodeName: null,
            nodeStableId: null,
            loginName: null,
            isForeignAccount: false,
            peerAddr: "127.0.0.1:5000",
          },
        })}
      />,
    );

    expect(
      screen.getByText("remote.approval.evidence.loopbackDescription"),
    ).toBeInTheDocument();
    expect(screen.getByText("127.0.0.1:5000")).toBeInTheDocument();
    expect(
      screen.queryByText("remote.approval.evidence.nodeName"),
    ).not.toBeInTheDocument();
    expect(approveButton()).toBeEnabled();
  });

  it("requires_explicit_ack_when_source_unavailable", () => {
    render(
      <PairingApprovalDialog
        event={makeEvent({
          evidence: {
            ...tailnetEvidence,
            sourceKind: "unavailable",
          },
        })}
      />,
    );

    expect(
      screen.getByText("remote.approval.evidence.unavailableDescription"),
    ).toBeInTheDocument();
    expect(approveButton()).toBeDisabled();

    fireEvent.click(
      screen.getByLabelText("remote.approval.evidence.degradedAck"),
    );
    expect(approveButton()).toBeEnabled();
  });

  it("unavailable_source_defaults_to_reject", () => {
    render(
      <PairingApprovalDialog
        event={makeEvent({
          evidence: {
            ...tailnetEvidence,
            sourceKind: "unavailable",
            nodeName: null,
            nodeStableId: null,
            loginName: null,
          },
        })}
      />,
    );

    // "We could not find out" is not a reason to *propose* approval. The suggested
    // action must flip to reject, exactly like a foreign account does.
    expect(
      screen.getByText("remote.approval.defaultAction.reject"),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("remote.approval.defaultAction.approve"),
    ).not.toBeInTheDocument();

    // Not a hard reject: rejecting stays available, and one explicit ack unblocks approve.
    expect(rejectButton()).toBeEnabled();
    expect(approveButton()).toBeDisabled();
    fireEvent.click(
      screen.getByLabelText("remote.approval.evidence.degradedAck"),
    );
    expect(approveButton()).toBeEnabled();
  });

  it("not_in_tailnet_source_defaults_to_reject", () => {
    render(
      <PairingApprovalDialog
        event={makeEvent({
          evidence: {
            ...tailnetEvidence,
            sourceKind: "notInTailnet",
            nodeName: null,
            nodeStableId: null,
            loginName: null,
          },
        })}
      />,
    );

    expect(
      screen.getByText("remote.approval.defaultAction.reject"),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("remote.approval.defaultAction.approve"),
    ).not.toBeInTheDocument();
    expect(approveButton()).toBeDisabled();
  });

  it("sends_the_degraded_acknowledgement_to_the_backend", async () => {
    render(
      <PairingApprovalDialog
        event={makeEvent({
          evidence: { ...tailnetEvidence, sourceKind: "unavailable" },
        })}
      />,
    );

    fireEvent.click(
      screen.getByLabelText("remote.approval.evidence.degradedAck"),
    );
    fireEvent.click(approveButton());

    // The backend recomputes the evidence itself, so it cannot see our checkbox:
    // without this flag it would have to guess whether the user was told.
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("approve_pairing", {
        requestId: "req-1",
        userCode: "ABCD-EFGH",
        confirmedUnverifiedSource: true,
        confirmedForeignAccount: false,
      }),
    );
  });

  it("does_not_claim_confirmation_that_never_happened", async () => {
    // A clean tailnet peer needs no ack, so the flag must be absent/false rather
    // than optimistically true.
    render(<PairingApprovalDialog event={makeEvent()} />);

    fireEvent.click(approveButton());

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("approve_pairing", {
        requestId: "req-1",
        userCode: "ABCD-EFGH",
        confirmedUnverifiedSource: false,
        confirmedForeignAccount: false,
      }),
    );
  });

  it("foreign_account_defaults_to_reject", () => {
    render(
      <PairingApprovalDialog
        event={makeEvent({
          evidence: { ...tailnetEvidence, isForeignAccount: true },
        })}
      />,
    );

    // The suggested action flips to reject -- not "highlighted but still approve".
    expect(
      screen.getByText("remote.approval.defaultAction.reject"),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("remote.approval.defaultAction.approve"),
    ).not.toBeInTheDocument();

    // Two-step: evidence cannot vouch for itself, so approve needs an explicit ack.
    expect(
      screen.getByText("remote.approval.foreignAccount.warning"),
    ).toBeInTheDocument();
    expect(approveButton()).toBeDisabled();

    fireEvent.click(
      screen.getByLabelText("remote.approval.foreignAccount.ack"),
    );
    expect(approveButton()).toBeEnabled();
    expect(rejectButton()).toBeEnabled();
  });

  it("renders_before_whois_resolves", () => {
    const hungProbe = vi.fn(() => new Promise<ApprovalEvidence>(() => {}));

    render(
      <PairingApprovalDialog event={makeEvent()} probeEvidence={hungProbe} />,
    );

    // The probe did start and is still pending forever.
    expect(hungProbe).toHaveBeenCalled();

    // A whois that never settles must not gate the dialog.
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(screen.getByText("mac-mini")).toBeInTheDocument();
    expect(approveButton()).toBeEnabled();
    expect(rejectButton()).toBeEnabled();
  });

  it("disables_approve_when_user_code_is_empty", () => {
    render(<PairingApprovalDialog event={makeEvent({ userCode: "" })} />);

    expect(
      screen.getByText("remote.approval.userCode.empty"),
    ).toBeInTheDocument();
    expect(approveButton()).toBeDisabled();
    expect(rejectButton()).toBeEnabled();
  });

  it("submits the pairing command carrying request id and user code", async () => {
    const onResolved = vi.fn();
    render(
      <PairingApprovalDialog event={makeEvent()} onResolved={onResolved} />,
    );

    fireEvent.click(approveButton());

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("approve_pairing", {
        requestId: "req-1",
        userCode: "ABCD-EFGH",
        // A clean tailnet peer needs no ack, so the flag is false rather than
        // optimistically true.
        confirmedUnverifiedSource: false,
        confirmedForeignAccount: false,
      }),
    );
    await waitFor(() => expect(onResolved).toHaveBeenCalledWith("approved"));
  });

  it("rejects with only the request id", async () => {
    const onResolved = vi.fn();
    render(
      <PairingApprovalDialog event={makeEvent()} onResolved={onResolved} />,
    );

    fireEvent.click(rejectButton());

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("reject_pairing", {
        requestId: "req-1",
      }),
    );
    await waitFor(() => expect(onResolved).toHaveBeenCalledWith("rejected"));
  });

  it("closes the dialog when rejecting fails", async () => {
    const onResolved = vi.fn();
    // 配对码过期后这条命令会回 Err：`PairingStore::reject` 命中终态。而用户点
    // 「拒绝」的意图恰恰是「让这一单消失」—— 后端回的「它已经不在了」与这个意图
    // 一致。把它当失败留住弹框，就是把用户锁进一个关不掉的模态框里。
    //
    // 这条 mock 保证命令一定失败，所以下面那句断言只可能来自失败分支：成功分支
    // 在这条用例里物理上走不到。
    invokeMock.mockRejectedValueOnce(new Error("pairing request not found"));

    render(
      <PairingApprovalDialog event={makeEvent()} onResolved={onResolved} />,
    );

    fireEvent.click(rejectButton());

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("reject_pairing", {
        requestId: "req-1",
      }),
    );
    await waitFor(() => expect(onResolved).toHaveBeenCalledWith("rejected"));
    // 拒绝失败不该留一句「无法完成该审批请求」给用户：没有任何需要他处理的失败。
    expect(screen.queryByText("remote.approval.failed")).not.toBeInTheDocument();
  });

  it("keeps the failure notice but stays closable when approving fails", async () => {
    const onResolved = vi.fn();
    invokeMock.mockRejectedValueOnce(new Error("pairing request expired"));

    render(
      <PairingApprovalDialog event={makeEvent()} onResolved={onResolved} />,
    );

    fireEvent.click(approveButton());

    // 批准失败要如实告知（配对码过期时后端必然回 Err），但告知不是留住弹框的理由。
    await waitFor(() =>
      expect(screen.getByText("remote.approval.failed")).toBeInTheDocument(),
    );
    expect(onResolved).not.toHaveBeenCalled();

    // 遮罩点不掉（ui/dialog.tsx 全局 preventDefault）、壳里也没有 X，所以这条
    // 显式按钮是这时唯一的出口。
    fireEvent.click(
      screen.getByRole("button", { name: "remote.approval.close" }),
    );
    expect(onResolved).toHaveBeenCalledWith("dismissed");
  });

  it("offers a visible way out before anything fails", async () => {
    render(<PairingApprovalDialog event={makeEvent()} />);

    // 出口不能只在 failed 时才出现：命令永不返回（后端挂死）时 failed 恒为假、
    // busy 恒为真，那种状态下按 failed 或 busy 门控都会再造一个关不掉的框。
    const close = screen.getByRole("button", { name: "remote.approval.close" });
    expect(close).toBeEnabled();
  });
});

describe("PairingApprovalDialog i18n gate", () => {
  const source = stripComments(fs.readFileSync(DIALOG_TSX, "utf8"));

  it("has no hardcoded Chinese string literals", () => {
    expect(source.match(/\p{Script=Han}/gu) ?? []).toEqual([]);
  });

  it("routes the visible copy through i18next", () => {
    // A component with no CJK literals but also no `t()` would be rendering
    // nothing at all; anchor the gate on a key the dialog is known to use.
    expect(source).toContain('"remote.approval.approve"');
  });
});
