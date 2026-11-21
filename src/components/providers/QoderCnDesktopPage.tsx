import { useState, type FormEvent } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Copy, Monitor, Plus } from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { AppPageHeader } from "@/components/shell/AppPageHeader";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  qoderCnDesktopApi,
  type QoderProviderInput,
  type QoderUpstreamFormat,
} from "@/lib/api/qoderCnDesktop";
import { extractErrorMessage } from "@/utils/errorUtils";

const queryKey = ["qoderCnDesktop"] as const;
const formats: Record<QoderUpstreamFormat, string> = {
  openai_responses: "OpenAI Responses",
  openai_chat: "OpenAI Chat Completions",
  anthropic: "Anthropic Messages",
};
const initialInput: QoderProviderInput = {
  name: "",
  baseUrl: "",
  model: "",
  format: "openai_responses",
  anthropicApiKey: false,
  apiKey: "",
};
const selectClass =
  "h-9 min-w-0 rounded-md border border-input bg-background px-3 text-sm";

export function QoderCnDesktopPage() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [editor, setEditor] = useState<QoderProviderInput | null>(null);
  const [sourceId, setSourceId] = useState("");
  const [removeId, setRemoveId] = useState<string | null>(null);
  const { data, error, isPending } = useQuery({
    queryKey,
    queryFn: qoderCnDesktopApi.status,
    refetchInterval: 5000,
  });
  function actionError(error: unknown) {
    const message = extractErrorMessage(error);
    return message.startsWith("qoderCnDesktop.errors.")
      ? t(message)
      : message || t("common.error");
  }

  const action = useMutation({
    mutationFn: (request: () => Promise<void>) => request(),
    onSuccess: () => queryClient.invalidateQueries({ queryKey }),
    onError: (error) => toast.error(actionError(error)),
  });
  const busy = action.isPending;

  function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!editor) return;
    void action
      .mutateAsync(() => qoderCnDesktopApi.save(editor))
      .then(() => setEditor(null))
      .catch(() => undefined);
  }

  function copy(value: string) {
    void navigator.clipboard.writeText(value).then(
      () => toast.success(t("qoderCnDesktop.copied")),
      (error) => toast.error(actionError(error)),
    );
  }

  return (
    <>
      <AppPageHeader
        icon={<Monitor className="h-5 w-5" strokeWidth={1.5} />}
        title={t("qoderCnDesktop.title")}
        actions={
          <Button disabled={busy} onClick={() => setEditor(initialInput)}>
            <Plus className="h-4 w-4" />
            {t("qoderCnDesktop.add")}
          </Button>
        }
      />
      <div className="mx-auto min-h-0 w-full max-w-5xl flex-1 space-y-4 overflow-y-auto px-5 py-4">
        {error && (
          <p role="alert" className="text-destructive">
            {actionError(error)}
          </p>
        )}
        {isPending && <p>{t("common.loading")}</p>}
        {data && (
          <section className="space-y-3 rounded-lg border bg-card p-4">
            <div className="flex flex-wrap items-center justify-between gap-3">
              <div>
                <h2 className="font-semibold">{t("qoderCnDesktop.gateway")}</h2>
                <p className="text-sm text-muted-foreground">
                  {t(
                    data.running
                      ? "qoderCnDesktop.running"
                      : "qoderCnDesktop.stopped",
                  )}
                </p>
              </div>
              <div className="flex gap-2">
                {data.enabled && (
                  <Button
                    variant="outline"
                    disabled={busy}
                    onClick={() =>
                      action.mutate(() => qoderCnDesktopApi.setGateway(false))
                    }
                  >
                    {t("qoderCnDesktop.stop")}
                  </Button>
                )}
                {!data.running && (
                  <Button
                    disabled={busy || !data.currentProviderId}
                    onClick={() =>
                      action.mutate(() => qoderCnDesktopApi.setGateway(true))
                    }
                  >
                    {t("qoderCnDesktop.start")}
                  </Button>
                )}
              </div>
            </div>
            <p className="text-sm text-muted-foreground">
              {t("qoderCnDesktop.setup")}
            </p>
            <dl className="grid gap-2 sm:grid-cols-3">
              {[
                ["Base URL", data.baseUrl, false],
                ["Model ID", data.model, false],
                ["API Key", data.gatewayToken, true],
              ].map(([label, value, secret]) => (
                <div
                  key={String(label)}
                  className="min-w-0 rounded-md bg-muted/40 px-2.5 py-1.5"
                >
                  <dt className="text-xs text-muted-foreground">{label}</dt>
                  <dd className="flex items-center justify-between gap-1">
                    <code className="truncate text-xs">
                      {value ? (secret ? "••••••••" : value) : "—"}
                    </code>
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-7 w-7 shrink-0"
                      disabled={!value}
                      aria-label={t("qoderCnDesktop.copy", { label })}
                      onClick={() => copy(String(value))}
                    >
                      <Copy className="h-3.5 w-3.5" />
                    </Button>
                  </dd>
                </div>
              ))}
            </dl>
          </section>
        )}
        {editor && (
          <form
            className="grid gap-3 rounded-lg border bg-card p-4 sm:grid-cols-2"
            onSubmit={save}
          >
            <label className="space-y-1 text-sm">
              <span>{t("qoderCnDesktop.name")}</span>
              <Input
                required
                value={editor.name}
                onChange={(event) =>
                  setEditor({ ...editor, name: event.target.value })
                }
              />
            </label>
            <label className="space-y-1 text-sm">
              <span>{t("qoderCnDesktop.format")}</span>
              <select
                className={`${selectClass} w-full`}
                value={editor.format}
                onChange={(event) =>
                  setEditor({
                    ...editor,
                    format: event.target.value as QoderUpstreamFormat,
                  })
                }
              >
                {Object.entries(formats).map(([value, label]) => (
                  <option key={value} value={value}>
                    {label}
                  </option>
                ))}
              </select>
            </label>
            <label className="space-y-1 text-sm">
              <span>Base URL</span>
              <Input
                type="url"
                required
                value={editor.baseUrl}
                onChange={(event) =>
                  setEditor({ ...editor, baseUrl: event.target.value })
                }
              />
            </label>
            <label className="space-y-1 text-sm">
              <span>{t("qoderCnDesktop.upstreamModel")}</span>
              <Input
                required
                value={editor.model}
                onChange={(event) =>
                  setEditor({ ...editor, model: event.target.value })
                }
              />
            </label>
            <label className="space-y-1 text-sm">
              <span>
                {editor.id ? t("qoderCnDesktop.replaceKey") : "API Key"}
              </span>
              <Input
                autoComplete="off"
                type="password"
                required={!editor.id}
                value={editor.apiKey}
                onChange={(event) =>
                  setEditor({ ...editor, apiKey: event.target.value })
                }
              />
            </label>
            {editor.format === "anthropic" && (
              <label className="flex items-end gap-2 pb-2 text-sm">
                <input
                  type="checkbox"
                  checked={editor.anthropicApiKey}
                  onChange={(event) =>
                    setEditor({
                      ...editor,
                      anthropicApiKey: event.target.checked,
                    })
                  }
                />
                <span>{t("qoderCnDesktop.xApiKey")}</span>
              </label>
            )}
            <div className="flex gap-2 sm:col-span-2">
              <Button type="submit" disabled={busy}>
                {t("common.save")}
              </Button>
              <Button
                type="button"
                variant="outline"
                disabled={busy}
                onClick={() => setEditor(null)}
              >
                {t("common.cancel")}
              </Button>
            </div>
          </form>
        )}
        {data && (
          <section className="space-y-3">
            <h2 className="font-semibold">{t("qoderCnDesktop.providers")}</h2>
            {data.importableProviders.length > 0 && (
              <div className="flex gap-2">
                <Select value={sourceId} onValueChange={setSourceId}>
                  <SelectTrigger
                    aria-label={t("qoderCnDesktop.source")}
                    className="flex-1"
                  >
                    <SelectValue placeholder={t("qoderCnDesktop.source")} />
                  </SelectTrigger>
                  <SelectContent>
                    {data.importableProviders.map((provider) => (
                      <SelectItem key={provider.id} value={provider.id}>
                        {provider.name} · {provider.model}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                <Button
                  variant="outline"
                  disabled={!sourceId || busy}
                  onClick={() => {
                    void action
                      .mutateAsync(() => qoderCnDesktopApi.import(sourceId))
                      .then(() => setSourceId(""))
                      .catch(() => undefined);
                  }}
                >
                  {t("qoderCnDesktop.import")}
                </Button>
              </div>
            )}
            {data.providers.length === 0 && (
              <p className="rounded-lg border border-dashed p-4 text-sm text-muted-foreground">
                {t("qoderCnDesktop.empty")}
              </p>
            )}
            {data.providers.map((provider) => {
              const active = data.currentProviderId === provider.id;
              const canDelete = !active || !data.enabled;
              return (
                <div
                  key={provider.id}
                  className="flex flex-wrap items-center justify-between gap-3 rounded-lg border bg-card p-3"
                >
                  <div className="min-w-0">
                    <h3 className="truncate font-medium">{provider.name}</h3>
                    <p className="truncate text-sm text-muted-foreground">
                      {provider.model} · {formats[provider.format]}
                    </p>
                  </div>
                  <div className="flex flex-wrap gap-2">
                    <Button
                      size="sm"
                      disabled={active || busy}
                      onClick={() =>
                        action.mutate(() =>
                          qoderCnDesktopApi.select(provider.id),
                        )
                      }
                    >
                      {t(
                        active
                          ? "qoderCnDesktop.active"
                          : "qoderCnDesktop.select",
                      )}
                    </Button>
                    <Button
                      size="sm"
                      variant="outline"
                      disabled={busy}
                      onClick={() => setEditor({ ...provider, apiKey: "" })}
                    >
                      {t("common.edit")}
                    </Button>
                    {removeId === provider.id ? (
                      <>
                        <Button
                          size="sm"
                          variant="destructive"
                          disabled={!canDelete || busy}
                          onClick={() => {
                            void action
                              .mutateAsync(() =>
                                qoderCnDesktopApi.delete(provider.id),
                              )
                              .then(() => setRemoveId(null))
                              .catch(() => undefined);
                          }}
                        >
                          {t("common.confirm")}
                        </Button>
                        <Button
                          size="sm"
                          variant="ghost"
                          onClick={() => setRemoveId(null)}
                        >
                          {t("common.cancel")}
                        </Button>
                      </>
                    ) : (
                      <Button
                        size="sm"
                        variant="ghost"
                        disabled={!canDelete || busy}
                        onClick={() => setRemoveId(provider.id)}
                      >
                        {t("common.delete")}
                      </Button>
                    )}
                  </div>
                </div>
              );
            })}
          </section>
        )}
      </div>
    </>
  );
}
