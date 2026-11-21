/**
 * Target builders. Importable only from parity/screens. A target is a role plus the exact
 * accessible name a user reads; there is no index, no pattern and no test id.
 */
import { makeTarget, specOf, type Target } from "./internal/target.ts";

export type Role =
  | "button" | "link" | "textbox" | "switch" | "checkbox" | "radio" | "combobox" | "option"
  | "tab" | "tabpanel" | "menu" | "menuitem" | "dialog" | "alertdialog" | "listitem" | "list"
  | "heading" | "status" | "alert" | "navigation" | "region" | "row" | "cell" | "group" | "main";

export const by = {
  role(role: Role, name: string, opts?: { within?: Target }): Target {
    return makeTarget({ kind: "role", role, name, within: opts?.within ? specOf(opts.within) : undefined });
  },
  /** A textbox or combobox whose accessible name comes from its label. */
  field(label: string, opts?: { within?: Target }): Target {
    return makeTarget({ kind: "field", name: label, within: opts?.within ? specOf(opts.within) : undefined });
  },
  /** A run of visible text, by its full text. */
  text(text: string, opts?: { within?: Target }): Target {
    return makeTarget({ kind: "text", name: text, within: opts?.within ? specOf(opts.within) : undefined });
  },
};
