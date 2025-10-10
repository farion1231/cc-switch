import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Provider } from "../types";
import { AppType } from "../lib/tauri-api";
import ProviderForm from "./ProviderForm";

interface EditProviderModalProps {
  appType: AppType;
  provider: Provider;
  onSave: (provider: Provider) => void;
  onClose: () => void;
}

const EditProviderModal: React.FC<EditProviderModalProps> = ({
  appType,
  provider,
  onSave,
  onClose,
}) => {
  const { t } = useTranslation();
  const [effectiveProvider, setEffectiveProvider] =
    useState<Provider>(provider);

  // 若为当前应用且正在编辑“当前供应商”，则优先读取 live 配置作为初始值（Claude/Codex 均适用）
  useEffect(() => {
    let mounted = true;
    const maybeLoadLive = async () => {
      try {
        const currentId = await window.api.getCurrentProvider(appType);
        if (currentId && currentId === provider.id) {
          const live = await window.api.getLiveProviderSettings(appType);
          if (!mounted) return;
          setEffectiveProvider({ ...provider, settingsConfig: live });
        } else {
          setEffectiveProvider(provider);
        }
      } catch (e) {
        // 读取失败则回退到原 provider
        setEffectiveProvider(provider);
      }
    };
    maybeLoadLive();
    return () => {
      mounted = false;
    };
  }, [appType, provider]);

  const handleSubmit = (data: Omit<Provider, "id">) => {
    onSave({
      ...provider,
      ...data,
    });
  };

  const title =
    appType === "claude"
      ? t("provider.editClaudeProvider")
      : t("provider.editCodexProvider");

  return (
    <ProviderForm
      appType={appType}
      title={title}
      submitText={t("common.save")}
      initialData={effectiveProvider}
      showPresets={false}
      onSubmit={handleSubmit}
      onClose={onClose}
    />
  );
};

export default EditProviderModal;
