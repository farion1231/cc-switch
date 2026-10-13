import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Loader2 } from "lucide-react";
import type { Provider } from "@/types";
import type { PublicRouteConnectivityResult } from "@/types/proxy";
import { CURSOR_AUTO_MODEL_IDS } from "@/config/cursorProviderPresets";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { toast } from "@/lib/toast";
import { extractErrorMessage } from "@/utils/errorUtils";
import {
  useAppMode,
  useCursorByokStatus,
  useCursorIsRunning,
  useCursorWriteByok,
  useProxyStatusQuery,
  usePublicRouteStatus,
  useTestPublicRouteConnectivity,
} from "@/lib/query/proxy";

/** Cursor 要粘贴的路径是 Cursor 自己的菜单名（英文），不随界面语言变。 */
const CURSOR_KEY_PATH = "Cursor → Settings → Models → OpenAI API Key";

/**
 * 写进 Cursor 的可用模型名：供应商模型目录里的客户端模型名；目录为空时用新建供应商预填的那批，
 * 否则 Cursor 侧没有可选模型。
 */
function cursorClientModelIds(provider: Provider): string[] {
  const models = (
    provider.settingsConfig as
      | { modelCatalog?: { models?: unknown } }
      | undefined
  )?.modelCatalog?.models;
  const ids = Array.isArray(models)
    ? models
        .map((row) => (row as { model?: unknown } | null)?.model)
        .filter((id): id is string => typeof id === "string" && id.length > 0)
    : [];
  return ids.length > 0 ? ids : [...CURSOR_AUTO_MODEL_IDS];
}

/** 前提链的四个缺口，正好对应步骤条的四段。 */
type Step = "localRoute" | "publicRoute" | "exitCursor" | "pasteKey";
type Stage = "checking" | Step | "switching" | "writing" | "writeFailed";

/**
 * 卡 D 下方连通性检查的结果：先测本地路由，本地通了才测公网隧道，坏在哪一层就报哪一层。
 */
type ConnectivityState =
  | "idle"
  | "testing"
  | "localFailed"
  | "publicFailed"
  | "ok";

const STEPS: Step[] = ["localRoute", "publicRoute", "exitCursor", "pasteKey"];

/** 步骤条当前停在第几段：进行中的自动步骤算在它前面那个缺口上。 */
const STEP_INDEX: Record<Stage, number> = {
  checking: 0,
  localRoute: 0,
  publicRoute: 1,
  exitCursor: 2,
  switching: 3,
  writing: 3,
  writeFailed: 3,
  pasteKey: 3,
};

/** 每张卡共用的「标题 + 说明」头。 */
function CardHeading({ title, body }: { title: string; body: string }) {
  return (
    <div className="space-y-1">
      <p className="text-body font-semibold text-fg-1">{title}</p>
      <p className="text-caption text-fg-2">{body}</p>
    </div>
  );
}

export interface CursorSetupDialogProps {
  provider: Provider;
  onClose: () => void;
  /** 实际切换供应商：前提齐备时由对话框自己触发（只做一次） */
  onSwitch: (provider: Provider) => unknown;
  /**
   * 卡 A：**就地**开启 Cursor 本地接管（路由目标沿用上次，**不**把那家切成当前供应商）。
   * 不能改成「跳去路由视图让用户自己进路由模式」——进路由必须选一家做目标，选那家就等于
   * 提前启用它，跟「最后才启用供应商」冲突（详见本文件 tests 里卡 A 那条的注释）。
   */
  onEnableLocalRoute: () => void;
  /**
   * 卡 B：关框并打开**公网路由**那一栏（`settings` 的 `publicRoute` section）。
   * 别拿本地路由（`routing`）的跳转凑合——2026-10-13 用户实测过，跳错页面。
   */
  onOpenPublicRouteSettings: () => void;
}

/**
 * Cursor 非官方供应商的「点启用时按需引导」框：只呈现当前缺的那一步，已满足的不复述。
 * 前提 1、2 齐备后对话框自己先完成实际切换，再依次判「Cursor 已退出 → 写 BYOK → 粘密钥」。
 * 全程不代开路由、不代劳退出 Cursor：能跳转的只跳转，能提示的只提示。
 */
export function CursorSetupDialog({
  provider,
  onClose,
  onSwitch,
  onEnableLocalRoute,
  onOpenPublicRouteSettings,
}: CursorSetupDialogProps) {
  const { t } = useTranslation();
  const [stage, setStage] = useState<Stage>("checking");
  const [writeError, setWriteError] = useState<string | null>(null);
  const [copyError, setCopyError] = useState<string | null>(null);

  const { data: mode, isSuccess: modeReady } = useAppMode("cursor");
  const { data: proxyStatus, isSuccess: statusReady } = useProxyStatusQuery();
  const { data: tunnel, isSuccess: tunnelReady } = usePublicRouteStatus();
  const cursorRunning = useCursorIsRunning();
  const byokStatus = useCursorByokStatus();
  const writeByok = useCursorWriteByok();
  const { refetch: refetchCursorRunning } = cursorRunning;
  const { refetch: refetchByokStatus } = byokStatus;
  const { mutateAsync: writeByokAsync } = writeByok;

  const localRouteReady =
    modeReady &&
    statusReady &&
    mode?.mode !== "direct" &&
    proxyStatus?.running === true;
  const publicRouteReady =
    tunnelReady &&
    tunnel?.enabled === true &&
    tunnel.tunnelRunning === true &&
    Boolean(tunnel.publicUrl);
  const baseUrl = tunnel?.publicUrl
    ? `${tunnel.publicUrl.replace(/\/+$/, "")}/cursor/v1`
    : "";
  const modelIds = useMemo(() => cursorClientModelIds(provider), [provider]);

  // 前提 1、2：缺哪一步就停在哪一步；都齐了才往下走（缺的步骤被补上后会自动续上）
  useEffect(() => {
    if (!modeReady || !statusReady || !tunnelReady) return;
    if (!localRouteReady) {
      setStage("localRoute");
      return;
    }
    if (!publicRouteReady) {
      setStage("publicRoute");
      return;
    }
    setStage((current) =>
      current === "checking" ||
      current === "localRoute" ||
      current === "publicRoute"
        ? "exitCursor"
        : current,
    );
  }, [modeReady, statusReady, tunnelReady, localRouteReady, publicRouteReady]);

  // 最后一步：前提齐备、Cursor 已退出、BYOK 也写好了，才真正**启用**这家供应商（只做一次）。
  // 顺序是用户明确要求的（2026-10-13）：公网路由 → 退出 Cursor → 写 BYOK → **最后**才启用。
  // 原先放在前提判定之后立刻切换，用户看到的是「公网路由还没开，供应商已经启用了」。
  const switchedRef = useRef(false);
  useEffect(() => {
    if (stage !== "switching" || switchedRef.current) return;
    switchedRef.current = true;
    void (async () => {
      try {
        await onSwitch(provider);
      } catch {
        // 切换失败的提示由上层负责；卡 D 的「粘密钥」引导照常进行
      }
      setStage("pasteKey");
    })();
  }, [stage, provider, onSwitch]);

  // 前提 3：Cursor 运行中会把设置回写覆盖，只提示、绝不代劳关闭；每 3 秒看一眼
  useEffect(() => {
    if (stage !== "exitCursor") return;
    if (cursorRunning.data === false) {
      setStage("writing");
      return;
    }
    const timer = window.setInterval(() => {
      void refetchCursorRunning();
    }, 3000);
    return () => window.clearInterval(timer);
  }, [stage, cursorRunning.data, refetchCursorRunning]);

  // 前提 4：写 Base URL + 可用模型列表。密钥是加密存储的，写不进，留到卡 D 手工粘
  const writingRef = useRef(false);
  useEffect(() => {
    if (stage !== "writing" || writingRef.current) return;
    writingRef.current = true;
    void (async () => {
      try {
        const fresh = (await refetchByokStatus()).data;
        const alreadyWritten =
          fresh !== undefined &&
          fresh.baseUrl === baseUrl &&
          fresh.useOpenAIKey &&
          fresh.modelCount > 0;
        if (!alreadyWritten) {
          await writeByokAsync({ baseUrl, modelIds });
        }
        setWriteError(null);
        // 写完 BYOK 才轮到「启用供应商」
        setStage("switching");
      } catch (error) {
        setWriteError(extractErrorMessage(error) || t("common.unknown"));
        setStage("writeFailed");
      }
    })();
  }, [stage, baseUrl, modelIds, refetchByokStatus, writeByokAsync, t]);

  const retryWrite = () => {
    writingRef.current = false;
    setWriteError(null);
    setStage("writing");
  };

  // 写 BYOK 成功后（进入卡 D）：先测本地路由，本地通了才继续测公网隧道，坏在哪层就报哪层
  const testConnectivity = useTestPublicRouteConnectivity();
  const { mutateAsync: testConnectivityAsync } = testConnectivity;
  const [connectivity, setConnectivity] = useState<ConnectivityState>("idle");
  const [connectivityDetail, setConnectivityDetail] = useState<string | null>(
    null,
  );

  const runConnectivity = useCallback(async () => {
    setConnectivity("testing");
    setConnectivityDetail(null);

    // IPC 本身报错（命令失败）也归到「这一层不通」，并带上错误原因
    const probe = async (
      scope: "local" | "public",
    ): Promise<PublicRouteConnectivityResult> => {
      try {
        const result = await testConnectivityAsync(scope);
        if (result && typeof result.ok === "boolean") return result;
        return { ok: false, status: null, detail: "" };
      } catch (error) {
        return {
          ok: false,
          status: null,
          detail: extractErrorMessage(error) || "",
        };
      }
    };

    const local = await probe("local");
    if (!local.ok) {
      setConnectivity("localFailed");
      setConnectivityDetail(local.detail || null);
      return;
    }
    const publicProbe = await probe("public");
    if (!publicProbe.ok) {
      setConnectivity("publicFailed");
      setConnectivityDetail(publicProbe.detail || null);
      return;
    }
    setConnectivity("ok");
  }, [testConnectivityAsync]);

  // 刚进卡 D 时自动跑一次；「重新测试」按钮直接再调 runConnectivity
  const connectivityRef = useRef(false);
  useEffect(() => {
    if (stage !== "pasteKey" || connectivityRef.current) return;
    connectivityRef.current = true;
    void runConnectivity();
  }, [stage, runConnectivity]);

  const copyKey = async () => {
    const key = tunnel?.apiKey;
    if (!key) {
      setCopyError(t("cursorSetup.cardD.copyFailed"));
      return;
    }
    try {
      await navigator.clipboard.writeText(key);
      setCopyError(null);
      toast.success(t("cursorSetup.cardD.copied"));
    } catch {
      // 剪贴板可能被拒（非安全上下文 / 手势失效）：如实提示手动复制，不谎报成功
      setCopyError(t("cursorSetup.cardD.copyFailed"));
      toast.error(t("cursorSetup.cardD.copyFailed"));
    }
  };

  const currentIndex = STEP_INDEX[stage];

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent
        zIndex="alert"
        className="max-w-[480px] gap-0 rounded-dialog border-border bg-surface p-6 shadow-v7-lg sm:rounded-dialog"
      >
        <div className="space-y-4">
          <div className="space-y-1">
            <DialogTitle className="text-title">
              {t("cursorSetup.title", { provider: provider.name })}
            </DialogTitle>
            <DialogDescription className="text-body text-fg-2">
              {t("cursorSetup.lead")}
            </DialogDescription>
          </div>

          <ol
            data-testid="cursor-setup-steps"
            className="flex flex-wrap items-center gap-x-3 gap-y-1 text-caption"
          >
            {STEPS.map((step, index) => {
              const state =
                index < currentIndex
                  ? "done"
                  : index === currentIndex
                    ? "current"
                    : "todo";
              return (
                <li
                  key={step}
                  data-testid={`cursor-setup-step-${step}`}
                  data-state={state}
                  className={
                    state === "current"
                      ? "flex items-center gap-1 font-semibold text-fg-1"
                      : "flex items-center gap-1 text-fg-2"
                  }
                >
                  <span aria-hidden="true">
                    {state === "done" ? "✓" : state === "current" ? "●" : "○"}
                  </span>
                  <span>{t(`cursorSetup.steps.${step}`)}</span>
                </li>
              );
            })}
          </ol>

          {(stage === "checking" ||
            stage === "switching" ||
            stage === "writing") && (
            <p className="flex items-center gap-2 text-caption text-fg-2">
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
              {t(
                stage === "writing"
                  ? "cursorSetup.writing"
                  : stage === "switching"
                    ? "cursorSetup.switching"
                    : "cursorSetup.checking",
              )}
            </p>
          )}

          {stage === "localRoute" && (
            <div
              data-testid="cursor-setup-localRoute"
              className="space-y-3 rounded-panel bg-subtle px-4 py-3"
            >
              <CardHeading
                title={t("cursorSetup.cardA.title")}
                body={t("cursorSetup.cardA.body")}
              />
              <div className="flex flex-wrap justify-end gap-2">
                <Button variant="neutral" size="regular" onClick={onClose}>
                  {t("common.cancel")}
                </Button>
                <Button
                  variant="route"
                  size="regular"
                  autoFocus
                  onClick={onEnableLocalRoute}
                >
                  {t("cursorSetup.cardA.action")}
                </Button>
              </div>
            </div>
          )}

          {stage === "publicRoute" && (
            <div
              data-testid="cursor-setup-publicRoute"
              className="space-y-3 rounded-panel bg-subtle px-4 py-3"
            >
              <CardHeading
                title={t("cursorSetup.cardB.title")}
                body={t("cursorSetup.cardB.body")}
              />
              {tunnel?.tunnelError && (
                <p
                  role="alert"
                  className="rounded-control bg-danger-soft px-3 py-2 text-caption text-danger-text"
                >
                  {tunnel.tunnelError}
                </p>
              )}
              <div className="flex flex-wrap justify-end gap-2">
                <Button variant="neutral" size="regular" onClick={onClose}>
                  {t("common.cancel")}
                </Button>
                <Button
                  variant="route"
                  size="regular"
                  autoFocus
                  onClick={() => {
                    onOpenPublicRouteSettings();
                    onClose();
                  }}
                >
                  {t("cursorSetup.cardB.action")}
                </Button>
              </div>
            </div>
          )}

          {stage === "exitCursor" && (
            <div
              data-testid="cursor-setup-exitCursor"
              className="space-y-3 rounded-panel bg-subtle px-4 py-3"
            >
              <CardHeading
                title={t("cursorSetup.cardC.title")}
                body={t("cursorSetup.cardC.body")}
              />
              <p className="flex items-center gap-2 text-caption text-fg-2">
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
                {t("cursorSetup.cardC.waiting")}
              </p>
              <div className="flex flex-wrap justify-end gap-2">
                <Button variant="neutral" size="regular" onClick={onClose}>
                  {t("cursorSetup.cardC.action")}
                </Button>
              </div>
            </div>
          )}

          {stage === "writeFailed" && (
            <div
              data-testid="cursor-setup-writeFailed"
              className="space-y-3 rounded-panel bg-subtle px-4 py-3"
            >
              <CardHeading
                title={t("cursorSetup.writeFailed.title")}
                body={t("cursorSetup.writeFailed.body")}
              />
              {writeError && (
                <p
                  role="alert"
                  className="rounded-control bg-danger-soft px-3 py-2 text-caption text-danger-text"
                >
                  {writeError}
                </p>
              )}
              <div className="flex flex-wrap justify-end gap-2">
                <Button variant="neutral" size="regular" onClick={onClose}>
                  {t("common.cancel")}
                </Button>
                <Button variant="route" size="regular" onClick={retryWrite}>
                  {t("cursorSetup.writeFailed.retry")}
                </Button>
              </div>
            </div>
          )}

          {stage === "pasteKey" && (
            <div
              data-testid="cursor-setup-pasteKey"
              className="space-y-3 rounded-panel bg-subtle px-4 py-3"
            >
              <CardHeading
                title={t("cursorSetup.cardD.title")}
                body={t("cursorSetup.cardD.body")}
              />
              <div className="space-y-1">
                <p className="text-caption font-semibold text-fg-2">
                  {t("cursorSetup.cardD.pathLabel")}
                </p>
                <code className="block rounded-control bg-surface px-3 py-2 text-caption text-fg-1">
                  {CURSOR_KEY_PATH}
                </code>
              </div>
              {copyError && (
                <p
                  role="alert"
                  className="rounded-control bg-danger-soft px-3 py-2 text-caption text-danger-text"
                >
                  {copyError}
                </p>
              )}
              <div className="flex flex-wrap justify-end gap-2">
                <Button variant="neutral" size="regular" onClick={onClose}>
                  {t("cursorSetup.cardD.done")}
                </Button>
                <Button
                  variant="route"
                  size="regular"
                  autoFocus
                  onClick={() => void copyKey()}
                >
                  {t("cursorSetup.cardD.copy")}
                </Button>
              </div>

              <div
                data-testid="cursor-setup-connectivity"
                className="space-y-2 rounded-control bg-surface px-3 py-2"
              >
                <div className="flex items-center justify-between gap-2">
                  <p className="text-caption font-semibold text-fg-2">
                    {t("cursorSetup.connectivity.label")}
                  </p>
                  <Button
                    variant="neutral"
                    size="compact"
                    disabled={connectivity === "testing"}
                    onClick={() => void runConnectivity()}
                  >
                    {t("cursorSetup.connectivity.retest")}
                  </Button>
                </div>

                {connectivity === "testing" && (
                  <p
                    data-testid="cursor-setup-connectivity-testing"
                    className="flex items-center gap-2 text-caption text-fg-2"
                  >
                    <Loader2 className="h-3.5 w-3.5 animate-spin" />
                    {t("cursorSetup.connectivity.testing")}
                  </p>
                )}

                {(connectivity === "localFailed" ||
                  connectivity === "publicFailed") && (
                  <div
                    data-testid={`cursor-setup-connectivity-${connectivity}`}
                    role="alert"
                    className="rounded-control bg-danger-soft px-3 py-2 text-caption text-danger-text"
                  >
                    <p>{t(`cursorSetup.connectivity.${connectivity}`)}</p>
                    {connectivityDetail && (
                      <p data-testid="cursor-setup-connectivity-detail">
                        {t("cursorSetup.connectivity.detailLabel")}{" "}
                        {connectivityDetail}
                      </p>
                    )}
                  </div>
                )}

                {connectivity === "ok" && (
                  <div
                    data-testid="cursor-setup-connectivity-ok"
                    className="space-y-1"
                  >
                    <p className="text-caption text-success-text">
                      {t("cursorSetup.connectivity.ok")}
                    </p>
                    <p className="text-caption text-fg-2">
                      {t("cursorSetup.connectivity.keyReminder")}
                    </p>
                  </div>
                )}
              </div>
            </div>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}
