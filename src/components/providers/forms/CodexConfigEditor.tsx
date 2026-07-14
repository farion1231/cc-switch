import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { CodexAuthSection, CodexConfigSection } from "./CodexConfigSections";
import { CodexCommonConfigModal } from "./CodexCommonConfigModal";
import { GrokGlobalConfigModal } from "./GrokGlobalConfigModal";
import { configApi } from "@/lib/api";

interface CodexConfigEditorProps {
  appId?: "codex" | "grok";

  showCodexFeatures?: boolean;

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
}

const CodexConfigEditor: React.FC<CodexConfigEditorProps> = ({
  appId = "codex",
  showCodexFeatures = true,
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
}) => {
  const { t } = useTranslation();
  const [isCommonConfigModalOpen, setIsCommonConfigModalOpen] = useState(false);
  const [isGrokGlobalConfigOpen, setIsGrokGlobalConfigOpen] = useState(false);
  const [isAddingGrokGlobalConfig, setIsAddingGrokGlobalConfig] =
    useState(false);

  const handleAddGrokGlobalConfig = async () => {
    setIsAddingGrokGlobalConfig(true);
    try {
      await configApi.mergeGrokProfileIntoGlobalConfig(configValue);
      toast.success(t("grokConfig.addedToGlobalConfig"));
    } catch (error) {
      toast.error(String(error));
    } finally {
      setIsAddingGrokGlobalConfig(false);
    }
  };

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
        appId={appId}
        value={authValue}
        onChange={onAuthChange}
        onBlur={onAuthBlur}
        error={authError}
        isProxyTakeover={isProxyTakeover}
      />

      {/* Config TOML Section */}
      <CodexConfigSection
        appId={appId}
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
        showCodexFeatures={showCodexFeatures}
        onEditGrokGlobalConfig={() => setIsGrokGlobalConfigOpen(true)}
        onAddGrokGlobalConfig={handleAddGrokGlobalConfig}
        isAddingGrokGlobalConfig={isAddingGrokGlobalConfig}
      />

      {/* Common Config Modal */}
      {showCodexFeatures && (
        <CodexCommonConfigModal
          isOpen={isCommonConfigModalOpen}
          onClose={handleCloseCommonConfigModal}
          value={commonConfigSnippet}
          onSave={onCommonConfigSnippetChange}
          error={commonConfigError}
          onExtract={onExtract}
          isExtracting={isExtracting}
        />
      )}
      {appId === "grok" && (
        <GrokGlobalConfigModal
          isOpen={isGrokGlobalConfigOpen}
          onClose={() => setIsGrokGlobalConfigOpen(false)}
        />
      )}
    </div>
  );
};

export default CodexConfigEditor;
