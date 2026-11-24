import { useCallback, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useQueryClient } from "@tanstack/react-query";
import { Power } from "lucide-react";
import { settingsApi } from "@/lib/api";
import { useSettingsQuery } from "@/lib/query";
import { toast } from "@/lib/toast";
import { extractErrorMessage } from "@/utils/errorUtils";
import { AppPageHeader } from "@/components/shell/AppPageHeader";
import { NeutralBadge } from "@/components/mcp/AppMatrix";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { HoverTip } from "@/components/ui/hover-tip";
import type { Settings } from "@/types";

export type ManagedModule = "mcp" | "skills";

const SETTING_FIELD: Record<ManagedModule, "manageMcp" | "manageSkills"> = {
  mcp: "manageMcp",
  skills: "manageSkills",
};

/**
 * 读写本机 MCP / Skills 管理开关，并在开启 / 关闭时弹带撤销的轻提示以防误触。
 */
export function useModuleManagement(module: ManagedModule) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const { data: settings } = useSettingsQuery();
  const [isPending, setIsPending] = useState(false);
  const pendingRef = useRef(false);

  const field = SETTING_FIELD[module];
  const enabled = Boolean(settings?.[field]);

  const setEnabled = useCallback(
    async (next: boolean, options?: { fromUndo?: boolean }) => {
      if (pendingRef.current) return;
      pendingRef.current = true;
      setIsPending(true);
      try {
        const current = await settingsApi.get();
        const updated: Settings = { ...current, [field]: next };
        await settingsApi.save(updated);
        queryClient.setQueryData(["settings"], updated);
        void queryClient.invalidateQueries({ queryKey: ["settings"] });

        const message = next
          ? t(`moduleManagement.${module}.toastEnabled`)
          : t(`moduleManagement.${module}.toastDisabled`);
        const notify = next ? toast.success : toast.info;
        if (options?.fromUndo) {
          notify(message, { closeButton: true });
        } else {
          notify(message, {
            closeButton: true,
            duration: 5000,
            action: {
              label: t("appMatrix.undo"),
              onClick: () => {
                void setEnabled(!next, { fromUndo: true });
              },
            },
          });
        }
      } catch (error) {
        toast.error(t("settings.saveFailedGeneric"), {
          description: extractErrorMessage(error) || undefined,
        });
      } finally {
        pendingRef.current = false;
        setIsPending(false);
      }
    },
    [field, module, queryClient, t],
  );

  return { enabled, isPending, setEnabled };
}

/**
 * 未开启态居中引导页：不挂载管理列表、不触发任何磁盘扫描，一键开启。
 */
export function ModuleManagementOffPage({
  module,
  icon,
  title,
  pending,
  onEnable,
}: {
  module: ManagedModule;
  icon: ReactNode;
  title: string;
  pending: boolean;
  onEnable: () => void;
}) {
  const { t } = useTranslation();

  return (
    <>
      <AppPageHeader
        icon={<span className="text-fg-3">{icon}</span>}
        title={title}
        titleExtra={
          <NeutralBadge>{t("moduleManagement.offBadge")}</NeutralBadge>
        }
      />
      <div
        id="main-content"
        data-testid={`${module}-management-off`}
        className="flex min-h-0 flex-1 items-center justify-center bg-[radial-gradient(hsl(var(--border))_1px,transparent_1px)] px-8 pb-12 [background-size:20px_20px]"
      >
        <div className="group flex max-w-[420px] flex-col items-center text-center">
          <div
            aria-hidden="true"
            className="mb-5 flex h-14 w-14 items-center justify-center rounded-2xl border border-border bg-subtle text-fg-3 transition-all duration-150 group-has-[button:hover]:border-action/40 group-has-[button:hover]:bg-action-soft group-has-[button:hover]:text-action-text"
          >
            <span className="scale-125">{icon}</span>
          </div>
          <h2 className="m-0 mb-2 text-section text-fg-1">
            {t(`moduleManagement.${module}.offTitle`)}
          </h2>
          <p className="m-0 mb-6 text-body leading-relaxed text-fg-2">
            {t(`moduleManagement.${module}.offDescription`)}
          </p>
          <Button
            type="button"
            variant="solid"
            size="regular"
            disabled={pending}
            onClick={onEnable}
          >
            <Power className="h-4 w-4" strokeWidth={2} />
            {t(`moduleManagement.${module}.enableButton`)}
          </Button>
        </div>
      </div>
    </>
  );
}

/**
 * 已开启时挂在页头标题旁的小开关，随时一键关闭托管。
 */
export function ModuleManagementHeaderSwitch({
  module,
  disabled,
  onDisable,
}: {
  module: ManagedModule;
  disabled?: boolean;
  onDisable: () => void;
}) {
  const { t } = useTranslation();

  return (
    <div className="flex items-center">
      <span aria-hidden="true" className="mx-1.5 h-3.5 w-px bg-border" />
      <HoverTip content={t(`moduleManagement.${module}.headerSwitchTip`)}>
        <span className="inline-flex items-center">
          <Switch
            size="sm"
            checked={true}
            disabled={disabled}
            aria-label={t(`moduleManagement.${module}.headerSwitchAria`)}
            onCheckedChange={(checked) => {
              if (!checked) onDisable();
            }}
          />
        </span>
      </HoverTip>
    </div>
  );
}
