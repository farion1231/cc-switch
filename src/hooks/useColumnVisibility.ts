import { useCallback, useMemo, useRef, useState } from "react";
import type { ColumnOption, ColumnVisibility } from "@/types/table";

type ColumnRule<K extends string> = Pick<ColumnOption<K>, "id" | "required">;

function readHiddenColumns<K extends string>(
  storageKey: string,
  columns: readonly ColumnRule<K>[],
): Set<K> {
  try {
    const parsed: unknown = JSON.parse(
      localStorage.getItem(storageKey) ?? "[]",
    );
    if (
      !Array.isArray(parsed) ||
      !parsed.every((id) => typeof id === "string")
    ) {
      return new Set();
    }
    return new Set(
      columns
        .filter((column) => !column.required && parsed.includes(column.id))
        .map((column) => column.id),
    );
  } catch {
    return new Set();
  }
}

/** One stable storage key per table; only hidden IDs are persisted. */
export function useColumnVisibility<K extends string>(
  storageKey: string,
  columns: readonly ColumnRule<K>[],
) {
  const [hidden, setHidden] = useState(() =>
    readHiddenColumns(storageKey, columns),
  );
  // Event handlers can run in one React batch; keep their latest choice available.
  const hiddenRef = useRef(hidden);

  const visibility = useMemo<ColumnVisibility<K>>(
    () =>
      Object.fromEntries(
        columns.map((column) => [
          column.id,
          column.required || !hidden.has(column.id),
        ]),
      ) as Record<K, boolean>,
    [columns, hidden],
  );

  const commit = useCallback(
    (next: Set<K>) => {
      const ids = columns
        .filter((column) => !column.required && next.has(column.id))
        .map((column) => column.id);
      const normalized = new Set(ids);
      hiddenRef.current = normalized;
      setHidden(normalized);
      try {
        localStorage.setItem(storageKey, JSON.stringify(ids));
      } catch {
        // Storage may be unavailable; the choice still applies to this mount.
      }
    },
    [columns, storageKey],
  );

  const setVisible = useCallback(
    (id: K, visible: boolean) => {
      const column = columns.find((candidate) => candidate.id === id);
      if (!column || column.required) return;
      const next = new Set(hiddenRef.current);
      if (visible) next.delete(id);
      else next.add(id);
      commit(next);
    },
    [columns, commit],
  );

  const reset = useCallback(() => commit(new Set()), [commit]);

  return { visibility, setVisible, reset };
}
