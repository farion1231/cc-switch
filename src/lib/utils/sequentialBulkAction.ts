export interface SequentialBulkActionFailure<T> {
  item: T;
  error: unknown;
}

export interface SequentialBulkActionResult<T> {
  succeeded: T[];
  failed: Array<SequentialBulkActionFailure<T>>;
}

/**
 * Runs local configuration writes in order. Several app adapters update an
 * entire config file, so parallel writes can overwrite one another.
 */
export async function runSequentialBulkAction<T>(
  items: readonly T[],
  action: (item: T) => Promise<unknown>,
): Promise<SequentialBulkActionResult<T>> {
  const succeeded: T[] = [];
  const failed: Array<SequentialBulkActionFailure<T>> = [];

  for (const item of items) {
    try {
      await action(item);
      succeeded.push(item);
    } catch (error) {
      failed.push({ item, error });
    }
  }

  return { succeeded, failed };
}

/** 一条成功的写入，连同 action 解析出的返回值。 */
export interface SequentialBulkActionSuccess<T, R> {
  item: T;
  result: R;
}

export interface SequentialBulkActionOutcome<T, R> {
  succeeded: Array<SequentialBulkActionSuccess<T, R>>;
  failed: Array<SequentialBulkActionFailure<T>>;
}

/**
 * 同 [`runSequentialBulkAction`]，但保留每条 action 的返回值。
 *
 * 有些后端把"部分成功"放在响应体里：卸载 Skill 可能删掉了管理记录却留下文件，
 * 并在 **Ok 响应** 上用 `piCleanupIncomplete` / `preservedPiPath` 说明。批量
 * 调用方若只统计成功条数，就会把这些提示吞掉、对用户显示"全部完成"。
 */
export async function runSequentialBulkActionCollect<T, R>(
  items: readonly T[],
  action: (item: T) => Promise<R>,
): Promise<SequentialBulkActionOutcome<T, R>> {
  const succeeded: Array<SequentialBulkActionSuccess<T, R>> = [];
  const failed: Array<SequentialBulkActionFailure<T>> = [];

  for (const item of items) {
    try {
      const result = await action(item);
      succeeded.push({ item, result });
    } catch (error) {
      failed.push({ item, error });
    }
  }

  return { succeeded, failed };
}
