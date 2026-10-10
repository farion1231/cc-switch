export interface ColumnOption<K extends string = string> {
  id: K;
  label: string;
  required?: boolean;
  /** Defaults to true; required columns are always visible. */
  defaultVisible?: boolean;
}

/** Missing entries use the column's default visibility. */
export type ColumnVisibility<K extends string = string> = Readonly<
  Partial<Record<K, boolean>>
>;
