import { useState } from "react";
import { ArrowLeft } from "lucide-react";
import { useTranslation } from "react-i18next";
import { ProviderIcon } from "@/components/ProviderIcon";
import { IconPicker } from "@/components/IconPicker";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogTrigger,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { getIconMetadata } from "@/icons/extracted/metadata";

interface ProviderIdentityFieldsProps {
  name: string;
  notes?: string | null;
  websiteUrl?: string | null;
  icon?: string | null;
  iconColor?: string | null;
  onNameChange: (value: string) => void;
  onNotesChange: (value: string) => void;
  onWebsiteUrlChange: (value: string) => void;
  onIconChange: (icon: string, color: string) => void;
}

/** Controlled variant of the identity section used by the standard provider form. */
export function ProviderIdentityFields({
  name,
  notes,
  websiteUrl,
  icon,
  iconColor,
  onNameChange,
  onNotesChange,
  onWebsiteUrlChange,
  onIconChange,
}: ProviderIdentityFieldsProps) {
  const { t } = useTranslation();
  const [iconDialogOpen, setIconDialogOpen] = useState(false);
  const effectiveColor =
    iconColor || (icon ? getIconMetadata(icon)?.defaultColor : undefined);

  return (
    <>
      <div className="mb-6 flex justify-center">
        <Dialog open={iconDialogOpen} onOpenChange={setIconDialogOpen}>
          <DialogTrigger asChild>
            <button
              type="button"
              className="flex h-20 w-20 cursor-pointer items-center justify-center rounded-xl border-2 border-muted bg-muted/30 p-3 transition-colors hover:border-primary hover:bg-muted/50"
              title={
                icon
                  ? t("providerIcon.clickToChange")
                  : t("providerIcon.clickToSelect")
              }
            >
              <ProviderIcon
                icon={icon ?? undefined}
                name={name || t("provider.name")}
                color={effectiveColor}
                size={48}
              />
            </button>
          </DialogTrigger>
          <DialogContent
            variant="fullscreen"
            zIndex="top"
            overlayClassName="bg-[hsl(var(--background))] backdrop-blur-0"
            className="p-0 sm:rounded-none"
          >
            <div className="flex h-full flex-col">
              <div className="flex-shrink-0 border-b border-border-default bg-muted/40 py-4">
                <div className="flex items-center gap-4 px-6">
                  <DialogClose asChild>
                    <Button type="button" variant="outline" size="icon">
                      <ArrowLeft className="h-4 w-4" />
                    </Button>
                  </DialogClose>
                  <p className="text-lg font-semibold leading-tight">
                    {t("providerIcon.selectIcon")}
                  </p>
                </div>
              </div>
              <div className="flex-1 overflow-y-auto">
                <div className="w-full space-y-2 px-6 py-6">
                  <IconPicker
                    value={icon ?? undefined}
                    color={effectiveColor}
                    onValueChange={(nextIcon) => {
                      const metadata = getIconMetadata(nextIcon);
                      onIconChange(nextIcon, metadata?.defaultColor ?? "");
                    }}
                  />
                  <div className="flex justify-end">
                    <DialogClose asChild>
                      <Button type="button" variant="outline">
                        {t("common.done")}
                      </Button>
                    </DialogClose>
                  </div>
                </div>
              </div>
            </div>
          </DialogContent>
        </Dialog>
      </div>

      <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
        <div className="space-y-2">
          <Label htmlFor="copilot-byok-group-name">{t("provider.name")}</Label>
          <Input
            id="copilot-byok-group-name"
            value={name}
            onChange={(event) => onNameChange(event.target.value)}
            placeholder={t("provider.namePlaceholder")}
          />
        </div>
        <div className="space-y-2">
          <Label htmlFor="copilot-byok-group-notes">
            {t("provider.notes")}
          </Label>
          <Input
            id="copilot-byok-group-notes"
            value={notes ?? ""}
            onChange={(event) => onNotesChange(event.target.value)}
            placeholder={t("provider.notesPlaceholder")}
          />
        </div>
      </div>

      <div className="space-y-2">
        <Label htmlFor="copilot-byok-group-website">
          {t("provider.websiteUrl")}
        </Label>
        <Input
          id="copilot-byok-group-website"
          value={websiteUrl ?? ""}
          onChange={(event) => onWebsiteUrlChange(event.target.value)}
          placeholder={t("providerForm.websiteUrlPlaceholder")}
        />
      </div>
    </>
  );
}
