import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { ProviderIcon } from "./ProviderIcon";

/** 和后端 `MAX_LIBRARY_NAME_CHARS` 一致 */
export const MAX_CUSTOM_ICON_NAME_CHARS = 64;

interface CustomIconNameDialogProps {
  /** 有值即打开 */
  icon: string | null;
  initialName: string;
  title: string;
  description?: string;
  confirmText: string;
  pending?: boolean;
  onConfirm: (name: string) => void;
  onCancel: () => void;
}

/** 给「我的图标」里的图片起名：上传后、把当前图标存进库、重命名共用 */
export function CustomIconNameDialog({
  icon,
  initialName,
  title,
  description,
  confirmText,
  pending = false,
  onConfirm,
  onCancel,
}: CustomIconNameDialogProps) {
  const { t } = useTranslation();
  const [name, setName] = useState(initialName);

  useEffect(() => {
    if (icon) setName(initialName.slice(0, MAX_CUSTOM_ICON_NAME_CHARS));
  }, [icon, initialName]);

  const trimmed = name.trim();

  return (
    <Dialog
      open={icon !== null}
      onOpenChange={(open) => {
        if (!open && !pending) onCancel();
      }}
    >
      {/* 选择器本身是 top 层的全屏对话框，这里同层、后打开的叠在上面 */}
      <DialogContent className="max-w-sm" zIndex="top">
        <form
          onSubmit={(e) => {
            e.preventDefault();
            // 选择器渲染在供应商表单里。React 事件沿组件树冒泡、会穿过 Portal，
            // 不拦住就会顺带提交外层表单，把供应商提前保存掉
            e.stopPropagation();
            if (trimmed && !pending) onConfirm(trimmed);
          }}
        >
          <DialogHeader>
            <DialogTitle>{title}</DialogTitle>
            {description && (
              <DialogDescription>{description}</DialogDescription>
            )}
          </DialogHeader>
          <div className="flex items-center gap-4 px-6 py-3">
            {icon && (
              <span className="flex h-16 w-16 shrink-0 overflow-hidden rounded-[14px] ring-1 ring-inset ring-black/10 dark:ring-white/10">
                <ProviderIcon icon={icon} name={trimmed} size={64} />
              </span>
            )}
            <div className="flex-1 space-y-2">
              <Label htmlFor="custom-icon-name">
                {t("iconPicker.nameLabel")}
              </Label>
              <Input
                id="custom-icon-name"
                autoFocus
                value={name}
                maxLength={MAX_CUSTOM_ICON_NAME_CHARS}
                placeholder={t("iconPicker.namePlaceholder")}
                onChange={(e) => setName(e.target.value)}
              />
            </div>
          </div>
          <DialogFooter>
            <Button
              type="button"
              variant="outline"
              disabled={pending}
              onClick={onCancel}
            >
              {t("common.cancel")}
            </Button>
            <Button type="submit" disabled={!trimmed || pending}>
              {confirmText}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
