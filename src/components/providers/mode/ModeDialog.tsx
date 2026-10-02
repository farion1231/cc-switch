import { useEffect, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Loader2 } from "lucide-react";
import type { AppId } from "@/lib/api";
import type { Provider } from "@/types";
import type { AppMode } from "@/types/proxy";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import { extractErrorMessage } from "@/utils/errorUtils";

/** 路由模式改写的客户端文件（确认框里写明，用默认位置）。 */
const CLIENT_FILE: Partial<Record<AppId, string>> = {
  claude: "~/.claude/settings.json",
  codex: "~/.codex/config.toml",
  gemini: "~/.gemini/.env",
  grokbuild: "~/.grok/config.toml",
};

export type ModeDialogState =
  | { kind: "enter"; target: Exclude<AppMode, "direct">; pick: string | null }
  | { kind: "needsRoute"; providerId: string; reason: string };

interface ModeDialogProps {
  app: AppId;
  state: ModeDialogState | null;
  active: AppMode;
  providers: Provider[];
  /** 能做路由目标 / 叠加默认的供应商 */
  eligibleIds: string[];
  directProviderName: string;
  /** 默认选中：路由用上次的路由目标，叠加用当前默认那家 */
  defaultPick: { route: string | null; stack: string | null };
  /** 叠加里除了默认那家以外的成员及其模型数 */
  stackMembers: { name: string; models: number }[];
  /** 第一次进入路由时要勾选「我了解」 */
  needsAck: boolean;
  onClose: () => void;
  onEnter: (
    target: Exclude<AppMode, "direct">,
    pick: string,
    acknowledged: boolean,
  ) => Promise<void>;
  /** 对话框 F：仍然直连切换 */
  onSwitchDirect: (providerId: string) => void;
}

/**
 * 模式切换确认框（B4.4 的 A / C / D / E，以及 F「需要路由」）。确认键 = 入口按钮去掉「…」。
 * 出错时就地显示，不关框。
 */
export function ModeDialog(props: ModeDialogProps) {
  const { state, onClose } = props;
  return (
    <Dialog open={state !== null} onOpenChange={(open) => !open && onClose()}>
      {state && (
        <DialogContent
          zIndex="alert"
          className="max-w-[480px] gap-0 rounded-dialog border-border bg-surface p-6 shadow-v7-lg sm:rounded-dialog"
        >
          {state.kind === "enter" ? (
            <EnterBody {...props} state={state} />
          ) : (
            <NeedsRouteBody {...props} state={state} />
          )}
        </DialogContent>
      )}
    </Dialog>
  );
}

function EnterBody({
  app,
  state,
  active,
  providers,
  eligibleIds,
  directProviderName,
  defaultPick,
  stackMembers,
  needsAck,
  onClose,
  onEnter,
}: ModeDialogProps & {
  state: Extract<ModeDialogState, { kind: "enter" }>;
}) {
  const { t } = useTranslation();
  const ackId = useId();
  const ackRef = useRef<HTMLInputElement>(null);
  const appName = APP_DISPLAY_NAME[app];
  const target = state.target;
  const viaDirect = active !== "direct";
  const eligible = providers.filter((p) => eligibleIds.includes(p.id));
  const initialPick =
    [state.pick, target === "route" ? defaultPick.route : defaultPick.stack]
      .filter((id): id is string => Boolean(id))
      .find((id) => eligibleIds.includes(id)) ??
    eligible[0]?.id ??
    "";
  const [pick, setPick] = useState(initialPick);
  const [ack, setAck] = useState(false);
  const [ackError, setAckError] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const showAck = target === "route" && active === "direct" && needsAck;

  useEffect(() => {
    setPick(initialPick);
    // 每次打开按传进来的预选重置
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [state]);

  const memberModels = stackMembers.reduce((sum, m) => sum + m.models, 0);
  const notes =
    target === "route"
      ? [
          t("mode.dialog.routeNoteFile", {
            file: CLIENT_FILE[app] ?? "",
          }),
          t("mode.dialog.routeNoteSwitch"),
          t("mode.dialog.routeNoteKeepRunning"),
        ]
      : [
          stackMembers.length > 0
            ? t("mode.dialog.stackNoteMembers", {
                names: stackMembers.map((m) => m.name).join("、"),
                count: memberModels,
              })
            : t("mode.dialog.stackNoteEmpty"),
          t("mode.dialog.stackNoteNoFailover"),
          app === "codex"
            ? t("mode.dialog.stackNoteRestartCodex", { app: appName })
            : t("mode.dialog.stackNoteRestart", { app: appName }),
        ];
  if (active === "stack" && target === "route") {
    notes.push(t("mode.dialog.stackToRouteNote"));
  }

  const confirm = async () => {
    if (!pick || busy) return;
    if (showAck && !ack) {
      setAckError(true);
      ackRef.current?.focus();
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await onEnter(target, pick, showAck && ack);
      onClose();
    } catch (err) {
      setError(extractErrorMessage(err) || t("common.unknown"));
    } finally {
      setBusy(false);
    }
  };

  const targetName = t(`mode.names.${target}`);
  const title = viaDirect
    ? t("mode.dialog.switchTitle", {
        app: appName,
        from: t(`mode.names.${active}`),
        to: targetName,
      })
    : target === "route"
      ? t("mode.dialog.routeTitle", { app: appName })
      : t("mode.dialog.stackTitle", { app: appName });
  const lead = viaDirect
    ? t("mode.dialog.switchLead", { app: appName, to: targetName })
    : target === "route"
      ? t("mode.dialog.routeLead")
      : t("mode.dialog.stackLead");

  return (
    <div className="space-y-4">
      <div className="space-y-1">
        <DialogTitle className="text-title">{title}</DialogTitle>
        <DialogDescription className="text-body text-fg-2">
          {lead}
        </DialogDescription>
      </div>

      {viaDirect && (
        <ol className="space-y-2 text-body text-fg-1">
          {[
            t("mode.dialog.stepDirect", { provider: directProviderName }),
            target === "route"
              ? t("mode.dialog.stepRoute")
              : t("mode.dialog.stepStack"),
          ].map((text, index) => (
            <li key={text} className="flex items-center gap-2.5">
              <span className="flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-subtle text-badge text-fg-2">
                {index + 1}
              </span>
              {text}
            </li>
          ))}
        </ol>
      )}

      <div className="space-y-1.5">
        <label className="text-caption font-semibold text-fg-2">
          {target === "route"
            ? t("mode.dialog.routeTo")
            : t("mode.dialog.stackDefault")}
        </label>
        <Select value={pick} onValueChange={setPick} disabled={busy}>
          <SelectTrigger
            className="h-9"
            aria-label={
              target === "route"
                ? t("mode.dialog.routeTo")
                : t("mode.dialog.stackDefault")
            }
          >
            <SelectValue />
          </SelectTrigger>
          <SelectContent className="z-[70]">
            {eligible.map((provider) => (
              <SelectItem key={provider.id} value={provider.id}>
                {provider.category === "official"
                  ? t("mode.dialog.officialOption", { name: provider.name })
                  : provider.name}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      <ul className="space-y-1.5 rounded-panel bg-subtle px-4 py-3 text-caption text-fg-2">
        {notes.map((note) => (
          <li key={note} className="flex gap-2">
            <span aria-hidden="true">·</span>
            <span>{note}</span>
          </li>
        ))}
      </ul>

      {showAck && (
        <div className="space-y-1">
          <label
            htmlFor={ackId}
            className="flex items-center gap-2 text-body text-fg-1"
          >
            <Checkbox
              id={ackId}
              ref={ackRef}
              checked={ack}
              aria-invalid={ackError}
              onCheckedChange={(checked) => {
                setAck(checked);
                if (checked) setAckError(false);
              }}
            />
            {t("mode.dialog.ack")}
          </label>
          {ackError && (
            <p className="ps-6 text-caption text-danger-text">
              {t("mode.dialog.ackRequired")}
            </p>
          )}
        </div>
      )}

      {error && (
        <p
          role="alert"
          className="rounded-control bg-danger-soft px-3 py-2 text-caption text-danger-text"
        >
          {error}
        </p>
      )}

      <div className="flex flex-wrap justify-end gap-2 pt-1">
        <Button
          variant="neutral"
          size="regular"
          autoFocus
          disabled={busy}
          onClick={onClose}
        >
          {t("common.cancel")}
        </Button>
        <Button
          variant={target}
          size="regular"
          disabled={busy || !pick}
          onClick={() => void confirm()}
        >
          {busy && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
          {target === "route"
            ? t("mode.dialog.confirmRoute")
            : t("mode.dialog.confirmStack")}
        </Button>
      </div>
    </div>
  );
}

function NeedsRouteBody({
  app,
  state,
  providers,
  onClose,
  onEnter,
  onSwitchDirect,
}: ModeDialogProps & {
  state: Extract<ModeDialogState, { kind: "needsRoute" }>;
}) {
  const { t } = useTranslation();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const provider = providers.find((p) => p.id === state.providerId);
  const name = provider?.name ?? state.providerId;

  const route = async () => {
    setBusy(true);
    setError(null);
    try {
      await onEnter("route", state.providerId, true);
      onClose();
    } catch (err) {
      setError(extractErrorMessage(err) || t("common.unknown"));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="space-y-4">
      <div className="space-y-1">
        <DialogTitle className="text-title">
          {t("mode.dialog.needsRouteTitle", { provider: name })}
        </DialogTitle>
        <DialogDescription className="text-body text-fg-2">
          {t("mode.dialog.needsRouteLead", {
            provider: name,
            reason: state.reason,
            app: APP_DISPLAY_NAME[app],
          })}
        </DialogDescription>
      </div>
      {error && (
        <p
          role="alert"
          className="rounded-control bg-danger-soft px-3 py-2 text-caption text-danger-text"
        >
          {error}
        </p>
      )}
      <div className="flex flex-wrap justify-end gap-2 pt-1">
        <Button
          variant="neutral"
          size="regular"
          autoFocus
          disabled={busy}
          onClick={onClose}
        >
          {t("common.cancel")}
        </Button>
        <Button
          variant="neutral"
          size="regular"
          disabled={busy}
          onClick={() => {
            onClose();
            onSwitchDirect(state.providerId);
          }}
        >
          {t("mode.dialog.switchAnyway")}
        </Button>
        <Button
          variant="route"
          size="regular"
          disabled={busy}
          onClick={() => void route()}
        >
          {busy && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
          {t("mode.dialog.routeAndUse")}
        </Button>
      </div>
    </div>
  );
}
