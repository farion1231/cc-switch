import { useEffect, useState } from "react";
import { AlertTriangle } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

export interface CodexCatalogDuplicateGroup {
  model: string;
  rows: Array<{
    displayName?: string;
  }>;
}

interface CodexCatalogDuplicateDialogProps {
  groups: CodexCatalogDuplicateGroup[] | null;
  onClose: () => void;
}

export function CodexCatalogDuplicateDialog({
  groups,
  onClose,
}: CodexCatalogDuplicateDialogProps) {
  const { t } = useTranslation();
  const duplicateGroups = groups ?? [];
  const [open, setOpen] = useState(duplicateGroups.length > 0);

  useEffect(() => {
    setOpen(duplicateGroups.length > 0);
  }, [duplicateGroups.length]);

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent
        className="max-w-md"
        zIndex="alert"
        // 关闭动画结束后再交还焦点，避免 FocusScope 抢回焦点。
        onCloseAutoFocus={(event) => {
          event.preventDefault();
          onClose();
        }}
      >
        <DialogHeader className="space-y-3 border-b-0 bg-transparent pb-0">
          <DialogTitle className="flex items-center gap-2 text-lg font-semibold">
            <AlertTriangle className="h-5 w-5 text-destructive" />
            {t("codexConfig.catalogDuplicateDialogTitle", {
              defaultValue: "无法保存：实际请求模型重复",
            })}
          </DialogTitle>
          <DialogDescription asChild>
            <div className="space-y-3 text-sm leading-relaxed text-fg-2">
              <p>
                {t("codexConfig.catalogDuplicateDialogIntro", {
                  defaultValue: "请调整菜单配置，避免“实际请求模型”重复。",
                })}
              </p>
              <ul className="space-y-2">
                {duplicateGroups.map((group) => (
                  <li
                    key={group.model}
                    className="rounded-control border border-border bg-subtle px-3 py-2"
                  >
                    <div className="flex flex-wrap items-center gap-1.5 text-xs">
                      <span className="text-fg-2">
                        {t("codexConfig.catalogColumnModel", {
                          defaultValue: "实际请求模型",
                        })}
                        ：
                      </span>
                      <code className="font-mono text-xs text-fg-1">
                        {group.model}
                      </code>
                    </div>
                    <div className="mt-2 text-xs text-fg-2">
                      {t("codexConfig.catalogDuplicateRowsUsing", {
                        count: group.rows.length,
                        defaultValue: "重复菜单：",
                      })}
                    </div>
                    <ul className="mt-1 space-y-1 text-xs">
                      {group.rows.map((row, index) => (
                        <li
                          key={`${group.model}-${index}`}
                          className="flex gap-1.5"
                        >
                          <span aria-hidden="true" className="text-fg-3">
                            •
                          </span>
                          <span>
                            <span className="text-fg-2">
                              {t("codexConfig.catalogColumnDisplay", {
                                defaultValue: "菜单显示名",
                              })}
                              ：
                            </span>
                            <span className="text-fg-1">
                              {row.displayName?.trim() ||
                                t(
                                  "codexConfig.catalogDuplicateDisplayNameEmpty",
                                  {
                                    defaultValue: "（未填写，使用实际模型名）",
                                  },
                                )}
                            </span>
                          </span>
                        </li>
                      ))}
                    </ul>
                  </li>
                ))}
              </ul>
            </div>
          </DialogDescription>
        </DialogHeader>
        <DialogFooter className="border-t-0 bg-transparent pt-2">
          <Button type="button" variant="solid" onClick={() => setOpen(false)}>
            {t("codexConfig.catalogDuplicateReturn", {
              defaultValue: "返回修改",
            })}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
