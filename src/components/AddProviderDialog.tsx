import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Provider } from "../types";
import { AppType } from "../lib/query";
import { useAddProviderMutation } from "../lib/query";
import ProviderForm from "./ProviderForm";
import { Button } from "@/components/ui/button";
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
          <Button variant="outline" type="button">
            {t("common.cancel")}
          </Button>
        </DialogClose>
        <Button
          type="submit"
          form="provider-form"
          disabled={addProviderMutation.isPending}
        >
          {t("common.add")}
        </Button>
      </DialogFooter>
    </DialogContent>
  );
}