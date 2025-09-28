import React from "react";
import { useTranslation } from "react-i18next";
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
  const { t } = useTranslation();

  return (
    <ProviderForm
      appType={appType}
      title={t("provider.addNewProvider")}
      submitText={t("common.add")}
      showPresets={true}
      onSubmit={onAdd}
      onClose={onClose}
    />
  );
};

export default AddProviderModal;
