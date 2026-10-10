export interface ColumnOption<K extends string = string> {
  id: K;
  label: string;
  required?: boolean;
}

/** Missing entries stay visible, so newly added columns appear by default. */
export type ColumnVisibility<K extends string = string> = Readonly<
  Partial<Record<K, boolean>>
>;
