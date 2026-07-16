import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { CodexAuthSection, CodexConfigSection } from "./CodexConfigSections";
import { CodexCommonConfigModal } from "./CodexCommonConfigModal";

interface CodexConfigEditorProps {
  authValue: string;

  configValue: string;

  providerName?: string;

  showRemoteCompaction?: boolean;

  isProxyTakeover?: boolean;

  onAuthChange: (value: string) => void;

  onConfigChange: (value: string) => void;

  onAuthBlur?: () => void;

  useCommonConfig: boolean;

  onCommonConfigToggle: (checked: boolean) => void | Promise<void>;

  commonConfigSnippet: string;

  onCommonConfigSnippetChange: (value: string) => boolean | Promise<boolean>;

  onCommonConfigErrorClear: () => void;

  commonConfigError: string;

  authError: string;

  configError: string; // config.toml 错误提示

  onExtract?: () => void;

  isExtracting?: boolean;

  /** 「启用 Codex 记忆功能」开关：开启时把 [memories] 段的两个模型字段
   *  同步为顶层 model。状态由 TOML 段存在性反推，无需回调。 */
  memoriesEnabled?: boolean;
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
  useCommonConfig,
  onCommonConfigToggle,
  commonConfigSnippet,
  onCommonConfigSnippetChange,
  onCommonConfigErrorClear,
  commonConfigError,
  authError,
  configError,
  onExtract,
  isExtracting,
  memoriesEnabled = false,
}) => {
  const { t } = useTranslation();
  const [isCommonConfigModalOpen, setIsCommonConfigModalOpen] = useState(false);

  const handleCloseCommonConfigModal = () => {
    onCommonConfigErrorClear();
    setIsCommonConfigModalOpen(false);
  };

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
        useCommonConfig={useCommonConfig}
        onCommonConfigToggle={onCommonConfigToggle}
        onEditCommonConfig={() => setIsCommonConfigModalOpen(true)}
        commonConfigError={commonConfigError}
        configError={configError}
        isProxyTakeover={isProxyTakeover}
        memoriesEnabled={memoriesEnabled}
      />

      {/* Common Config Modal */}
      <CodexCommonConfigModal
        isOpen={isCommonConfigModalOpen}
        onClose={handleCloseCommonConfigModal}
        value={commonConfigSnippet}
        onSave={onCommonConfigSnippetChange}
        error={commonConfigError}
        onExtract={onExtract}
        isExtracting={isExtracting}
      />
    </div>
  );
};

export default CodexConfigEditor;
