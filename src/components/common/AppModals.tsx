import React from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import type { Provider, UsageScript } from "@/types";
import type { AppId } from "@/lib/api";
import { hermesApi } from "@/lib/api/hermes";
import { extractErrorMessage } from "@/utils/errorUtils";
import { AddProviderDialog } from "@/components/providers/AddProviderDialog";
import { EditProviderDialog } from "@/components/providers/EditProviderDialog";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import UsageScriptModal from "@/components/UsageScriptModal";
import { DeepLinkImportDialog } from "@/components/DeepLinkImportDialog";
import { FirstRunNoticeDialog } from "@/components/FirstRunNoticeDialog";

export interface AppModalsProps {
  activeApp: AppId;
  isAddOpen: boolean;
  setIsAddOpen: (open: boolean) => void;
  addProvider: (data: any) => Promise<void>;
  editingProvider: Provider | null;
  effectiveEditingProvider: Provider | null;
  setEditingProvider: (provider: Provider | null) => void;
  handleEditProvider: (data: { provider: Provider; originalId?: string }) => Promise<void>;
  isCurrentAppTakeoverActive: boolean;
  usageProvider: Provider | null;
  effectiveUsageProvider: Provider | null;
  setUsageProvider: (provider: Provider | null) => void;
  saveUsageScript: (provider: Provider, script: UsageScript) => Promise<void>;
  confirmAction: { provider: Provider; action: "remove" | "delete" } | null;
  setConfirmAction: (action: { provider: Provider; action: "remove" | "delete" } | null) => void;
  handleConfirmAction: () => Promise<void>;
  launchDashboardOpen: boolean;
  setLaunchDashboardOpen: (open: boolean) => void;
}

export const AppModals: React.FC<AppModalsProps> = ({
  activeApp,
  isAddOpen,
  setIsAddOpen,
  addProvider,
  editingProvider,
  effectiveEditingProvider,
  setEditingProvider,
  handleEditProvider,
  isCurrentAppTakeoverActive,
  usageProvider,
  effectiveUsageProvider,
  setUsageProvider,
  saveUsageScript,
  confirmAction,
  setConfirmAction,
  handleConfirmAction,
  launchDashboardOpen,
  setLaunchDashboardOpen,
}) => {
  const { t } = useTranslation();

  return (
    <>
      <AddProviderDialog
        open={isAddOpen}
        onOpenChange={setIsAddOpen}
        appId={activeApp}
        onSubmit={addProvider}
      />

      <EditProviderDialog
        open={Boolean(editingProvider)}
        provider={effectiveEditingProvider}
        onOpenChange={(open) => {
          if (!open) {
            setEditingProvider(null);
          }
        }}
        onSubmit={handleEditProvider}
        appId={activeApp}
        isProxyTakeover={isCurrentAppTakeoverActive}
      />

      {effectiveUsageProvider && (
        <UsageScriptModal
          key={effectiveUsageProvider.id}
          provider={effectiveUsageProvider}
          appId={activeApp}
          isOpen={Boolean(usageProvider)}
          onClose={() => setUsageProvider(null)}
          onSave={(script) => {
            if (usageProvider) {
              void saveUsageScript(usageProvider, script);
            }
          }}
        />
      )}

      <ConfirmDialog
        isOpen={Boolean(confirmAction)}
        title={
          confirmAction?.action === "remove"
            ? t("confirm.removeProvider")
            : t("confirm.deleteProvider")
        }
        message={
          confirmAction
            ? confirmAction.action === "remove"
              ? t("confirm.removeProviderMessage", {
                  name: confirmAction.provider.name,
                })
              : t("confirm.deleteProviderMessage", {
                  name: confirmAction.provider.name,
                })
            : ""
        }
        onConfirm={() => void handleConfirmAction()}
        onCancel={() => setConfirmAction(null)}
      />

      <ConfirmDialog
        isOpen={launchDashboardOpen}
        title={t("hermes.webui.launchConfirmTitle")}
        message={t("hermes.webui.launchConfirmMessage")}
        confirmText={t("hermes.webui.launchConfirmAction")}
        variant="info"
        onConfirm={() => {
          setLaunchDashboardOpen(false);
          void (async () => {
            try {
              await hermesApi.launchDashboard();
              toast.success(t("hermes.webui.launching"));
            } catch (error) {
              toast.error(t("hermes.webui.launchFailed"), {
                description: extractErrorMessage(error) || undefined,
              });
            }
          })();
        }}
        onCancel={() => setLaunchDashboardOpen(false)}
      />

      <DeepLinkImportDialog />
      <FirstRunNoticeDialog />
    </>
  );
};
