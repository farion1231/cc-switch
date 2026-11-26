import { useEffect, useRef } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import { AuthCenterPanel } from "@/components/settings/AuthCenterPanel";
import type { AuthCenterTarget } from "@/lib/api";
import { appIdForAuthTarget } from "@/lib/authBinding";

interface AuthSettingsPanelProps {
  target: AuthCenterTarget | null;
  onClose: () => void;
}

export function AuthSettingsPanel({ target, onClose }: AuthSettingsPanelProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const prevTargetRef = useRef<AuthCenterTarget | null>(null);
  const isOpen = target !== null;

  useEffect(() => {
    const prevTarget = prevTargetRef.current;
    prevTargetRef.current = target;
    if (prevTarget && !target) {
      const appId = appIdForAuthTarget(prevTarget);
      if (appId) {
        void queryClient.invalidateQueries({
          queryKey: ["providers", appId],
          refetchType: "all",
        });
      }
    }
  }, [target, queryClient]);

  return (
    <FullScreenPanel
      isOpen={isOpen}
      title={t("nav.auth")}
      onClose={onClose}
      motionPreset="slide-from-right"
    >
      {target ? <AuthCenterPanel authScrollTarget={target} /> : null}
    </FullScreenPanel>
  );
}
