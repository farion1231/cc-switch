import { useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Download, Trash2, Loader2, FileText } from "lucide-react";
import type { TemplateComponent } from "@/types/template";

interface ComponentCardProps {
  component: TemplateComponent;
  onInstall: () => Promise<void>;
  onUninstall: () => Promise<void>;
  onViewDetail: () => void;
}

// 组件类型图标映射
const componentTypeIcons: Record<string, string> = {
  agent: "🤖",
  command: "⚡",
  mcp: "🔌",
  setting: "⚙️",
  hook: "🪝",
  skill: "💡",
};

export function ComponentCard({
  component,
  onInstall,
  onUninstall,
  onViewDetail,
}: ComponentCardProps) {
  const { t } = useTranslation();
  const [loading, setLoading] = useState(false);

  const handleInstall = async () => {
    setLoading(true);
    try {
      await onInstall();
    } finally {
      setLoading(false);
    }
  };

  const handleUninstall = async () => {
    setLoading(true);
    try {
      await onUninstall();
    } finally {
      setLoading(false);
    }
  };

  const typeIcon = componentTypeIcons[component.componentType] || "📦";

  return (
    <Card className="glass-card flex flex-col h-full transition-all duration-300 hover:scale-[1.01] hover:shadow-lg group relative overflow-hidden cursor-pointer">
      <div className="absolute inset-0 bg-gradient-to-br from-primary/5 to-transparent opacity-0 group-hover:opacity-100 transition-opacity duration-500 pointer-events-none" />

      <div onClick={onViewDetail}>
        <CardHeader className="pb-3">
          <div className="flex items-start justify-between gap-2">
            <div className="flex-1 min-w-0">
              <div className="flex items-center gap-2 mb-1.5">
                <span className="text-2xl">{typeIcon}</span>
              </div>
              <CardTitle className="text-base font-semibold truncate">
                {component.name}
              </CardTitle>
              {component.category && (
                <CardDescription className="text-xs mt-1">
                  <Badge
                    variant="outline"
                    className="text-[10px] px-1.5 py-0 h-4 border-border-default"
                  >
                    {component.category}
                  </Badge>
                </CardDescription>
              )}
            </div>
            {component.installed && (
              <Badge
                variant="default"
                className="shrink-0 bg-green-600/90 hover:bg-green-600 dark:bg-green-700/90 dark:hover:bg-green-700 text-white border-0"
              >
                {t("templates.installed", { defaultValue: "已安装" })}
              </Badge>
            )}
          </div>
        </CardHeader>

        <CardContent className="flex-1 pt-0">
          <p className="text-sm text-muted-foreground/90 line-clamp-3 leading-relaxed">
            {component.description ||
              t("templates.noDescription", { defaultValue: "暂无描述" })}
          </p>
        </CardContent>
      </div>

      <CardFooter className="flex gap-2 pt-3 border-t border-border/50 relative z-10">
        <Button
          variant="ghost"
          size="sm"
          onClick={(e) => {
            e.stopPropagation();
            onViewDetail();
          }}
          disabled={loading}
          className="flex-1"
        >
          <FileText className="h-3.5 w-3.5 mr-1.5" />
          {t("templates.viewDetail", { defaultValue: "查看详情" })}
        </Button>

        {component.installed ? (
          <Button
            variant="outline"
            size="sm"
            onClick={(e) => {
              e.stopPropagation();
              handleUninstall();
            }}
            disabled={loading}
            className="flex-1 border-red-300 text-red-500 hover:bg-red-50 hover:text-red-600 dark:border-red-500/50 dark:text-red-400 dark:hover:bg-red-900/30 dark:hover:text-red-300"
          >
            {loading ? (
              <Loader2 className="h-3.5 w-3.5 mr-1.5 animate-spin" />
            ) : (
              <Trash2 className="h-3.5 w-3.5 mr-1.5" />
            )}
            {loading
              ? t("templates.uninstalling", { defaultValue: "卸载中..." })
              : t("templates.uninstall", { defaultValue: "卸载" })}
          </Button>
        ) : (
          <Button
            variant="mcp"
            size="sm"
            onClick={(e) => {
              e.stopPropagation();
              handleInstall();
            }}
            disabled={loading}
            className="flex-1"
          >
            {loading ? (
              <Loader2 className="h-3.5 w-3.5 mr-1.5 animate-spin" />
            ) : (
              <Download className="h-3.5 w-3.5 mr-1.5" />
            )}
            {loading
              ? t("templates.installing", { defaultValue: "安装中..." })
              : t("templates.install", { defaultValue: "安装" })}
          </Button>
        )}
      </CardFooter>
    </Card>
  );
}
