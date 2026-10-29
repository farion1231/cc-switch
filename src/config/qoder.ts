import type { Settings, VisibleApps } from "@/types";
import { DEFAULT_VISIBLE_APPS } from "./appConfig";

export type QoderEdition = "global" | "cn";

export function qoderEdition(
  settings?: Pick<Settings, "qoderEdition" | "language"> | null,
  language = "en",
): QoderEdition {
  return (
    settings?.qoderEdition ??
    ((settings?.language ?? language).toLowerCase().startsWith("zh")
      ? "cn"
      : "global")
  );
}

export function isQoderApp(app: string): app is "qoder" | "qoder-cn" {
  return app === "qoder" || app === "qoder-cn";
}

/** Editions have separate native data and enablement flags; only the selected one is shown. */
export function resolveVisibleApps(
  settings?: Pick<Settings, "visibleApps" | "qoderEdition" | "language"> | null,
  language = "en",
): VisibleApps {
  const visible = { ...DEFAULT_VISIBLE_APPS, ...settings?.visibleApps };
  visible[qoderEdition(settings, language) === "cn" ? "qoder" : "qoder-cn"] =
    false;
  return visible;
}
