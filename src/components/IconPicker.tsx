import React, { useEffect, useState, useMemo, useRef } from "react";
import { useTranslation } from "react-i18next";
import { BookmarkPlus, Loader2, Pencil, Plus, Trash2, X } from "lucide-react";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Button } from "@/components/ui/button";
import { HoverTip } from "@/components/ui/hover-tip";
import { ConfirmDialog } from "./ConfirmDialog";
import { CustomIconNameDialog } from "./CustomIconNameDialog";
import { ProviderIcon } from "./ProviderIcon";
import { ProviderIconBox } from "./ProviderIconBox";
import { iconList } from "@/icons/extracted";
import { searchIcons, getIconMetadata } from "@/icons/extracted/metadata";
import { cn } from "@/lib/utils";
import { toast } from "@/lib/toast";
import type { CustomIconEntry } from "@/lib/api/customIcons";
import {
  useAddCustomIconMutation,
  useCustomIconsQuery,
  useDeleteCustomIconMutation,
  useRenameCustomIconMutation,
} from "@/lib/query/customIcons";
import { extractErrorMessage } from "@/utils/errorUtils";
import {
  CUSTOM_ICON_ACCEPT,
  CustomIconError,
  isCustomIcon,
  normalizeIconFile,
} from "@/utils/customIcon";

interface IconPickerProps {
  value?: string; // 当前选中的图标
  onValueChange: (icon: string) => void; // 选择回调
  color?: string; // 预览颜色
  /** 把当前图标存进「我的图标」时默认填的名字，一般是供应商名称 */
  defaultName?: string;
}

/** 起名对话框的三种用途：刚上传的图片、把当前图标存进库、给库里的图标改名 */
type NameDialogState =
  | { mode: "upload"; data: string; initialName: string }
  | { mode: "saveCurrent"; data: string; initialName: string }
  | { mode: "rename"; data: string; initialName: string; id: string };

const GRID_CLASS = "grid grid-cols-6 sm:grid-cols-8 lg:grid-cols-10 gap-2";

const tileClass = (selected: boolean) =>
  cn(
    "flex w-full flex-col items-center gap-1 p-3 rounded-lg",
    "border-2 transition-all duration-200",
    "hover:bg-subtle hover:border-primary/50",
    selected ? "border-primary bg-primary/10" : "border-transparent",
  );

/** 文件名去掉扩展名，当作上传图片的默认名称 */
function nameFromFile(file: File): string {
  return file.name.replace(/\.[^.]+$/, "").trim();
}

export const IconPicker: React.FC<IconPickerProps> = ({
  value,
  onValueChange,
  defaultName = "",
}) => {
  const { t } = useTranslation();
  const [searchQuery, setSearchQuery] = useState("");
  const [uploading, setUploading] = useState(false);
  const [nameDialog, setNameDialog] = useState<NameDialogState | null>(null);
  const [deleting, setDeleting] = useState<CustomIconEntry | null>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  // 每次选择（上传、清除、点图标）都加一：处理中的上传完成时序号已经变了，
  // 说明用户后来又做了别的选择，旧结果作废，不能再弹出起名框
  const selectionSeqRef = useRef(0);

  const { data: library = [] } = useCustomIconsQuery();
  const addIcon = useAddCustomIconMutation();
  const renameIcon = useRenameCustomIconMutation();
  const deleteIcon = useDeleteCustomIconMutation();

  // 关掉选择器也让处理中的上传作废：重新打开是一个新实例、新序号，管不到旧上传
  useEffect(
    () => () => {
      selectionSeqRef.current += 1;
    },
    [],
  );

  const select = (icon: string) => {
    selectionSeqRef.current += 1;
    setUploading(false);
    onValueChange(icon);
  };

  // 图片先缩好，再弹出起名框；起好名存进「我的图标」才选中。
  // 选中只改表单里的值，点保存才写进供应商
  const handleFileChange = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    // 清空，同一个文件改完再选一次也能触发 change
    e.target.value = "";
    if (!file) return;
    const seq = ++selectionSeqRef.current;
    setUploading(true);
    try {
      const data = await normalizeIconFile(file);
      if (seq === selectionSeqRef.current) {
        setNameDialog({
          mode: "upload",
          data,
          initialName: nameFromFile(file),
        });
      }
    } catch (error) {
      if (seq !== selectionSeqRef.current) return;
      const code =
        error instanceof CustomIconError ? error.code : "decodeFailed";
      toast.error(t(`iconPicker.uploadError.${code}`));
    } finally {
      if (seq === selectionSeqRef.current) setUploading(false);
    }
  };

  const handleNameConfirm = async (name: string) => {
    if (!nameDialog) return;
    try {
      if (nameDialog.mode === "rename") {
        await renameIcon.mutateAsync({ id: nameDialog.id, name });
      } else {
        const result = await addIcon.mutateAsync({
          name,
          data: nameDialog.data,
        });
        if (result.existed) {
          toast.info(t("iconPicker.alreadySaved", { name: result.icon.name }));
        }
        if (nameDialog.mode === "upload") select(result.icon.data);
      }
      setNameDialog(null);
    } catch (error) {
      toast.error(extractErrorMessage(error));
    }
  };

  const handleDeleteConfirm = async () => {
    if (!deleting) return;
    try {
      await deleteIcon.mutateAsync(deleting.id);
      setDeleting(null);
    } catch (error) {
      toast.error(extractErrorMessage(error));
    }
  };

  const query = searchQuery.trim().toLowerCase();

  // 过滤图标列表
  const filteredIcons = useMemo(() => {
    if (!query) return iconList;
    return searchIcons(query);
  }, [query]);

  const filteredLibrary = useMemo(
    () =>
      query
        ? library.filter((icon) => icon.name.toLowerCase().includes(query))
        : library,
    [library, query],
  );

  // 当前用的是自定义图片、但不在库里：深链接带来的，或者从库里删掉了。
  // 单独显示出来，可以存进库
  const currentUnsaved =
    isCustomIcon(value) && !library.some((icon) => icon.data === value)
      ? value
      : undefined;

  const nameDialogText = (() => {
    switch (nameDialog?.mode) {
      case "rename":
        return {
          title: t("iconPicker.renameTitle"),
          confirm: t("common.save"),
        };
      case "saveCurrent":
        return {
          title: t("iconPicker.saveCurrentTitle"),
          confirm: t("iconPicker.saveToLibrary"),
        };
      default:
        return {
          title: t("iconPicker.uploadTitle"),
          description: t("iconPicker.uploadDescription"),
          confirm: t("iconPicker.saveAndUse"),
        };
    }
  })();

  const tileAction = (
    label: string,
    icon: React.ReactNode,
    onClick: () => void,
  ) => (
    <HoverTip content={label}>
      <button
        type="button"
        aria-label={label}
        onClick={onClick}
        className="rounded-md bg-surface/90 p-1 text-fg-2 shadow-sm hover:bg-subtle hover:text-fg-1"
      >
        {icon}
      </button>
    </HoverTip>
  );

  return (
    <div className="space-y-6">
      <div>
        <Label htmlFor="icon-search">
          {t("iconPicker.search", { defaultValue: "搜索图标" })}
        </Label>
        <div className="mt-2 flex gap-2">
          <Input
            id="icon-search"
            type="text"
            placeholder={t("iconPicker.searchPlaceholder", {
              defaultValue: "输入图标名称...",
            })}
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
          />
          {value && (
            <Button type="button" variant="ghost" onClick={() => select("")}>
              <X className="h-4 w-4" />
              {t("iconPicker.clear")}
            </Button>
          )}
        </div>
      </div>

      <section>
        <h3 className="mb-2 text-sm font-medium text-fg-2">
          {t("iconPicker.myIcons")}
        </h3>
        <div className={GRID_CLASS}>
          <HoverTip content={t("iconPicker.uploadHint")}>
            <button
              type="button"
              disabled={uploading}
              onClick={() => fileInputRef.current?.click()}
              className={cn(
                "flex w-full flex-col items-center gap-1 p-3 rounded-lg",
                "border-2 border-dashed border-border text-fg-2 transition-colors",
                "hover:border-primary/50 hover:bg-subtle hover:text-fg-1",
                "disabled:cursor-wait disabled:opacity-60",
              )}
            >
              <span className="flex h-8 w-8 items-center justify-center">
                {uploading ? (
                  <Loader2 className="h-5 w-5 animate-spin" />
                ) : (
                  <Plus className="h-5 w-5" />
                )}
              </span>
              <span className="text-xs truncate w-full text-center">
                {t("iconPicker.upload")}
              </span>
            </button>
          </HoverTip>
          <input
            ref={fileInputRef}
            type="file"
            accept={CUSTOM_ICON_ACCEPT}
            className="hidden"
            data-testid="icon-upload-input"
            onChange={handleFileChange}
          />

          {currentUnsaved && (
            <div className="group relative">
              <button
                type="button"
                className={tileClass(true)}
                title={t("iconPicker.currentIcon")}
              >
                <ProviderIconBox icon={currentUnsaved} name="" />
                <span className="text-xs text-fg-2 truncate w-full text-center">
                  {t("iconPicker.currentIcon")}
                </span>
              </button>
              <div className="absolute right-1 top-1 flex gap-0.5 opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100">
                {tileAction(
                  t("iconPicker.saveToLibrary"),
                  <BookmarkPlus className="h-3.5 w-3.5" />,
                  () =>
                    setNameDialog({
                      mode: "saveCurrent",
                      data: currentUnsaved,
                      initialName: defaultName,
                    }),
                )}
              </div>
            </div>
          )}

          {filteredLibrary.map((icon) => (
            <div key={icon.id} className="group relative">
              <button
                type="button"
                onClick={() => select(icon.data)}
                className={tileClass(value === icon.data)}
                title={icon.name}
              >
                <ProviderIconBox icon={icon.data} name={icon.name} />
                <span className="text-xs text-fg-2 truncate w-full text-center">
                  {icon.name}
                </span>
              </button>
              <div className="absolute right-1 top-1 flex gap-0.5 opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100">
                {tileAction(
                  t("iconPicker.rename"),
                  <Pencil className="h-3.5 w-3.5" />,
                  () =>
                    setNameDialog({
                      mode: "rename",
                      data: icon.data,
                      initialName: icon.name,
                      id: icon.id,
                    }),
                )}
                {tileAction(
                  t("iconPicker.delete"),
                  <Trash2 className="h-3.5 w-3.5" />,
                  () => setDeleting(icon),
                )}
              </div>
            </div>
          ))}
        </div>
        {library.length === 0 && !currentUnsaved && (
          <p className="mt-2 text-xs text-fg-3">
            {t("iconPicker.myIconsEmpty")}
          </p>
        )}
      </section>

      <section>
        <h3 className="mb-2 text-sm font-medium text-fg-2">
          {t("iconPicker.builtinIcons")}
        </h3>
        <div className={GRID_CLASS}>
          {filteredIcons.map((iconName) => {
            const meta = getIconMetadata(iconName);
            const isSelected = value === iconName;

            return (
              <button
                key={iconName}
                type="button"
                onClick={() => select(iconName)}
                className={tileClass(isSelected)}
                title={meta?.displayName || iconName}
              >
                <ProviderIcon icon={iconName} name={iconName} size={32} />
                <span className="text-xs text-fg-2 truncate w-full text-center">
                  {meta?.displayName || iconName}
                </span>
              </button>
            );
          })}
        </div>
        {filteredIcons.length === 0 && (
          <div className="text-center py-8 text-fg-2">
            {t("iconPicker.noResults", { defaultValue: "未找到匹配的图标" })}
          </div>
        )}
      </section>

      <CustomIconNameDialog
        icon={nameDialog?.data ?? null}
        initialName={nameDialog?.initialName ?? ""}
        title={nameDialogText.title}
        description={nameDialogText.description}
        confirmText={nameDialogText.confirm}
        pending={addIcon.isPending || renameIcon.isPending}
        onConfirm={handleNameConfirm}
        onCancel={() => setNameDialog(null)}
      />

      <ConfirmDialog
        isOpen={deleting !== null}
        zIndex="top"
        title={t("iconPicker.deleteTitle")}
        message={t("iconPicker.deleteMessage", { name: deleting?.name ?? "" })}
        confirmText={t("iconPicker.delete")}
        pending={deleteIcon.isPending}
        onConfirm={handleDeleteConfirm}
        onCancel={() => setDeleting(null)}
      />
    </div>
  );
};
