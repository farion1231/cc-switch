import { useUpdate } from "@/contexts/UpdateContext";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";

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
        relative h-6 w-6 rounded-full
        ${isActive ? "text-blue-600 dark:text-blue-300 hover:bg-blue-50 dark:hover:bg-blue-500/10" : "text-muted-foreground hover:bg-muted/60"}
        ${className}
      `}
    >
      <span
        className={`
          absolute inset-0 m-auto h-2 w-2 rounded-full ring-1 ring-background
          ${isActive ? "bg-blue-500 dark:bg-blue-400" : "bg-blue-300/70 dark:bg-blue-300/60"}
        `}
      />
    </Button>
  );
}
