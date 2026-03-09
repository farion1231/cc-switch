import React, { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Plus, Trash2, Save, TriangleAlert } from "lucide-react";
import { toast } from "sonner";
import { useOpenClawTools, useSaveOpenClawTools } from "@/hooks/useOpenClaw";
import { extractErrorMessage } from "@/utils/errorUtils";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { OpenClawToolsConfig, OpenClawToolsProfile } from "@/types";
import {
  getOpenClawToolsProfileSelectValue,
  getOpenClawUnsupportedProfile,
  OPENCLAW_TOOL_PROFILES,
  OPENCLAW_UNSET_PROFILE,
  OPENCLAW_UNSUPPORTED_PROFILE,
} from "./utils";

interface ListItem {
  id: string;
  value: string;
}

const ToolsPanel: React.FC = () => {
  const { t } = useTranslation();
  const { data: toolsData, isLoading } = useOpenClawTools();
  const saveToolsMutation = useSaveOpenClawTools();
  const [config, setConfig] = useState<OpenClawToolsConfig>({});
  const [allowList, setAllowList] = useState<ListItem[]>([]);
  const [denyList, setDenyList] = useState<ListItem[]>([]);

  useEffect(() => {
    if (toolsData) {
      setConfig(toolsData);
      setAllowList(
        (toolsData.allow ?? []).map((value) => ({
          id: crypto.randomUUID(),
          value,
        })),
      );
      setDenyList(
        (toolsData.deny ?? []).map((value) => ({
          id: crypto.randomUUID(),
          value,
        })),
      );
    }
  }, [toolsData]);

  const unsupportedProfile = getOpenClawUnsupportedProfile(config.profile);

  const profileLabels = useMemo<Record<OpenClawToolsProfile, string>>(
    () => ({
      minimal: t("openclaw.tools.profileMinimal", {
        defaultValue: "Minimal",
      }),
      coding: t("openclaw.tools.profileCoding", {
        defaultValue: "Coding",
      }),
      messaging: t("openclaw.tools.profileMessaging", {
        defaultValue: "Messaging",
      }),
      full: t("openclaw.tools.profileFull", {
        defaultValue: "Full",
      }),
    }),
    [t],
  );

  const handleSave = async () => {
    try {
      const { allow, deny, ...other } = config;
      const newConfig: OpenClawToolsConfig = {
        ...other,
        profile: config.profile,
        allow: allowList.map((item) => item.value).filter((s) => s.trim()),
        deny: denyList.map((item) => item.value).filter((s) => s.trim()),
      };

      await saveToolsMutation.mutateAsync(newConfig);
      toast.success(t("openclaw.tools.saveSuccess"));
    } catch (error) {
      const detail = extractErrorMessage(error);
      toast.error(t("openclaw.tools.saveFailed"), {
        description: detail || undefined,
      });
    }
  };

  const updateListItem = (
    setList: React.Dispatch<React.SetStateAction<ListItem[]>>,
    index: number,
    value: string,
  ) => {
    setList((prev) =>
      prev.map((item, i) => (i === index ? { ...item, value } : item)),
    );
  };

  const removeListItem = (
    setList: React.Dispatch<React.SetStateAction<ListItem[]>>,
    index: number,
  ) => {
    setList((prev) => prev.filter((_, i) => i !== index));
  };

  if (isLoading) {
    return (
      <div className="px-6 pt-4 pb-8 flex items-center justify-center min-h-[200px]">
        <div className="text-sm text-muted-foreground">
          {t("common.loading")}
        </div>
      </div>
    );
  }

  return (
    <div className="px-6 pt-4 pb-8">
      <p className="text-sm text-muted-foreground mb-6">
        {t("openclaw.tools.description")}
      </p>

      {unsupportedProfile && (
        <Alert className="mb-6 border-amber-500/30 bg-amber-500/5">
          <TriangleAlert className="h-4 w-4" />
          <AlertTitle>
            {t("openclaw.tools.unsupportedProfileTitle", {
              defaultValue: "Unsupported tools profile",
            })}
          </AlertTitle>
          <AlertDescription>
            {t("openclaw.tools.unsupportedProfileDescription", {
              value: unsupportedProfile,
              defaultValue:
                "The current tools.profile value '{{value}}' is not in the supported OpenClaw list. It will be preserved until you choose a new value.",
            })}
          </AlertDescription>
        </Alert>
      )}

      <div className="mb-6">
        <Label className="mb-2 block">{t("openclaw.tools.profile")}</Label>
        <Select
          value={getOpenClawToolsProfileSelectValue(config.profile)}
          onValueChange={(value) => {
            if (value === OPENCLAW_UNSUPPORTED_PROFILE) return;
            if (value === OPENCLAW_UNSET_PROFILE) {
              setConfig((prev) => ({ ...prev, profile: undefined }));
              return;
            }
            setConfig((prev) => ({ ...prev, profile: value }));
          }}
        >
          <SelectTrigger className="w-[220px]">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={OPENCLAW_UNSET_PROFILE}>
              {t("openclaw.tools.profileUnset", {
                defaultValue: "Not set",
              })}
            </SelectItem>
            {unsupportedProfile && (
              <SelectItem
                value={OPENCLAW_UNSUPPORTED_PROFILE}
                disabled={true}
              >{`${unsupportedProfile} (${t(
                "openclaw.tools.unsupportedProfileLabel",
                {
                  defaultValue: "unsupported",
                },
              )})`}</SelectItem>
            )}
            {OPENCLAW_TOOL_PROFILES.map((profile) => (
              <SelectItem key={profile} value={profile}>
                {profileLabels[profile]}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      <div className="mb-6">
        <Label className="mb-2 block">{t("openclaw.tools.allowList")}</Label>
        <div className="space-y-2">
          {allowList.map((item, index) => (
            <div key={item.id} className="flex items-center gap-2">
              <Input
                value={item.value}
                onChange={(e) =>
                  updateListItem(setAllowList, index, e.target.value)
                }
                placeholder={t("openclaw.tools.patternPlaceholder")}
                className="font-mono text-xs"
              />
              <Button
                variant="ghost"
                size="icon"
                className="flex-shrink-0 h-9 w-9 text-muted-foreground hover:text-destructive"
                onClick={() => removeListItem(setAllowList, index)}
              >
                <Trash2 className="w-4 h-4" />
              </Button>
            </div>
          ))}
          <Button
            variant="outline"
            size="sm"
            onClick={() =>
              setAllowList((prev) => [
                ...prev,
                { id: crypto.randomUUID(), value: "" },
              ])
            }
          >
            <Plus className="w-4 h-4 mr-1" />
            {t("openclaw.tools.addAllow")}
          </Button>
        </div>
      </div>

      <div className="mb-6">
        <Label className="mb-2 block">{t("openclaw.tools.denyList")}</Label>
        <div className="space-y-2">
          {denyList.map((item, index) => (
            <div key={item.id} className="flex items-center gap-2">
              <Input
                value={item.value}
                onChange={(e) =>
                  updateListItem(setDenyList, index, e.target.value)
                }
                placeholder={t("openclaw.tools.patternPlaceholder")}
                className="font-mono text-xs"
              />
              <Button
                variant="ghost"
                size="icon"
                className="flex-shrink-0 h-9 w-9 text-muted-foreground hover:text-destructive"
                onClick={() => removeListItem(setDenyList, index)}
              >
                <Trash2 className="w-4 h-4" />
              </Button>
            </div>
          ))}
          <Button
            variant="outline"
            size="sm"
            onClick={() =>
              setDenyList((prev) => [
                ...prev,
                { id: crypto.randomUUID(), value: "" },
              ])
            }
          >
            <Plus className="w-4 h-4 mr-1" />
            {t("openclaw.tools.addDeny")}
          </Button>
        </div>
      </div>

      <div className="flex justify-end">
        <Button
          size="sm"
          onClick={handleSave}
          disabled={saveToolsMutation.isPending}
        >
          <Save className="w-4 h-4 mr-1" />
          {saveToolsMutation.isPending ? t("common.saving") : t("common.save")}
        </Button>
      </div>
    </div>
  );
};

export default ToolsPanel;
