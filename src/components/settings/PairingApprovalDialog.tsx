import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { remoteApi } from "@/lib/api/remote";
import type {
  ApprovalEvent,
  ApprovalEvidence,
} from "@/hooks/useApprovalEventBridge";

/**
 * The three ways an approval request's origin can be classified for display.
 *
 * `notInTailnet` is kept separate from `unavailable` on purpose: the first is a
 * definite "this address is not on your tailnet", the second is "we could not
 * find out". Rendering both as one grey blob would hide that difference from the
 * person deciding whether to approve.
 */
type EvidenceClass = "tailnet" | "loopback" | "notInTailnet" | "unavailable";

/** Normalizes the wire value; anything unrecognized degrades to `unavailable`. */
export function classifySourceKind(
  raw: string | null | undefined,
): EvidenceClass {
  switch ((raw ?? "").toLowerCase()) {
    case "tailnet":
      return "tailnet";
    case "loopback":
      return "loopback";
    case "notintailnet":
      return "notInTailnet";
    default:
      // Fail closed: an unrecognized kind must never render as verified identity.
      return "unavailable";
  }
}

export interface PairingApprovalDialogProps {
  event: ApprovalEvent;
  /**
   * Optional re-probe of the origin evidence. Fire-and-forget: `whois` has a 2s
   * timeout upstream, and a hung promise must never keep the dialog from
   * rendering or disable its buttons.
   */
  probeEvidence?: () => Promise<ApprovalEvidence>;
  /**
   * 弹框已经离开屏幕时回调一次。
   *
   * `dismissed` 是「用户把它收起来了，但没做任何处置」—— 与 `rejected` 分开，
   * 是因为批准失败后关闭弹框**并没有**拒绝这一单：后端从没收到过拒绝指令，
   * 将来若有调用方按这个值记账，合并两者会记错。
   */
  onResolved?: (result: "approved" | "rejected" | "dismissed") => void;
}

function EvidenceRow({
  label,
  value,
}: {
  label: string;
  value?: string | null;
}) {
  if (!value) return null;
  return (
    <div className="flex items-start justify-between gap-4 text-sm">
      <span className="text-muted-foreground">{label}</span>
      <span className="font-mono break-all text-right">{value}</span>
    </div>
  );
}

export default function PairingApprovalDialog({
  event,
  probeEvidence,
  onResolved,
}: PairingApprovalDialogProps) {
  const { t } = useTranslation();
  const [evidence, setEvidence] = useState<ApprovalEvidence>(event.evidence);
  const [degradedAck, setDegradedAck] = useState(false);
  const [foreignAck, setForeignAck] = useState(false);
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);

  // Held in a ref so an inline arrow prop does not re-trigger the probe effect.
  const probeRef = useRef(probeEvidence);
  probeRef.current = probeEvidence;

  useEffect(() => {
    const probe = probeRef.current;
    if (!probe) return;

    let cancelled = false;
    try {
      const pending = probe();
      if (pending && typeof pending.then === "function") {
        pending.then(
          (next) => {
            if (!cancelled && next) setEvidence(next);
          },
          () => {
            // Keep whatever the event already carried; a failed probe is not fatal.
          },
        );
      }
    } catch {
      // Same: probing must never break the dialog.
    }

    return () => {
      cancelled = true;
    };
  }, [event.requestId]);

  const kind = classifySourceKind(evidence.sourceKind);
  const isForeign = evidence.isForeignAccount === true;
  const isDegraded = kind === "unavailable" || kind === "notInTailnet";
  const hasUserCode = event.userCode.trim().length > 0;

  // 建议动作默认是「拒绝」的两种情况，与后端 `pairing::decide` 一一对应：
  // 外来账号（`ConfirmForeign`）与拿不出可核实来源（`ConfirmUnavailable`，即
  // `Unavailable` / `NotInTailnet`）。两处都**不是硬拒绝** —— 拒绝按钮始终可用，
  // 批准只是要多一次显式确认。
  //
  // 界面把建议动作翻成拒绝、后端也在没收到确认时返回 unconfirmed 且不碰状态机，
  // 是同一条判据的两面：只改一边会让界面「建议批准」而后端「默认不放行」。
  const defaultReject = isForeign || isDegraded;
  const approveDisabled =
    busy ||
    !hasUserCode ||
    (isDegraded && !degradedAck) ||
    (isForeign && !foreignAck);

  const run = async (action: "approved" | "rejected") => {
    setBusy(true);
    setFailed(false);
    try {
      if (action === "approved") {
        // 如实转述用户到底勾没勾那道确认：后端据此判断「来源核不出来时」要不要
        // 放行（见 `remoteApi.approvePairing`）。已核实的来源恒为 `false`。
        // 两个标志位都如实转述用户勾没勾过：它们是后端两道**独立**的确认闸门
        // （见 `remoteApi.approvePairing`）。顶替勾选会顺带把另一道也放过去。
        await remoteApi.approvePairing(
          event.requestId,
          event.userCode,
          degradedAck,
          foreignAck,
        );
      } else {
        await remoteApi.rejectPairing(event.requestId);
      }
      onResolved?.(action);
    } catch {
      // 失败分支也必须给出口。关闭这个弹框的**唯一**路径是 `onResolved`
      // （遮罩点不掉：`ui/dialog.tsx` 全局 `onInteractOutside` 里 preventDefault；
      // 也没有 X 按钮），所以任何一次 `Err` 若只记 `failed` 就是死锁。
      if (action === "rejected") {
        // 拒绝失败 = 这一单已经不在（配对码过期后 `PairingStore::reject`
        // 命中终态返回 Err）。用户点「拒绝」的意图就是「让它消失」，
        // 结果与意图一致 —— 把它当失败留住弹框是错的。
        // 这里**不**置 `failed`：没有需要用户知道或处理的失败。
        onResolved?.("rejected");
      } else {
        // 批准失败要如实告知（配对码过期时必然如此），但提示不是留住弹框的
        // 理由 —— 关闭途径由下面那个常驻按钮兜底。
        setFailed(true);
      }
    } finally {
      setBusy(false);
    }
  };

  // 关闭只收起提示框，不作任何处置：批准 / 拒绝仍然只能由那两个按钮触发。
  // 刻意不按 `busy` 禁用 —— 命令若永远不返回（后端挂死），`busy` 会一直为真，
  // 那时禁用关闭按钮等于又造一个关不掉的模态框。
  const dismiss = () => onResolved?.("dismissed");

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label={t("remote.approval.title")}
      className="w-full max-w-lg rounded-lg border bg-background p-6 shadow-lg"
    >
      <h2 className="text-lg font-semibold">{t("remote.approval.title")}</h2>

      <div className="mt-4 space-y-2 text-sm">
        <div className="flex items-start justify-between gap-4">
          <span className="text-muted-foreground">
            {t("remote.approval.client.label")}
          </span>
          <span className="font-mono break-all text-right">
            {event.clientName}
          </span>
        </div>
        <EvidenceRow
          label={t("remote.approval.evidence.peerAddr")}
          value={evidence.peerAddr}
        />
      </div>

      <section className="mt-4 space-y-2 rounded-md border p-3">
        <p className="text-xs font-medium uppercase tracking-wide text-muted-foreground">
          {t(`remote.approval.evidence.${kind}`)}
        </p>

        {kind === "tailnet" && (
          <div className="space-y-2">
            <EvidenceRow
              label={t("remote.approval.evidence.nodeName")}
              value={evidence.nodeName}
            />
            <EvidenceRow
              label={t("remote.approval.evidence.nodeStableId")}
              value={evidence.nodeStableId}
            />
            <EvidenceRow
              label={t("remote.approval.evidence.loginName")}
              value={evidence.loginName}
            />
          </div>
        )}

        {kind === "loopback" && (
          <p className="text-sm">
            {t("remote.approval.evidence.loopbackDescription")}
          </p>
        )}

        {kind === "notInTailnet" && (
          <p className="text-sm">
            {t("remote.approval.evidence.notInTailnetDescription")}
          </p>
        )}

        {kind === "unavailable" && (
          <p className="text-sm">
            {t("remote.approval.evidence.unavailableDescription")}
          </p>
        )}

        {isDegraded && (
          <label className="mt-3 flex items-start gap-2 text-sm">
            <input
              type="checkbox"
              checked={degradedAck}
              onChange={(e) => setDegradedAck(e.target.checked)}
            />
            <span>{t("remote.approval.evidence.degradedAck")}</span>
          </label>
        )}
      </section>

      <div className="mt-4 space-y-2 text-sm">
        <div className="flex items-start justify-between gap-4">
          <span className="text-muted-foreground">
            {t("remote.approval.userCode.label")}
          </span>
          <span className="font-mono break-all text-right">
            {hasUserCode ? event.userCode : t("remote.approval.userCode.empty")}
          </span>
        </div>
      </div>

      {isForeign && (
        <section className="mt-4 space-y-2 rounded-md border border-destructive/50 p-3">
          <p className="text-sm text-destructive">
            {t("remote.approval.foreignAccount.warning")}
          </p>
          <label className="flex items-start gap-2 text-sm">
            <input
              type="checkbox"
              checked={foreignAck}
              onChange={(e) => setForeignAck(e.target.checked)}
            />
            <span>{t("remote.approval.foreignAccount.ack")}</span>
          </label>
        </section>
      )}

      <p className="mt-4 text-sm text-muted-foreground">
        {defaultReject
          ? t("remote.approval.defaultAction.reject")
          : t("remote.approval.defaultAction.approve")}
      </p>

      {failed && (
        <p role="alert" className="mt-2 text-sm text-destructive">
          {t("remote.approval.failed")}
        </p>
      )}

      <div className="mt-6 flex justify-end gap-2">
        {
          // 常驻的显式出口。不依赖 Escape：那条路径在本 bug 的复现环境里
          // 没有被验证过，而「关不掉的模态框」不允许只靠一条没验过的退路兜着。
          // 只有 `failed` 时才出现也不行 —— 命令永不返回时 `failed` 永远为假。
        }
        <Button variant="ghost" onClick={dismiss}>
          {t("remote.approval.close")}
        </Button>
        <Button
          variant="outline"
          disabled={busy}
          onClick={() => void run("rejected")}
        >
          {t("remote.approval.reject")}
        </Button>
        <Button disabled={approveDisabled} onClick={() => void run("approved")}>
          {t("remote.approval.approve")}
        </Button>
      </div>
    </div>
  );
}
