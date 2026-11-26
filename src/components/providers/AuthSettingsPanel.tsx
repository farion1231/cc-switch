import { useEffect, useRef } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import { AuthCenterPanel } from "@/components/settings/AuthCenterPanel";
import type { AppId, AuthCenterTarget } from "@/lib/api";

const TARGET_TO_APP_ID: Record<AuthCenterTarget, AppId> = {
  claude_official: "claude",
  codex_oauth: "codex",
  xai_oauth: "grokbuild",
  agy_oauth: "agy",
  github_copilot: "copilot",
  workbuddy_account: "workbuddy",
  codebuddy_oauth: "codebuddy",
  kimi_oauth: "kimi",
  dsh_account: "dsh",
};

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
      const appId = TARGET_TO_APP_ID[prevTarget];
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
