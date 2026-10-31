import { useEffect, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { Button } from "@/components/ui/button";
import { Notice } from "@/components/ui/notice";
import { providersApi, type AppId } from "@/lib/api";
import { toast } from "@/lib/toast";

export function CredentialProtectionNotice({
  appId,
  providerId,
  onRemoved,
  onPendingChange,
}: {
  appId: AppId;
  providerId: string;
  onRemoved: () => void;
  onPendingChange: (pending: boolean) => void;
}) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [confirming, setConfirming] = useState(false);
  const [pending, setPending] = useState(false);
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  useEffect(() => {
    onPendingChange(pending);
    return () => onPendingChange(false);
  }, [pending, onPendingChange]);

  const removeProtection = async () => {
    if (pending) return;
    setPending(true);
    try {
      await providersApi.removeCredentialProtection(providerId, appId);
      // Close the matching editor before a refetch can remove this notice from it.
      if (mounted.current) onRemoved();
      await queryClient.invalidateQueries({ queryKey: ["providers", appId] });
    } catch {
      if (mounted.current) toast.error(t("credentialProtection.removeFailed"));
    } finally {
      if (mounted.current) setPending(false);
    }
  };

  return (
    <>
      <Notice
        tone="neutral"
        title={t("credentialProtection.title")}
        actions={
          <Button
            type="button"
            size="compact"
            variant="outline"
            disabled={pending}
            onClick={() => setConfirming(true)}
          >
            {t("credentialProtection.remove")}
          </Button>
        }
      >
        {t("credentialProtection.description")}
      </Notice>
      <ConfirmDialog
        isOpen={confirming}
        title={t("credentialProtection.confirmTitle")}
        message={t("credentialProtection.confirmMessage")}
        confirmText={t("credentialProtection.remove")}
        pending={pending}
        onConfirm={removeProtection}
        onCancel={() => setConfirming(false)}
      />
    </>
  );
}
