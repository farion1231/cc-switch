import React from "react";
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
  const handleSubmit = (data: Omit<Provider, "id">) => {
    onSave({
      ...provider,
      ...data,
    });
  };

  return (
    <ProviderForm
      appType={appType}
      title="编辑供应商"
      submitText="保存"
      initialData={provider}
      showPresets={false}
      onSubmit={handleSubmit}
      onClose={onClose}
    />
  );
};

export default EditProviderModal;
