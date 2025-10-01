import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Provider } from "../types";
import { AppType } from "../lib/query";
import { useUpdateProviderMutation, useProvidersQuery } from "../lib/query";
import ProviderForm from "./ProviderForm";
import {
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { extractErrorMessage } from "../utils/errorUtils";

interface EditProviderDialogProps {
  appType: AppType;
  providerId: string;
  onOpenChange: (open: boolean) => void;
}

export function EditProviderDialog({
  appType,
  providerId,
  onOpenChange
}: EditProviderDialogProps) {
  const { t } = useTranslation();
  const updateProviderMutation = useUpdateProviderMutation(appType);
  const { data: providersData } = useProvidersQuery(appType);

  const providers: Record<string, Provider> = providersData?.providers || Object.create(null);
  const provider = providers[providerId];

  if (!provider) {
    console.error("Provider not found:", providerId);
    return null;
  }

  const handleUpdateProvider = (data: Omit<Provider, "id">) => {
    const updatedProvider: Provider = {
      ...provider,
      ...data,
    };

    updateProviderMutation.mutate(updatedProvider, {
      onSuccess: () => {
        toast.success(t("notifications.providerSaved"));
        onOpenChange(false);
      },
      onError: (error) => {
        console.error(t("console.updateProviderFailed"), error);
        const errorMessage = extractErrorMessage(error);
        const message = errorMessage
          ? t("notifications.saveFailed", { error: errorMessage })
          : t("notifications.saveFailedGeneric");
        toast.error(message);
      }
    });
  };

  return (
    <DialogContent className="max-w-3xl max-h-[90vh] overflow-hidden">
      <DialogHeader>
        <DialogTitle>{t("common.edit")}</DialogTitle>
      </DialogHeader>
      <div className="overflow-y-auto max-h-[calc(90vh-8rem)]">
        <ProviderForm
          appType={appType}
          submitText={t("common.save")}
          initialData={provider}
          showPresets={false}
          onSubmit={handleUpdateProvider}
          onClose={() => onOpenChange(false)}
        />
      </div>
    </DialogContent>
  );
}