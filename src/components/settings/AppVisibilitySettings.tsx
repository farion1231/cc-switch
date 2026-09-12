import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { FolderOpen } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { ToggleRow } from "@/components/ui/toggle-row";
import { cn } from "@/lib/utils";
import { ProviderIcon } from "@/components/ProviderIcon";
import type { SettingsFormState } from "@/hooks/useSettings";
import type { VisibleApps } from "@/types";
import type { AppId } from "@/lib/api";
import { DEFAULT_VISIBLE_APPS } from "@/config/appConfig";
import {
  useOhMyPiAgentDiscoveryState,
  useDisableOhMyPiAgentAutoDiscovery,
} from "@/lib/query/ohmypi";
import { ohmypiApi, type OhMyPiAgentDiscoveryProvider } from "@/lib/api/ohmypi";
import { OhMyPiAutoDiscoveryConfirmDialog } from "./OhMyPiAutoDiscoveryConfirmDialog";

interface AppVisibilitySettingsProps {
  settings: SettingsFormState;
  onChange: (updates: Partial<SettingsFormState>) => void;
}

const APP_CONFIG: Array<{
  id: AppId;
  icon: string;
  nameKey: string;
}> = [
  { id: "claude", icon: "claude", nameKey: "apps.claudeCode" },
  { id: "claude-desktop", icon: "claude", nameKey: "apps.claudeDesktop" },
  { id: "codex", icon: "openai", nameKey: "apps.codex" },
  { id: "gemini", icon: "gemini", nameKey: "apps.gemini" },
  { id: "grokbuild", icon: "grok", nameKey: "apps.grokbuild" },
  { id: "opencode", icon: "opencode", nameKey: "apps.opencode" },
  { id: "openclaw", icon: "openclaw", nameKey: "apps.openclaw" },
  { id: "hermes", icon: "hermes", nameKey: "apps.hermes" },
  { id: "pi", icon: "pi", nameKey: "apps.pi" },
  { id: "ohmypi", icon: "ohmypi", nameKey: "apps.ohmypi" },
];

export function AppVisibilitySettings({
  settings,
  onChange,
}: AppVisibilitySettingsProps) {
  const { t } = useTranslation();
  const visibleApps: VisibleApps = settings.visibleApps ?? DEFAULT_VISIBLE_APPS;

  // ── Oh My Pi auto-discovery guard state ────────────────────────
  const discoveryState = useOhMyPiAgentDiscoveryState();
  const disableDiscovery = useDisableOhMyPiAgentAutoDiscovery();
  const [dialogOpen, setDialogOpen] = useState(false);
  const [dialogPending, setDialogPending] = useState(false);
  const [providers, setProviders] = useState<OhMyPiAgentDiscoveryProvider[]>(
    [],
  );
  const mountedRef = useRef(true);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  // Lazy-load the 12-provider display list only when the dialog opens.
  useEffect(() => {
    if (!dialogOpen) return;
    let cancelled = false;
    ohmypiApi
      .getAgentDiscoveryProviders()
      .then((list) => {
        if (!cancelled && mountedRef.current) setProviders(list);
      })
      .catch(() => {
        // MSW / test or offline: leave the list empty; the dialog still
        // shows the reason and scope text.
      });
    return () => {
      cancelled = true;
    };
  }, [dialogOpen]);

  // Count how many apps are currently visible
  const visibleCount = Object.values(visibleApps).filter(Boolean).length;

  const enableOhMyPi = () => {
    onChange({
      visibleApps: {
        ...visibleApps,
        ohmypi: true,
      },
    });
  };

  const handleOhMyPiToggleOn = async () => {
    // Refetch discovery state if stale, so we don't show a stale dialog.
    const state = await discoveryState.refetch();
    const data = state.data;
    if (!data || !data.needsConfirmation) {
      enableOhMyPi();
      return;
    }
    setDialogOpen(true);
  };

  const handleConfirmDisable = async () => {
    setDialogPending(true);
    try {
      await disableDiscovery.mutateAsync();
      if (!mountedRef.current) return;
      setDialogOpen(false);
      enableOhMyPi();
    } catch {
      if (!mountedRef.current) return;
      toast.error(t("ohmypi.autoDiscovery.disableFailed.toast"));
      setDialogOpen(false);
    } finally {
      if (mountedRef.current) setDialogPending(false);
    }
  };

  const handleToggle = (appId: AppId) => {
    const isCurrentlyVisible = visibleApps[appId];
    // Prevent disabling the last visible app
    if (isCurrentlyVisible && visibleCount <= 1) return;

    // Oh My Pi off→on: guard with auto-discovery confirmation
    if (!isCurrentlyVisible && appId === "ohmypi") {
      void handleOhMyPiToggleOn();
      return;
    }

    onChange({
      visibleApps: {
        ...visibleApps,
        [appId]: !isCurrentlyVisible,
      },
    });
  };

  return (
    <section className="space-y-2">
      <header className="space-y-1">
        <h3 className="text-sm font-medium">
          {t("settings.appVisibility.title")}
        </h3>
        <p className="text-xs text-muted-foreground">
          {t("settings.appVisibility.description")}
        </p>
      </header>
      <div className="flex flex-wrap gap-1 rounded-md border border-border-default bg-background p-1">
        {APP_CONFIG.map((app) => {
          const isVisible = visibleApps[app.id];
          // Disable button if this is the last visible app
          const isDisabled = isVisible && visibleCount <= 1;

          return (
            <AppButton
              key={app.id}
              active={isVisible}
              disabled={isDisabled}
              onClick={() => handleToggle(app.id)}
              icon={app.icon}
              name={t(app.nameKey)}
            >
              {t(app.nameKey)}
            </AppButton>
          );
        })}
      </div>
      <ToggleRow
        icon={<FolderOpen className="h-4 w-4 text-emerald-500" />}
        title={t("settings.appVisibility.showProfileSwitcher")}
        description={t("settings.appVisibility.showProfileSwitcherDescription")}
        checked={settings.showProfileSwitcher ?? true}
        onCheckedChange={(value) => onChange({ showProfileSwitcher: value })}
      />
      <OhMyPiAutoDiscoveryConfirmDialog
        isOpen={dialogOpen}
        providers={providers}
        pending={dialogPending}
        onConfirm={() => void handleConfirmDisable()}
        onCancel={() => setDialogOpen(false)}
      />
    </section>
  );
}

interface AppButtonProps {
  active: boolean;
  disabled?: boolean;
  onClick: () => void;
  icon: string;
  name: string;
  children: React.ReactNode;
}

function AppButton({
  active,
  disabled,
  onClick,
  icon,
  name,
  children,
}: AppButtonProps) {
  return (
    <Button
      type="button"
      onClick={onClick}
      disabled={disabled}
      size="sm"
      variant={active ? "default" : "ghost"}
      className={cn(
        "min-w-[90px] w-auto gap-1.5 px-3",
        active
          ? "shadow-sm"
          : "text-muted-foreground hover:text-foreground hover:bg-muted",
      )}
    >
      <ProviderIcon icon={icon} name={name} size={14} />
      {children}
    </Button>
  );
}
