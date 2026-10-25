import { useEffect, useRef, useState } from "react";
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
  // 连着勾几列时保存会并发：还有保存没回来就不拿读回的设置覆盖（先回来的旧值会冲掉后勾的列），
  // 失败也只在最后一次保存回来时退回，退回到设置里当时实际存的
  const pendingSaves = useRef(0);
  const latestSaved = useRef(savedColumns);
  useEffect(() => {
    latestSaved.current = savedColumns;
    if (pendingSaves.current === 0) setColumnOverrides(savedColumns ?? {});
  }, [savedColumns]);
  const logColumns = resolveRequestLogColumns(columnOverrides);

  const saveColumnOverrides = async (next: Record<string, boolean>) => {
    setColumnOverrides(next);
    pendingSaves.current += 1;
    let saved = false;
    try {
      saved = (await onChange({ ...value, requestLogColumns: next })) !== false;
    } finally {
      pendingSaves.current -= 1;
      if (pendingSaves.current === 0) {
        setColumnOverrides(saved ? next : (latestSaved.current ?? {}));
      }
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
