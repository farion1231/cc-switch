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
  /** 能做路由目标 / 聚合默认的供应商 */
  eligibleIds: string[];
  /** 默认选中：路由用上次的路由目标，聚合用当前默认那家 */
  defaultPick: { route: string | null; stack: string | null };
  /** 聚合里除了默认那家以外的成员及其模型数 */
  stackMembers: { name: string; models: number }[];
  onClose: () => void;
  onEnter: (target: Exclude<AppMode, "direct">, pick: string) => Promise<void>;
  /** 对话框 F：仍然直连切换 */
  onSwitchDirect: (providerId: string) => void;
}

/**
 * 模式切换确认框（B4.4 的 A / C / D / E，以及 F「需要路由」）。确认键 = 入口按钮去掉「…」。
 * 出错时就地显示，不关框。路由可以在框里换目标；聚合的默认供应商在列表里定（卡片上的
 * 「以这家为默认开始聚合…」或预览里的默认那家），框里只显示、不再选。
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
  defaultPick,
  stackMembers,
  onClose,
  onEnter,
}: ModeDialogProps & {
  state: Extract<ModeDialogState, { kind: "enter" }>;
}) {
  const { t } = useTranslation();
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
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const pickedProvider = eligible.find((p) => p.id === pick);

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
  const optionLabel = (provider: Provider) =>
    provider.category === "official"
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

      {target === "route" ? (
        <div className="space-y-1.5">
          <label className="text-caption font-semibold text-fg-2">
            {t("mode.dialog.routeTo")}
          </label>
          <Select value={pick} onValueChange={setPick} disabled={busy}>
            <SelectTrigger
              className="h-9"
              aria-label={t("mode.dialog.routeTo")}
            >
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
        </div>
      ) : (
        <div className="space-y-1">
          <p className="text-caption font-semibold text-fg-2">
            {t("mode.dialog.stackDefault")}
          </p>
          <p
            data-testid="stack-default"
            className="text-body font-medium text-fg-1"
          >
            {pickedProvider
              ? optionLabel(pickedProvider)
              : t("mode.noProvider")}
          </p>
        </div>
      )}

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
