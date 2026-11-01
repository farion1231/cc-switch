import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { McpApps, McpServer } from "@/types";
import { MCP_APP_IDS, type McpAppId } from "@/config/appConfig";
import { APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import { Button } from "@/components/ui/button";
import {
  Sheet,
  SheetBody,
  SheetFooter,
  SheetPageContent,
} from "@/components/ui/sheet";
import { useUpsertMcpServer } from "@/hooks/useMcp";
import { mcpApi, type ApiPreview } from "@/lib/api/mcp";
import {
  CHECKBOX_CLASS,
  FIELD_CLASS,
  LABEL_CLASS,
  MONO_FIELD_CLASS,
} from "./formBits";

export interface ApiMcpModalProps {
  initialData?: McpServer;
  existingServers: Record<string, McpServer>;
  visibleAppIds: McpAppId[];
  onClose: () => void;
}

export default function ApiMcpModal({
  initialData,
  existingServers,
  visibleAppIds,
  onClose,
}: ApiMcpModalProps) {
  const { t } = useTranslation();
  const upsert = useUpsertMcpServer();
  const [mode, setMode] = useState<"document" | "manual">("document");
  const [sourceUrl, setSourceUrl] = useState("");
  const [documentText, setDocumentText] = useState("");
  const [baseUrl, setBaseUrl] = useState("");
  const [toolName, setToolName] = useState("");
  const [endpoint, setEndpoint] = useState("");
  const [method, setMethod] = useState("GET");
  const [readOnly, setReadOnly] = useState(true);
  const [description, setDescription] = useState("");
  const [parameters, setParameters] = useState("[]");
  const [bodySchema, setBodySchema] = useState("");
  const [id, setId] = useState(initialData?.id ?? "");
  const [headers, setHeaders] = useState(
    initialData?.server.env?.CC_SWITCH_MCP_API_HEADERS ?? "{}",
  );
  const [showHeaders, setShowHeaders] = useState(false);
  const [preview, setPreview] = useState<ApiPreview | null>(null);
  const [selected, setSelected] = useState<string[]>([]);
  const [apps, setApps] = useState<McpApps>(
    () =>
      ({
        ...Object.fromEntries(
          MCP_APP_IDS.map((app) => [
            app,
            Boolean(
              initialData ? initialData.apps[app] : visibleAppIds.includes(app),
            ),
          ]),
        ),
        openclaw: false,
      }) as McpApps,
  );
  const [busy, setBusy] = useState(false);
  const busyRef = useRef(false);
  const [error, setError] = useState("");
  const errorRef = useRef<HTMLParagraphElement>(null);
  const revision = useRef(0);

  useEffect(() => {
    if (error) errorRef.current?.focus();
  }, [error]);

  useEffect(() => {
    if (!initialData) return;
    let active = true;
    busyRef.current = true;
    setBusy(true);
    mcpApi
      .decodeApiSpec(initialData.server)
      .then((result) => {
        if (active) {
          setPreview(result);
          setSelected(result.tools.map((tool) => tool.name));
        }
      })
      .catch(() => {
        if (active) setError(t("mcpApi.loadFailed"));
      })
      .finally(() => {
        if (active) {
          busyRef.current = false;
          setBusy(false);
        }
      });
    return () => {
      active = false;
    };
  }, [initialData, t]);

  const changeSource = (update: () => void) => {
    revision.current += 1;
    update();
    setPreview(null);
    setSelected([]);
    setError("");
  };
  const parseJson = (text: string) => {
    try {
      return JSON.parse(text);
    } catch {
      throw new Error(t("mcpApi.invalidJson"));
    }
  };
  const loadPreview = async () => {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    setError("");
    setPreview(null);
    const currentRevision = revision.current;
    try {
      let request: { url?: string; document?: string; baseUrl?: string };
      if (mode === "manual") {
        let url: URL;
        try {
          url = new URL(endpoint);
          if (
            !["http:", "https:"].includes(url.protocol) ||
            url.username ||
            url.password ||
            url.hash
          )
            throw new Error();
        } catch {
          throw new Error(t("mcpApi.invalidUrl"));
        }
        const params = parseJson(parameters);
        if (
          !Array.isArray(params) ||
          !toolName.trim() ||
          params.some(
            (p) =>
              !p ||
              typeof p !== "object" ||
              typeof p.in !== "string" ||
              typeof p.name !== "string",
          )
        )
          throw new Error(t("mcpApi.invalidManual"));
        const path = url.pathname.replace(/%7B/gi, "{").replace(/%7D/gi, "}");
        const names = new Set(
          params.map(
            (p: { in?: string; name?: string }) => `${p.in}:${p.name}`,
          ),
        );
        for (const match of path.matchAll(/\{([^}]+)\}/g)) {
          if (!names.has(`path:${match[1]}`))
            params.push({
              in: "path",
              name: match[1],
              required: true,
              schema: { type: "string" },
            });
        }
        const query = new Map<string, string[]>();
        for (const [key, value] of url.searchParams)
          query.set(key, [...(query.get(key) ?? []), value]);
        for (const [key, values] of query) {
          if (!names.has(`query:${key}`))
            params.push({
              in: "query",
              name: key,
              schema:
                values.length > 1
                  ? {
                      type: "array",
                      items: { type: "string" },
                      default: values,
                    }
                  : { type: "string", default: values[0] },
            });
        }
        const operation = {
          operationId: toolName.trim(),
          summary: description.trim() || toolName.trim(),
          parameters: params,
          "x-mcp-read-only": readOnly,
          ...(bodySchema.trim()
            ? {
                requestBody: {
                  required: true,
                  content: {
                    "application/json": { schema: parseJson(bodySchema) },
                  },
                },
              }
            : {}),
        };
        request = {
          document: JSON.stringify({
            openapi: "3.1.0",
            info: { title: toolName.trim() },
            servers: [{ url: url.origin }],
            paths: { [path]: { [method.toLowerCase()]: operation } },
          }),
        };
      } else {
        request = {
          ...(documentText.trim()
            ? { document: documentText }
            : { url: sourceUrl.trim() }),
          ...(baseUrl.trim() ? { baseUrl: baseUrl.trim() } : {}),
        };
      }
      const result = await mcpApi.previewApi(request);
      if (currentRevision !== revision.current) return;
      setPreview(result);
      setSelected(
        result.tools
          .filter((tool) => ["GET", "HEAD"].includes(tool.method))
          .map((tool) => tool.name),
      );
      if (!id)
        setId(
          result.title
            .toLowerCase()
            .replace(/[^a-z0-9_.-]+/g, "-")
            .replace(/^-|-$/g, "") || "api",
        );
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      errorRef.current?.focus();
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  };
  const save = async () => {
    if (busyRef.current || !preview) return;
    if (
      !initialData &&
      (!/^[a-zA-Z0-9_.-]+$/.test(id) || existingServers[id])
    ) {
      setError(t("mcpApi.invalidId"));
      errorRef.current?.focus();
      return;
    }
    busyRef.current = true;
    setBusy(true);
    setError("");
    try {
      const parsed = parseJson(headers);
      if (
        !parsed ||
        typeof parsed !== "object" ||
        Array.isArray(parsed) ||
        Object.values(parsed).some((value) => typeof value !== "string")
      )
        throw new Error(t("mcpApi.invalidHeaders"));
      const tools = preview.tools.filter((tool) =>
        selected.includes(tool.name),
      );
      const spec = await mcpApi.buildApiSpec({ tools }, parsed);
      await upsert.mutateAsync({
        ...initialData,
        id,
        name: initialData?.name ?? preview.title,
        server: spec,
        apps,
        tags: initialData?.tags ?? ["api"],
      });
      onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      errorRef.current?.focus();
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  };
  const field = (
    label: string,
    value: string,
    onChange: (value: string) => void,
    options: { multiline?: boolean; secret?: boolean; disabled?: boolean } = {},
  ) => {
    const props = {
      id: `api-${label}`,
      value,
      disabled: busy || options.disabled,
      onChange: (
        event: React.ChangeEvent<HTMLInputElement | HTMLTextAreaElement>,
      ) => onChange(event.target.value),
      className: options.multiline
        ? `${MONO_FIELD_CLASS} min-h-24 py-2`
        : FIELD_CLASS,
    };
    return (
      <div className="flex flex-col gap-2">
        <label className={LABEL_CLASS} htmlFor={props.id}>
          {t(`mcpApi.${label}`)}
        </label>
        {options.multiline ? (
          <textarea {...props} rows={4} />
        ) : (
          <input
            {...props}
            type={options.secret ? "password" : "text"}
            autoComplete="off"
          />
        )}
      </div>
    );
  };
  return (
    <Sheet
      open
      modal={false}
      onOpenChange={(open) => {
        if (!open && !busyRef.current) onClose();
      }}
    >
      <SheetPageContent
        title={t("mcpApi.title")}
        closeLabel={t("common.back")}
        aria-describedby={undefined}
      >
        <SheetBody className="flex flex-col gap-5 px-6 pb-6 pt-5">
          <p className="m-0 text-body text-fg-2">{t("mcpApi.intro")}</p>
          <details open={!initialData}>
            <summary className="cursor-pointer text-body font-medium">
              {t("mcpApi.source")}
            </summary>
            <div className="mt-4 flex flex-col gap-5">
              <div className="flex gap-2">
                {(["document", "manual"] as const).map((value) => (
                  <Button
                    key={value}
                    variant={mode === value ? "neutral" : "quiet"}
                    disabled={busy}
                    onClick={() => changeSource(() => setMode(value))}
                  >
                    {t(`mcpApi.${value}`)}
                  </Button>
                ))}
              </div>
              {mode === "document" ? (
                <>
                  {field("documentUrl", sourceUrl, (value) =>
                    changeSource(() => setSourceUrl(value)),
                  )}
                  {field(
                    "documentText",
                    documentText,
                    (value) => changeSource(() => setDocumentText(value)),
                    { multiline: true },
                  )}
                  {field("baseUrl", baseUrl, (value) =>
                    changeSource(() => setBaseUrl(value)),
                  )}
                </>
              ) : (
                <>
                  {field("toolName", toolName, (value) =>
                    changeSource(() => setToolName(value)),
                  )}
                  {field("endpoint", endpoint, (value) =>
                    changeSource(() => setEndpoint(value)),
                  )}
                  <div className="flex flex-col gap-2">
                    <label className={LABEL_CLASS} htmlFor="api-method">
                      {t("mcpApi.method")}
                    </label>
                    <select
                      id="api-method"
                      className={FIELD_CLASS}
                      value={method}
                      disabled={busy}
                      onChange={(event) =>
                        changeSource(() => {
                          setMethod(event.target.value);
                          setReadOnly(
                            ["GET", "HEAD"].includes(event.target.value),
                          );
                        })
                      }
                    >
                      {["GET", "HEAD", "POST", "PUT", "PATCH", "DELETE"].map(
                        (value) => (
                          <option key={value}>{value}</option>
                        ),
                      )}
                    </select>
                  </div>
                  <label className="flex items-center gap-2 text-body">
                    <input
                      type="checkbox"
                      className={CHECKBOX_CLASS}
                      checked={readOnly}
                      disabled={busy}
                      onChange={(event) =>
                        changeSource(() => setReadOnly(event.target.checked))
                      }
                    />
                    {t("mcpApi.readOnly")}
                  </label>
                  {field("description", description, (value) =>
                    changeSource(() => setDescription(value)),
                  )}
                  {field(
                    "parameters",
                    parameters,
                    (value) => changeSource(() => setParameters(value)),
                    { multiline: true },
                  )}
                  <p className="m-0 text-caption text-fg-2">
                    {t("mcpApi.parametersHint")}
                  </p>
                  {field(
                    "bodySchema",
                    bodySchema,
                    (value) => changeSource(() => setBodySchema(value)),
                    { multiline: true },
                  )}
                </>
              )}
              <div>
                <Button
                  variant="neutral"
                  disabled={busy}
                  onClick={() => void loadPreview()}
                >
                  {busy ? t("common.loading") : t("mcpApi.preview")}
                </Button>
              </div>
            </div>
          </details>
          {error && (
            <p
              ref={errorRef}
              role="alert"
              tabIndex={-1}
              className="text-body text-danger-text [overflow-wrap:anywhere]"
            >
              {error}
            </p>
          )}
          {preview && (
            <>
              <div className="flex flex-col gap-3">
                <h3 className="m-0 text-body font-medium">
                  {t("mcpApi.tools", { count: preview.tools.length })}
                </h3>
                <p className="m-0 text-caption text-fg-2">
                  {t("mcpApi.selectionHint")}
                </p>
                {preview.tools.map((tool) => (
                  <label
                    key={tool.name}
                    className="flex items-start gap-2 rounded-control border border-border p-3"
                  >
                    <input
                      type="checkbox"
                      className={CHECKBOX_CLASS}
                      aria-label={tool.name}
                      disabled={busy}
                      checked={selected.includes(tool.name)}
                      onChange={(event) =>
                        setSelected((old) =>
                          event.target.checked
                            ? [...old, tool.name]
                            : old.filter((name) => name !== tool.name),
                        )
                      }
                    />
                    <span className="min-w-0">
                      <span className="block text-body font-medium">
                        {tool.name}
                      </span>
                      <span className="block font-mono text-caption text-fg-2 [overflow-wrap:anywhere]">
                        {tool.method} {tool.url}
                      </span>
                      <span className="block text-caption text-fg-2">
                        {tool.description}
                      </span>
                      <span className="block text-caption text-fg-2">
                        {t(
                          (tool.readOnly ??
                            ["GET", "HEAD", "OPTIONS"].includes(tool.method))
                            ? "mcpApi.readOnly"
                            : "mcpApi.mayWrite",
                        )}
                      </span>
                    </span>
                  </label>
                ))}
              </div>
              {preview.skipped.length > 0 && (
                <details>
                  <summary>
                    {t("mcpApi.skipped", { count: preview.skipped.length })}
                  </summary>
                  <ul className="text-caption text-fg-2">
                    {preview.skipped.map((reason, index) => (
                      <li key={index}>{reason}</li>
                    ))}
                  </ul>
                </details>
              )}
              {field("serverId", id, setId, { disabled: Boolean(initialData) })}
              {field("headers", headers, setHeaders, {
                multiline: showHeaders,
                secret: !showHeaders,
              })}
              <div>
                <Button
                  variant="quiet"
                  onClick={() => setShowHeaders((value) => !value)}
                >
                  {t(showHeaders ? "mcpApi.hideHeaders" : "mcpApi.showHeaders")}
                </Button>
              </div>
              <p className="m-0 text-caption text-fg-2">
                {t("mcpApi.credentialsHint")}
              </p>
              <fieldset className="flex flex-wrap gap-4">
                <legend className={`${LABEL_CLASS} mb-2`}>
                  {t("mcpApi.apps")}
                </legend>
                {visibleAppIds.map((app) => (
                  <label
                    key={app}
                    className="flex items-center gap-2 text-body"
                  >
                    <input
                      type="checkbox"
                      className={CHECKBOX_CLASS}
                      checked={Boolean(apps[app])}
                      disabled={busy}
                      onChange={(event) =>
                        setApps((old) => ({
                          ...old,
                          [app]: event.target.checked,
                        }))
                      }
                    />
                    {APP_DISPLAY_NAME[app]}
                  </label>
                ))}
              </fieldset>
            </>
          )}
        </SheetBody>
        <SheetFooter className="h-14 px-6 py-0">
          <Button variant="neutral" disabled={busy} onClick={onClose}>
            {t("common.cancel")}
          </Button>
          <Button
            variant="solid"
            disabled={busy || !preview || selected.length === 0}
            onClick={() => void save()}
          >
            {t(busy ? "common.saving" : "common.save")}
          </Button>
        </SheetFooter>
      </SheetPageContent>
    </Sheet>
  );
}
