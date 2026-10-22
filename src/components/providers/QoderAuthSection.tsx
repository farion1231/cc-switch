import { useState } from "react";
import { useTranslation } from "react-i18next";
import { qoderEdition } from "@/config/qoder";
import { useSettingsQuery } from "@/lib/query";
import type { QoderAppId } from "@/lib/api/qoder";
import { QoderAccountLogin } from "./QoderAccountLogin";

export function QoderAuthSection({ initialApp }: { initialApp?: QoderAppId }) {
  const { i18n } = useTranslation();
  const { data: settings } = useSettingsQuery();
  const [selectedApp, setSelectedApp] = useState<QoderAppId | null>(null);
  const app =
    selectedApp ??
    initialApp ??
    (qoderEdition(settings, i18n.language) === "cn" ? "qoder-cn" : "qoder");

  return <QoderAccountLogin key={app} app={app} onAppChange={setSelectedApp} />;
}
