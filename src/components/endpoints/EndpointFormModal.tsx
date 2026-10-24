import { useState, useEffect, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { Eye, EyeOff } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { toast } from "@/lib/toast";
import type { Endpoint, EndpointApiFormat } from "@/types";

interface EndpointFormModalProps {
  isOpen: boolean;
  onClose: () => void;
  onSave: (endpoint: Endpoint) => void;
  editing: Endpoint | null;
}

const API_FORMATS: EndpointApiFormat[] = [
  "anthropic",
  "openai_chat",
  "openai_responses",
  "gemini_native",
];

const FORMAT_LABELS: Record<EndpointApiFormat, string> = {
  anthropic: "Anthropic Messages",
  openai_chat: "OpenAI Chat Completions",
  openai_responses: "OpenAI Responses",
  gemini_native: "Gemini Native",
};

/**
 * 端点表单：连接信息（地址/密钥/协议格式）。
 * 协议格式决定代理转发时是否做 wire 转换（跨端点档位路由的关键）；
 * 缺省 anthropic（大多数中转站即此）。
 */
export function EndpointFormModal({
  isOpen,
  onClose,
  onSave,
  editing,
}: EndpointFormModalProps) {
  const { t } = useTranslation();

  const [name, setName] = useState("");
  const [baseUrl, setBaseUrl] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [apiFormat, setApiFormat] = useState<EndpointApiFormat>("anthropic");
  const [notes, setNotes] = useState("");
  const [showKey, setShowKey] = useState(false);

  useEffect(() => {
    if (isOpen) {
      setName(editing?.name ?? "");
      setBaseUrl(editing?.baseUrl ?? "");
      setApiKey(editing?.apiKey ?? "");
      setApiFormat(editing?.apiFormat ?? "anthropic");
      setNotes(editing?.notes ?? "");
      setShowKey(false);
    }
  }, [isOpen, editing]);

  const handleSave = useCallback(() => {
    if (!name.trim()) {
      toast.error(t("endpoints.nameRequired", { defaultValue: "请填写名称" }));
      return;
    }
    if (!baseUrl.trim()) {
      toast.error(
        t("endpoints.baseUrlRequired", { defaultValue: "请填写 API 地址" }),
      );
      return;
    }
    onSave({
      id: editing?.id ?? crypto.randomUUID(),
      name: name.trim(),
      category: editing?.category ?? "custom",
      baseUrl: baseUrl.trim().replace(/\/+$/, ""),
      apiKey: apiKey.trim(),
      apiFormat,
      notes: notes.trim() || undefined,
      websiteUrl: editing?.websiteUrl,
      icon: editing?.icon,
      iconColor: editing?.iconColor,
      meta: editing?.meta,
      createdAt: editing?.createdAt ?? Date.now(),
      sortIndex: editing?.sortIndex,
    });
  }, [apiFormat, apiKey, baseUrl, editing, name, notes, onSave, t]);

  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4">
      <div className="w-full max-w-md rounded-xl border bg-bg-1 p-6 shadow-lg">
        <h3 className="mb-4 text-base font-semibold">
          {editing
            ? t("endpoints.editTitle", { defaultValue: "编辑端点" })
            : t("endpoints.addTitle", { defaultValue: "新建端点" })}
        </h3>

        <div className="space-y-4">
          <div className="space-y-1.5">
            <Label>{t("endpoints.name", { defaultValue: "名称" })}</Label>
            <Input
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder={t("endpoints.namePlaceholder", {
                defaultValue: "如：硅基流动",
              })}
            />
          </div>

          <div className="space-y-1.5">
            <Label>
              {t("endpoints.baseUrl", { defaultValue: "API 地址" })}
            </Label>
            <Input
              value={baseUrl}
              onChange={(e) => setBaseUrl(e.target.value)}
              placeholder="https://api.example.com"
            />
          </div>

          <div className="space-y-1.5">
            <Label>{t("endpoints.apiKey", { defaultValue: "API 密钥" })}</Label>
            <div className="relative">
              <Input
                type={showKey ? "text" : "password"}
                value={apiKey}
                onChange={(e) => setApiKey(e.target.value)}
                placeholder="sk-..."
                className="pr-10"
              />
              <button
                type="button"
                className="absolute right-2 top-1/2 -translate-y-1/2 text-fg-3 hover:text-fg-1"
                onClick={() => setShowKey((v) => !v)}
              >
                {showKey ? (
                  <EyeOff className="h-4 w-4" />
                ) : (
                  <Eye className="h-4 w-4" />
                )}
              </button>
            </div>
          </div>

          <div className="space-y-1.5">
            <Label>
              {t("endpoints.apiFormat", { defaultValue: "协议格式" })}
            </Label>
            <Select
              value={apiFormat}
              onValueChange={(v) => setApiFormat(v as EndpointApiFormat)}
            >
              <SelectTrigger>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {API_FORMATS.map((format) => (
                  <SelectItem key={format} value={format}>
                    {FORMAT_LABELS[format]}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <p className="text-xs text-fg-3">
              {t("endpoints.apiFormatHint", {
                defaultValue:
                  "代理按此格式在 Claude / OpenAI / Gemini 协议间转换；大多数中转站选 Anthropic",
              })}
            </p>
          </div>

          <div className="space-y-1.5">
            <Label>{t("endpoints.notes", { defaultValue: "备注" })}</Label>
            <Input
              value={notes}
              onChange={(e) => setNotes(e.target.value)}
              placeholder={t("endpoints.notesPlaceholder", {
                defaultValue: "可选",
              })}
            />
          </div>
        </div>

        <div className="mt-6 flex justify-end gap-2">
          <Button variant="outline" onClick={onClose}>
            {t("common.cancel", { defaultValue: "取消" })}
          </Button>
          <Button onClick={handleSave}>
            {t("common.save", { defaultValue: "保存" })}
          </Button>
        </div>
      </div>
    </div>
  );
}
