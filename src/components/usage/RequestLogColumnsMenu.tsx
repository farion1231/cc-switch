import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Columns3 } from "lucide-react";
import type { UsageDisplaySettings } from "@/types";
import { ChecklistMenu } from "./ChecklistMenu";
import {
  REQUEST_LOG_COLUMN_LABEL_KEYS,
  resolveRequestLogColumns,
  setRequestLogColumn,
  TOGGLEABLE_REQUEST_LOG_COLUMNS,
  type RequestLogColumnId,
} from "./requestLogColumns";

interface RequestLogColumnsMenuProps {
  value: UsageDisplaySettings | undefined;
  onChange: (next: UsageDisplaySettings) => Promise<boolean> | boolean | void;
}

/** 请求日志工具栏的「显示的列」：只存改过的列；保存要等设置读回来，先按刚勾的显示 */
export function RequestLogColumnsMenu({
  value,
  onChange,
}: RequestLogColumnsMenuProps) {
  const { t } = useTranslation();
  const savedColumns = value?.requestLogColumns;
  const [columnOverrides, setColumnOverrides] = useState(savedColumns ?? {});
  useEffect(() => setColumnOverrides(savedColumns ?? {}), [savedColumns]);
  const logColumns = resolveRequestLogColumns(columnOverrides);

  const saveColumnOverrides = async (next: Record<string, boolean>) => {
    setColumnOverrides(next);
    if ((await onChange({ ...value, requestLogColumns: next })) === false) {
      setColumnOverrides(savedColumns ?? {});
    }
  };

  return (
    <ChecklistMenu
      quiet
      icon={<Columns3 aria-hidden="true" className="h-4 w-4" />}
      label={t("usage.display.logColumns")}
      summary={t("usage.display.logColumns")}
      items={TOGGLEABLE_REQUEST_LOG_COLUMNS.map(({ id }) => ({
        id,
        label: t(REQUEST_LOG_COLUMN_LABEL_KEYS[id]),
        checked: logColumns.includes(id),
      }))}
      onToggle={(id) => {
        const column = id as RequestLogColumnId;
        void saveColumnOverrides(
          setRequestLogColumn(
            columnOverrides,
            column,
            !logColumns.includes(column),
          ),
        );
      }}
      onReset={
        Object.keys(columnOverrides).length > 0
          ? () => void saveColumnOverrides({})
          : undefined
      }
    />
  );
}
