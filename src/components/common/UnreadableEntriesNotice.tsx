import { useState } from "react";
import { useTranslation } from "react-i18next";
import {
  useMutation,
  useQuery,
  useQueryClient,
  type QueryKey,
} from "@tanstack/react-query";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { Button } from "@/components/ui/button";
import { Notice } from "@/components/ui/notice";
import { toast } from "@/lib/toast";
import { extractErrorMessage } from "@/utils/errorUtils";
import type { UnreadableEntry } from "@/types";

interface UnreadableEntriesNoticeProps {
  queryKey: QueryKey;
  load: () => Promise<UnreadableEntry[]>;
  remove: (id: string) => Promise<unknown>;
  /** 删掉之后要重读的列表 */
  invalidate: QueryKey[];
}

/**
 * 库里存的数据读不出来的条目（比如由更新版本写入）：不进正常列表，也就不能编辑、切换或启用。
 * 这里逐条列出它们和后端的解析错误，只给一个删除按钮；原始数据在删除前保持不动。
 */
export function UnreadableEntriesNotice({
  queryKey,
  load,
  remove,
  invalidate,
}: UnreadableEntriesNoticeProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const { data: entries = [] } = useQuery({ queryKey, queryFn: load });
  const [pending, setPending] = useState<UnreadableEntry | null>(null);
  const deletion = useMutation({
    mutationFn: (entry: UnreadableEntry) => remove(entry.id),
    onSuccess: async () => {
      setPending(null);
      await Promise.all(
        [queryKey, ...invalidate].map((key) =>
          queryClient.invalidateQueries({ queryKey: key }),
        ),
      );
    },
    onError: (error) => {
      toast.error(extractErrorMessage(error) || String(error));
    },
  });

  if (entries.length === 0) return null;
  return (
    <>
      {entries.map((entry) => (
        <Notice
          key={entry.id}
          tone="danger"
          title={t("unreadableEntry.title", {
            name: entry.name || entry.id,
            defaultValue: "「{{name}}」的数据无法读取",
          })}
          actions={
            <Button
              type="button"
              size="compact"
              variant="neutral"
              onClick={() => setPending(entry)}
            >
              {t("common.delete")}
            </Button>
          }
        >
          {t("unreadableEntry.hint", {
            defaultValue:
              "可能由更新版本的 CC Switch 写入。数据已原样保留，不能编辑或启用；升级后即可恢复，也可以删除。",
          })}{" "}
          {entry.error}
        </Notice>
      ))}
      <ConfirmDialog
        isOpen={pending !== null}
        title={t("unreadableEntry.confirmTitle", {
          defaultValue: "删除无法读取的条目",
        })}
        message={t("unreadableEntry.confirmMessage", {
          name: pending?.name || pending?.id || "",
          defaultValue:
            "删除「{{name}}」？只删除 CC Switch 里的这条记录，不改动各应用自己的配置文件。",
        })}
        confirmText={t("common.delete")}
        pending={deletion.isPending}
        onConfirm={() => pending && deletion.mutate(pending)}
        onCancel={() => setPending(null)}
      />
    </>
  );
}
