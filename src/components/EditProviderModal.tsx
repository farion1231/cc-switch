import React from "react";
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

  const handleSubmit = (data: Omit<Provider, "id">) => {
    onSave({
      ...provider,
      ...data,
    });
  };

  return (
    <ProviderForm
      appType={appType}
      title={t("common.edit")}
      submitText={t("common.save")}
      initialData={provider}
      showPresets={false}
      onSubmit={handleSubmit}
      onClose={onClose}
    />
  );
};

export default EditProviderModal;
