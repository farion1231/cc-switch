import { useEffect, useState } from "react";
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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import { extractErrorMessage } from "@/utils/errorUtils";
import { logFrontendInfo } from "@/lib/frontendLogger";
import { isOfficialAccount } from "@/utils/providerCapabilities";

/** 路由模式改写的客户端文件（确认框里写明，用默认位置）。 */
const CLIENT_FILE: Partial<Record<AppId, string>> = {
  claude: "~/.claude/settings.json",
  codex: "~/.codex/config.toml",
  gemini: "~/.gemini/.env",
  grokbuild: "~/.grok/config.toml",
};

export type ModeDialogState =
  | { kind: "enter"; target: Exclude<AppMode, "direct"> }
  | { kind: "needsRoute"; providerId: string; reason: string };

const MODE_LABEL: Record<Exclude<AppMode, "direct">, string> = {
  route: "路由",
  stack: "聚合",
};

/** 框里的选择写进日志：取消、仍然直连切换、进入模式在日志里分得清。 */
function logDialog(app: AppId, state: ModeDialogState, choice: string) {
  const subject =
    state.kind === "enter"
      ? `进入${MODE_LABEL[state.target]}模式确认框`
      : `「${state.providerId} 需要路由」确认框`;
  logFrontendInfo(`[MODE] ${app} ${subject}：${choice}`);
}

interface ModeDialogProps {
  app: AppId;
  state: ModeDialogState | null;
  active: AppMode;
  providers: Provider[];
  /** 能做路由目标 / 聚合默认的供应商 */
  eligibleIds: string[];
  /** 默认选中：上次的路由目标（聚合里就是默认那家），没有就用直连那家 */
  defaultPick: string | null;
  /** 聚合名单（含默认那家）及各家的模型数；框里按选中的默认那家算「另有」哪几家 */
  stackMembers: { id: string; name: string; models: number }[];
  onClose: () => void;
  onEnter: (target: Exclude<AppMode, "direct">, pick: string) => Promise<void>;
  /** 对话框 F：仍然直连切换 */
  onSwitchDirect: (providerId: string) => void;
}

/**
 * 模式切换确认框（B4.4 的 A / C / D / E，以及 F「需要路由」）。确认键 = 入口按钮去掉「…」。
 * 出错时就地显示，不关框。路由到哪家、聚合以哪家为默认都在框里选（进入模式只有这一个入口，
 * 卡片上不再有「从这家开始」的按钮）。
 */
export function ModeDialog(props: ModeDialogProps) {
  const { app, state, onClose } = props;

  useEffect(() => {
    if (state) logDialog(app, state, "弹出");
  }, [app, state]);

  const cancel = () => {
    if (state) logDialog(app, state, "取消");
    onClose();
  };

  return (
    <Dialog open={state !== null} onOpenChange={(open) => !open && cancel()}>
      {state && (
        <DialogContent
          zIndex="alert"
          className="max-w-[480px] gap-0 rounded-dialog border-border bg-surface p-6 shadow-v7-lg sm:rounded-dialog"
        >
          {state.kind === "enter" ? (
            <EnterBody {...props} state={state} onCancel={cancel} />
          ) : (
            <NeedsRouteBody {...props} state={state} onCancel={cancel} />
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
  defaultPick,
  stackMembers,
  onClose,
  onCancel,
  onEnter,
}: ModeDialogProps & {
  state: Extract<ModeDialogState, { kind: "enter" }>;
  onCancel: () => void;
}) {
  const { t } = useTranslation();
  const appName = APP_DISPLAY_NAME[app];
  const target = state.target;
  const viaDirect = active !== "direct";
  const eligible = providers.filter((p) => eligibleIds.includes(p.id));
  const initialPick =
    defaultPick && eligibleIds.includes(defaultPick)
      ? defaultPick
      : (eligible[0]?.id ?? "");
  const [pick, setPick] = useState(initialPick);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const pickedProvider = eligible.find((p) => p.id === pick);
  // 官方账号只能做聚合的默认、不能做成员（Codex）：选了别家，它的模型就进不了聚合
  const officialLeftOut =
    target === "stack" &&
    pickedProvider !== undefined &&
    !isOfficialAccount(app, pickedProvider) &&
    eligible.some((p) => isOfficialAccount(app, p));

  useEffect(() => {
    setPick(initialPick);
    // 每次打开按传进来的预选重置
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [state]);

  const otherMembers = stackMembers.filter((m) => m.id !== pick);
  const memberModels = otherMembers.reduce((sum, m) => sum + m.models, 0);
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
          otherMembers.length > 0
            ? t("mode.dialog.stackNoteMembers", {
                names: otherMembers.map((m) => m.name).join("、"),
                count: memberModels,
              })
            : t("mode.dialog.stackNoteEmpty"),
          t("mode.dialog.stackNoteNoFailover"),
          // Claude Code 运行中就会读到改过的 settings.json，不用重启。
          ...(app === "codex"
            ? [t("mode.dialog.stackNoteRestartCodex", { app: appName })]
            : []),
        ];
  if (active === "stack" && target === "route") {
    notes.push(t("mode.dialog.stackToRouteNote"));
  }

  const confirm = async () => {
    if (!pick || busy) return;
    logDialog(app, state, `确认，选 ${pick}`);
    setBusy(true);
    setError(null);
    try {
      await onEnter(target, pick);
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
  // 从路由 / 聚合互换时后端一步完成（先写回直连再接入只是实现细节），不再列步骤
  const lead =
    target === "route"
      ? t("mode.dialog.routeLead")
      : t("mode.dialog.stackLead");
  const pickLabel =
    target === "route"
      ? t("mode.dialog.routeTo")
      : t("mode.dialog.stackDefault");
  const optionLabel = (provider: Provider) =>
    isOfficialAccount(app, provider)
      ? t("mode.dialog.officialOption", { name: provider.name })
      : provider.name;

  return (
    <div className="space-y-4">
      <div className="space-y-1">
        <DialogTitle className="text-title">{title}</DialogTitle>
        <DialogDescription className="text-body text-fg-2">
          {lead}
        </DialogDescription>
      </div>

      <div className="space-y-1.5">
        <label className="text-caption font-semibold text-fg-2">
          {pickLabel}
        </label>
        <Select value={pick} onValueChange={setPick} disabled={busy}>
          <SelectTrigger className="h-9" aria-label={pickLabel}>
            <SelectValue />
          </SelectTrigger>
          <SelectContent className="z-[70]">
            {eligible.map((provider) => (
              <SelectItem key={provider.id} value={provider.id}>
                {optionLabel(provider)}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        {officialLeftOut && (
          <p
            data-testid="stack-official-note"
            className="rounded-control bg-warning-soft px-3 py-2 text-caption text-warning-text"
          >
            {t("mode.dialog.stackOfficialNote")}
          </p>
        )}
      </div>

      <ul className="space-y-1.5 rounded-panel bg-subtle px-4 py-3 text-caption text-fg-2">
        {notes.map((note) => (
          <li key={note} className="flex gap-2">
            <span aria-hidden="true">·</span>
            <span>{note}</span>
          </li>
        ))}
      </ul>

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
          onClick={onCancel}
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
  onCancel,
  onEnter,
  onSwitchDirect,
}: ModeDialogProps & {
  state: Extract<ModeDialogState, { kind: "needsRoute" }>;
  onCancel: () => void;
}) {
  const { t } = useTranslation();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const provider = providers.find((p) => p.id === state.providerId);
  const name = provider?.name ?? state.providerId;

  const route = async () => {
    logDialog(app, state, "开始路由并使用");
    setBusy(true);
    setError(null);
    try {
      await onEnter("route", state.providerId);
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
          onClick={onCancel}
        >
          {t("common.cancel")}
        </Button>
        <Button
          variant="neutral"
          size="regular"
          disabled={busy}
          onClick={() => {
            logDialog(app, state, "仍然直连切换");
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
