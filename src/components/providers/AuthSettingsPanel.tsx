import { useTranslation } from "react-i18next";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import { AuthCenterPanel } from "@/components/settings/AuthCenterPanel";
import type { ManagedAuthProvider } from "@/lib/api";

interface AuthSettingsPanelProps {
  target: ManagedAuthProvider | null;
  onClose: () => void;
}

export function AuthSettingsPanel({ target, onClose }: AuthSettingsPanelProps) {
  const { t } = useTranslation();
  const isOpen = target !== null;

  return (
    <FullScreenPanel
      isOpen={isOpen}
      title={t("settings.tabAuth", { defaultValue: "认证" })}
      onClose={onClose}
    >
      {target ? <AuthCenterPanel authScrollTarget={target} /> : null}
    </FullScreenPanel>
  );
}
