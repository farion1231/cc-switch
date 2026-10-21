import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Dialog, DialogContent, DialogTitle } from "@/components/ui/dialog";
import PairingApprovalDialog from "@/components/settings/PairingApprovalDialog";
import {
  useApprovalEventBridge,
  type ApprovalEvent,
} from "@/hooks/useApprovalEventBridge";

/**
 * 桌面端的配对审批入口。
 *
 * 后端 `POST /pairing/request` 受理后会经 `EventSink` 推一条
 * `remote-approval-required`（见 `remote::events::APPROVAL_EVENT`），证据由
 * `tailscale whois` 的 `Node.User.ID` 与 `tailscale status --json` 的
 * `Self.UserID` 比对得出。这个组件是那条事件在桌面端**唯一的落点** ——
 * 挂不挂它，决定了 Tailnet / Loopback 两条渲染分支与「外来账号默认拒绝」是
 * 活代码还是死代码。
 *
 * 挂在 `App` 这一层，而不是远程设置页：审批事件跟当前停在哪个页面无关，
 * 挂在设置页里等于「不在设置页就收不到审批」，而那时恰恰是用户最需要被叫住
 * 的时刻。
 */
export default function ApprovalPrompt() {
  const { t } = useTranslation();
  const [event, setEvent] = useState<ApprovalEvent | null>(null);

  useApprovalEventBridge((next) => setEvent(next));

  return (
    <Dialog
      open={event !== null}
      // 关闭只收起提示框，不作任何处置：批准 / 拒绝只能由对话框里的按钮触发。
      onOpenChange={(open) => {
        if (!open) setEvent(null);
      }}
    >
      <DialogContent
        zIndex="top"
        // 可见标题由 `PairingApprovalDialog` 自己渲染（同一个 i18n key），
        // 这里只给读屏软件补一个等价的名字，避免无障碍上出现两个不同标题。
        aria-describedby={undefined}
        className="max-w-lg border-0 bg-transparent p-0 shadow-none"
      >
        <DialogTitle className="sr-only">
          {t("remote.approval.title")}
        </DialogTitle>
        {event && (
          <PairingApprovalDialog
            event={event}
            onResolved={() => setEvent(null)}
          />
        )}
      </DialogContent>
    </Dialog>
  );
}
