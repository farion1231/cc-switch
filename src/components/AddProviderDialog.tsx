import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Provider } from "../types";
import { AppType } from "../lib/query";
import { useAddProviderMutation } from "../lib/query";
import ProviderForm from "./ProviderForm";
import {
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
  DialogClose,
} from "@/components/ui/dialog";
import { extractErrorMessage } from "../utils/errorUtils";

interface AddProviderDialogProps {
  appType: AppType;
  onOpenChange: (open: boolean) => void;
}

export function AddProviderDialog({
  appType,
  onOpenChange
}: AddProviderDialogProps) {
  const { t } = useTranslation();
  const addProviderMutation = useAddProviderMutation(appType);

  const handleAddProvider = (provider: Omit<Provider, "id">) => {
    addProviderMutation.mutate(provider, {
      onSuccess: () => {
        toast.success(t("notifications.providerAdded"));
        onOpenChange(false);
      },
      onError: (error) => {
        console.error(t("console.addProviderFailed"), error);
        const errorMessage = extractErrorMessage(error);
        const message = errorMessage
          ? t("notifications.addFailed", { error: errorMessage })
          : t("notifications.addFailedGeneric");
        toast.error(message);
      }
    });
  };

  return (
    <DialogContent className="max-w-3xl max-h-[90vh] overflow-hidden">
      <DialogHeader>
        <DialogTitle>{t("provider.addNewProvider")}</DialogTitle>
      </DialogHeader>
      <div className="overflow-y-auto max-h-[calc(90vh-8rem)]">
        <ProviderForm
          appType={appType}
          submitText={t("common.add")}
          showPresets={true}
          onSubmit={handleAddProvider}
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
          disabled={addProviderMutation.isPending}
          className="px-4 py-2 bg-blue-500 dark:bg-blue-600 text-white rounded-lg hover:bg-blue-600 dark:hover:bg-blue-700 disabled:bg-gray-400 disabled:cursor-not-allowed transition-colors text-sm font-medium flex items-center gap-2"
        >
          {t("common.add")}
        </button>
      </DialogFooter>
    </DialogContent>
  );
}