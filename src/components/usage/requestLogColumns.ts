/**
 * 请求日志的列：顺序即表格从左到右的顺序。`locked` 的列不能关（时间列是打开详情的入口）。
 * 设置里只存用户改过的列（id → 是否显示），没写的按这里的默认值；以后新加列不会被老用户的设置冻住。
 */
export const REQUEST_LOG_COLUMNS = [
  { id: "time", labelKey: "usage.time", defaultVisible: true, locked: true },
  { id: "app", labelKey: "usage.app", defaultVisible: true },
  { id: "provider", labelKey: "usage.provider", defaultVisible: true },
  { id: "model", labelKey: "usage.model", defaultVisible: true },
  { id: "freshInput", labelKey: "usage.freshInput", defaultVisible: true },
  { id: "output", labelKey: "usage.outputTokens", defaultVisible: true },
  { id: "cacheRead", labelKey: "usage.cacheReadTokens", defaultVisible: true },
  {
    id: "cacheWrite",
    labelKey: "usage.logColumns.cacheWrite",
    defaultVisible: false,
  },
  {
    id: "hitRate",
    labelKey: "usage.logColumns.hitRate",
    defaultVisible: false,
  },
  { id: "cost", labelKey: "usage.cost", defaultVisible: true },
  { id: "speed", labelKey: "usage.speed", defaultVisible: true },
  { id: "firstToken", labelKey: "usage.firstToken", defaultVisible: true },
  {
    id: "duration",
    labelKey: "usage.logColumns.duration",
    defaultVisible: false,
  },
  { id: "statusCode", labelKey: "usage.statusCode", defaultVisible: false },
] as const satisfies readonly {
  id: string;
  /** 表头和「显示的列」菜单共用的文案 */
  labelKey: string;
  defaultVisible: boolean;
  locked?: boolean;
}[];

export type RequestLogColumnId = (typeof REQUEST_LOG_COLUMNS)[number]["id"];

const isLocked = (column: (typeof REQUEST_LOG_COLUMNS)[number]) =>
  "locked" in column && column.locked;

export const REQUEST_LOG_COLUMN_LABEL_KEYS = Object.fromEntries(
  REQUEST_LOG_COLUMNS.map((column) => [column.id, column.labelKey]),
) as Record<RequestLogColumnId, string>;

/** 菜单里能开关的列（不含时间列） */
export const TOGGLEABLE_REQUEST_LOG_COLUMNS = REQUEST_LOG_COLUMNS.filter(
  (column) => !isLocked(column),
);

/** 按表格顺序列出要显示的列；认不出的 id（旧版或手改的）忽略 */
export function resolveRequestLogColumns(
  overrides: Record<string, boolean> | undefined,
): RequestLogColumnId[] {
  return REQUEST_LOG_COLUMNS.filter((column) => {
    if (isLocked(column)) return true;
    const override = overrides?.[column.id];
    return typeof override === "boolean" ? override : column.defaultVisible;
  }).map((column) => column.id);
}

/** 改一列：和默认值相同就把这条覆盖删掉，设置里只留真正改过的 */
export function setRequestLogColumn(
  overrides: Record<string, boolean> | undefined,
  id: RequestLogColumnId,
  visible: boolean,
): Record<string, boolean> {
  const column = REQUEST_LOG_COLUMNS.find((c) => c.id === id);
  const next = { ...overrides };
  if (!column || isLocked(column)) return next;
  if (visible === column.defaultVisible) {
    delete next[id];
  } else {
    next[id] = visible;
  }
  return next;
}
