import { useUpdate } from "@/contexts/UpdateContext";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { ArrowUpCircle } from "lucide-react";

interface UpdateBadgeProps {
  className?: string;
  onClick?: () => void;
}

export function UpdateBadge({ className = "", onClick }: UpdateBadgeProps) {
  const { hasUpdate, updateInfo } = useUpdate();
  const { t } = useTranslation();
  const isActive = hasUpdate && updateInfo;
  const title = isActive
    ? t("settings.updateAvailable", {
        version: updateInfo?.availableVersion ?? "",
      })
    : t("settings.checkForUpdates");

  if (!isActive) {
    return null;
  }

  return (
    <Button
      type="button"
      variant="ghost"
      size="icon"
      title={title}
      aria-label={title}
      onClick={onClick}
      className={`
        relative h-8 w-8 rounded-full
        ${isActive ? "text-primary hover:bg-primary/10" : "text-muted-foreground hover:bg-muted/60"}
        ${className}
      `}
    >
      <ArrowUpCircle className="h-5 w-5" />
    </Button>
  );
}
