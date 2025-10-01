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
  DialogFooter,
  DialogClose,
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
          showFooter={false}
        />
      </div>
      <DialogFooter>
        <DialogClose asChild>
          <button
            type="button"
            className="px-4 py-2 text-sm font-medium text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-gray-100 hover:bg-white dark:hover:bg-gray-700 rounded-lg transition-colors"
          >
            {t("common.cancel")}
          </button>
        </DialogClose>
        <button
          type="submit"
          form="provider-form"
          disabled={updateProviderMutation.isPending}
          className="px-4 py-2 bg-blue-500 dark:bg-blue-600 text-white rounded-lg hover:bg-blue-600 dark:hover:bg-blue-700 disabled:bg-gray-400 disabled:cursor-not-allowed transition-colors text-sm font-medium flex items-center gap-2"
        >
          {t("common.save")}
        </button>
      </DialogFooter>
    </DialogContent>
  );
}