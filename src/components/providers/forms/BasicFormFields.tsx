import { useTranslation } from "react-i18next";
import { useState } from "react";
import type { ReactNode } from "react";
import {
  FormControl,
  FormField,
  FormItem,
  FormLabel,
  FormMessage,
} from "@/components/ui/form";
import { ImeSafeInput } from "@/components/ui/ime-safe-input";
import { Button } from "@/components/ui/button";
import { HoverTip } from "@/components/ui/hover-tip";
import { ArrowLeft } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogTrigger,
  DialogClose,
} from "@/components/ui/dialog";
import { ProviderIconBox } from "@/components/ProviderIconBox";
import { IconPicker } from "@/components/IconPicker";
import { getIconMetadata } from "@/icons/extracted/metadata";
import type { UseFormReturn } from "react-hook-form";
import type { ProviderFormData } from "@/lib/schemas/provider";

/**
 * 必填标签：红色「*」画在伪元素里（不进标签文字），输入框另带 aria-required 给读屏。
 */
export const REQUIRED_LABEL =
  "after:ms-0.5 after:text-danger-text after:content-['*']";

interface BasicFormFieldsProps {
  form: UseFormReturn<ProviderFormData>;
  /** Slot to render content between icon and name fields */
  beforeNameSlot?: ReactNode;
}

export function BasicFormFields({
  form,
  beforeNameSlot,
}: BasicFormFieldsProps) {
  const { t } = useTranslation();
  const [iconDialogOpen, setIconDialogOpen] = useState(false);

  const currentIcon = form.watch("icon");
  const currentIconColor = form.watch("iconColor");
  const providerName = form.watch("name") || "Provider";
  const effectiveIconColor =
    currentIconColor ||
    (currentIcon ? getIconMetadata(currentIcon)?.defaultColor : undefined);

  const handleIconSelect = (icon: string) => {
    const meta = getIconMetadata(icon);
    form.setValue("icon", icon);
    form.setValue("iconColor", meta?.defaultColor ?? "");
  };

  const iconButton = (
    <Dialog open={iconDialogOpen} onOpenChange={setIconDialogOpen}>
      <HoverTip
        content={
          currentIcon
            ? t("providerIcon.clickToChange", {
                defaultValue: "点击更换图标",
              })
            : t("providerIcon.clickToSelect", {
                defaultValue: "点击选择图标",
              })
        }
      >
        <DialogTrigger asChild>
          <button
            type="button"
            aria-label={t("providerIcon.change", { defaultValue: "更换图标" })}
            className="group shrink-0 rounded-[8px]"
          >
            <ProviderIconBox
              icon={currentIcon}
              name={providerName}
              color={effectiveIconColor}
              className="bg-surface transition-colors group-hover:bg-subtle"
            />
          </button>
        </DialogTrigger>
      </HoverTip>
      <DialogContent
        variant="fullscreen"
        zIndex="top"
        overlayClassName="bg-[hsl(var(--background))] backdrop-blur-0"
        className="p-0 sm:rounded-none"
      >
        <div className="flex h-full flex-col">
          <div className="flex-shrink-0 py-4 border-b border-border bg-subtle">
            <div className="px-6 flex items-center gap-4">
              <DialogClose asChild>
                <Button
                  type="button"
                  variant="outline"
                  size="icon"
                  aria-label={t("common.back")}
                >
                  <ArrowLeft className="h-4 w-4" />
                </Button>
              </DialogClose>
              <p className="text-lg font-semibold leading-tight">
                {t("providerIcon.selectIcon", {
                  defaultValue: "选择图标",
                })}
              </p>
            </div>
          </div>
          <div className="flex-1 overflow-y-auto">
            <div className="space-y-2 px-6 py-6 w-full">
              <IconPicker
                value={currentIcon}
                onValueChange={handleIconSelect}
                color={effectiveIconColor}
              />
              <div className="flex justify-end gap-2">
                <DialogClose asChild>
                  <Button type="button" variant="outline">
                    {t("common.done", { defaultValue: "完成" })}
                  </Button>
                </DialogClose>
              </div>
            </div>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );

  return (
    <>
      {/* Slot for additional fields between icon and name */}
      {beforeNameSlot}

      {/* 基础信息 - 网格布局 */}
      <div className="grid grid-cols-2 gap-4">
        <FormField
          control={form.control}
          name="name"
          render={({ field }) => (
            <FormItem>
              <FormLabel className={REQUIRED_LABEL}>
                {t("provider.name")}
              </FormLabel>
              <div className="flex items-center gap-2">
                {iconButton}
                <FormControl>
                  <ImeSafeInput
                    ref={field.ref}
                    name={field.name}
                    value={field.value ?? ""}
                    onValueChange={field.onChange}
                    onBlur={field.onBlur}
                    disabled={field.disabled}
                    placeholder={t("provider.namePlaceholder")}
                    aria-required="true"
                  />
                </FormControl>
              </div>
              <FormMessage />
            </FormItem>
          )}
        />

        <FormField
          control={form.control}
          name="notes"
          render={({ field }) => (
            <FormItem>
              <FormLabel>{t("provider.notes")}</FormLabel>
              <FormControl>
                <ImeSafeInput
                  ref={field.ref}
                  name={field.name}
                  value={field.value ?? ""}
                  onValueChange={field.onChange}
                  onBlur={field.onBlur}
                  disabled={field.disabled}
                  placeholder={t("provider.notesPlaceholder")}
                />
              </FormControl>
              <FormMessage />
            </FormItem>
          )}
        />
      </div>

      <FormField
        control={form.control}
        name="websiteUrl"
        render={({ field }) => (
          <FormItem>
            <FormLabel>{t("provider.websiteUrl")}</FormLabel>
            <FormControl>
              <ImeSafeInput
                ref={field.ref}
                name={field.name}
                value={field.value ?? ""}
                onValueChange={field.onChange}
                onBlur={field.onBlur}
                disabled={field.disabled}
                placeholder={t("providerForm.websiteUrlPlaceholder")}
              />
            </FormControl>
            <FormMessage />
          </FormItem>
        )}
      />
    </>
  );
}
