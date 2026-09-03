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
