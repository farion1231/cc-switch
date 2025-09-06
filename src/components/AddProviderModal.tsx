import React from "react";
import { Provider } from "../types";
import { AppType } from "../lib/tauri-api";
import ProviderForm from "./ProviderForm";

interface AddProviderModalProps {
  appType: AppType;
  onAdd: (provider: Omit<Provider, "id">) => void;
  onClose: () => void;
}

const AddProviderModal: React.FC<AddProviderModalProps> = ({
  appType,
  onAdd,
  onClose,
}) => {
  return (
    <ProviderForm
      appType={appType}
      title="添加新供应商"
      submitText="添加"
      showPresets={true}
      onSubmit={onAdd}
      onClose={onClose}
    />
  );
};

export default AddProviderModal;
