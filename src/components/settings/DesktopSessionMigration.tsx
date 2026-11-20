import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { isMac } from "@/lib/platform";
import { DesktopSessionMigrationDialog } from "./DesktopSessionMigrationDialog";
export function DesktopSessionMigration() {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  if (!isMac()) return null;
  return (
    <>
      <section className="flex flex-wrap items-center gap-3 rounded-panel border border-border bg-surface p-4">
        <div className="min-w-0 flex-1">
          <h3 className="text-body font-medium">
            {t("desktopMigration.title")}
          </h3>
          <p className="mt-1 text-caption text-fg-2">
            {t("desktopMigration.description")}
          </p>
        </div>
        <Button variant="neutral" onClick={() => setOpen(true)}>
          {t("desktopMigration.title")}
        </Button>
      </section>
      <DesktopSessionMigrationDialog open={open} onOpenChange={setOpen} />
    </>
  );
}
