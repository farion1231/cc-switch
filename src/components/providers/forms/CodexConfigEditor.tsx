import React from "react";
import { useTranslation } from "react-i18next";
import { CodexAuthSection, CodexConfigSection } from "./CodexConfigSections";
import type { ProviderEditorInactiveField } from "@/lib/api/providers";

interface CodexConfigEditorProps {
  authValue: string;

  configValue: string;

  providerName?: string;

  showRemoteCompaction?: boolean;

  isProxyTakeover?: boolean;

  onAuthChange: (value: string) => void;

  onConfigChange: (value: string) => void;

  onAuthBlur?: () => void;

  authError: string;

  configError: string; // config.toml 错误提示

  /** 行里保存着、但不随切换生效的全局设置。 */
  inactiveFields?: ProviderEditorInactiveField[];
}

const CodexConfigEditor: React.FC<CodexConfigEditorProps> = ({
  authValue,
  configValue,
  providerName,
  showRemoteCompaction,
  isProxyTakeover = false,
  onAuthChange,
  onConfigChange,
  onAuthBlur,
  authError,
  configError,
  inactiveFields,
}) => {
  const { t } = useTranslation();

  return (
    <div className="space-y-6">
      {isProxyTakeover && (
        <div className="p-3 bg-amber-50 dark:bg-amber-900/20 border border-amber-200 dark:border-amber-700 rounded-lg">
          <p className="text-xs text-amber-600 dark:text-amber-400">
            {t("codexConfig.proxyTakeoverStorageNotice")}
          </p>
        </div>
      )}

      {/* Auth JSON Section */}
      <CodexAuthSection
        value={authValue}
        onChange={onAuthChange}
        onBlur={onAuthBlur}
        error={authError}
        isProxyTakeover={isProxyTakeover}
      />

      {/* Config TOML Section */}
      <CodexConfigSection
        value={configValue}
        onChange={onConfigChange}
        providerName={providerName}
        showRemoteCompaction={showRemoteCompaction}
        configError={configError}
        isProxyTakeover={isProxyTakeover}
        inactiveFields={inactiveFields}
      />
    </div>
  );
};

export default CodexConfigEditor;
