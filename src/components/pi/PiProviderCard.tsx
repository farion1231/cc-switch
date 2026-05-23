import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { MoreHorizontal, Pencil, Trash2, Star } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import type { PiProviderConfig } from "@/lib/api/pi";

interface PiProviderCardProps {
  id: string;
  config: PiProviderConfig;
  isActive: boolean;
  onEdit: (id: string) => void;
  onDelete: (id: string) => void;
  onSetActive: (id: string) => void;
}

const API_LABELS: Record<string, string> = {
  "openai-completions": "OpenAI Compatible",
  "openai-responses": "OpenAI Responses",
  "anthropic-messages": "Anthropic Messages",
  "google-generative-ai": "Google Generative AI",
};

export function PiProviderCard({
  id,
  config,
  isActive,
  onEdit,
  onDelete,
  onSetActive,
}: PiProviderCardProps) {
  const { t } = useTranslation();

  return (
    <Card className={isActive ? "border-primary" : ""}>
      <CardHeader className="pb-3">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <CardTitle className="text-base">{id.replace("cc-switch-", "")}</CardTitle>
            {isActive && (
              <Badge variant="default" className="text-xs">
                <Star className="w-3 h-3 mr-1" />
                {t("active")}
              </Badge>
            )}
          </div>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="ghost" size="icon" className="h-8 w-8">
                <MoreHorizontal className="w-4 h-4" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              {!isActive && (
                <DropdownMenuItem onClick={() => onSetActive(id)}>
                  <Star className="w-4 h-4 mr-2" />
                  {t("setActive")}
                </DropdownMenuItem>
              )}
              <DropdownMenuItem onClick={() => onEdit(id)}>
                <Pencil className="w-4 h-4 mr-2" />
                {t("edit")}
              </DropdownMenuItem>
              <DropdownMenuItem
                onClick={() => onDelete(id)}
                className="text-destructive"
              >
                <Trash2 className="w-4 h-4 mr-2" />
                {t("delete")}
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
        <CardDescription className="flex items-center gap-2">
          <Badge variant="secondary" className="text-xs">
            {API_LABELS[config.api] ?? config.api}
          </Badge>
        </CardDescription>
      </CardHeader>
      <CardContent className="text-sm text-muted-foreground">
        <div className="truncate">{config.baseUrl}</div>
        {config.models.length > 0 && (
          <div className="mt-1 text-xs">
            {config.models.length} model{config.models.length > 1 ? "s" : ""} configured
          </div>
        )}
      </CardContent>
    </Card>
  );
}
