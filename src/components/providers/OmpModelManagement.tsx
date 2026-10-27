import { useEffect, useMemo, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Notice } from "@/components/ui/notice";
import {
  ompApi,
  OMP_THINKING_LEVELS,
  type OmpModelManagementState,
  type OmpCatalogModel,
  type OmpSettingsEntryPatch,
  type OmpModelParameterPatch,
  type OmpThinkingLevel,
} from "@/lib/api/omp";
import {
  extractErrorMessage,
  translatePiProviderMutationError,
} from "@/utils/errorUtils";
import { invalidatePiProviderCaches } from "@/lib/query/pi";
import { summarizeOmpReferences } from "./ompModelReferences";

export const ompManagementKey = ["omp", "modelManagement"];
const equal = (a: unknown, b: unknown) =>
  JSON.stringify(a) === JSON.stringify(b);
const selectorClass =
  "h-9 rounded-md border border-input bg-background px-3 text-body focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring";
const refValid = (value: string) =>
  value.length > 0 &&
  value.trim().length > 0 &&
  !/[\u0000-\u001f\u007f]/.test(value);

export function OmpModelManagement({
  initialProvider,
  onClose,
}: {
  initialProvider?: string;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const client = useQueryClient();
  const query = useQuery({
    queryKey: ompManagementKey,
    queryFn: ompApi.getState,
    refetchOnWindowFocus: false,
    retry: false,
  });
  const [settingsDirty, setSettingsDirty] = useState(false);
  const [parametersDirty, setParametersDirty] = useState(false);
  const [busy, setBusy] = useState(false);
  const [pending, setPending] = useState<(() => void) | null>(null);
  const [generation, setGeneration] = useState(0);
  const guard = (
    action: () => void,
    dirty = settingsDirty || parametersDirty,
  ) => {
    if (busy) return;
    if (dirty) setPending(() => action);
    else action();
  };
  const updated = (state: OmpModelManagementState) => {
    client.setQueryData(ompManagementKey, state);
    void invalidatePiProviderCaches(client, "omp");
  };
  return (
    <FullScreenPanel
      isOpen
      title={t("omp.management.title")}
      subtitle="Oh My Pi"
      onClose={() => guard(onClose)}
      trackUnsavedChanges={settingsDirty || parametersDirty}
    >
      <Notice tone="neutral" title={t("omp.management.scope")} />
      <div className="flex flex-wrap items-center justify-between gap-2">
        <code className="break-all text-caption text-fg-2">
          {query.data?.settingsPath}
        </code>
        <Button
          variant="outline"
          disabled={busy || query.isFetching}
          onClick={() =>
            guard(() => {
              setBusy(true);
              void query
                .refetch()
                .then((result) => {
                  if (result.isSuccess) {
                    setGeneration((v) => v + 1);
                    setSettingsDirty(false);
                    setParametersDirty(false);
                  }
                })
                .finally(() => setBusy(false));
            })
          }
        >
          {t("omp.management.reload")}
        </Button>
      </div>
      {query.isPending && <p role="status">{t("common.loading")}</p>}
      {query.error && (
        <p role="alert" className="text-danger-text">
          {translatePiProviderMutationError(
            extractErrorMessage(query.error),
            t,
          )}
        </p>
      )}
      {query.data && (
        <ManagementBody
          key={generation}
          state={query.data}
          initialProvider={initialProvider}
          updated={updated}
          settingsDirty={setSettingsDirty}
          parametersDirty={setParametersDirty}
          setBusy={setBusy}
          busy={busy}
          guard={guard}
        />
      )}
      <ConfirmDialog
        isOpen={pending !== null}
        title={t("common.unsavedLeaveTitle")}
        message={t("common.unsavedLeaveMessage")}
        confirmText={t("common.unsavedLeaveConfirm")}
        cancelText={t("common.unsavedLeaveCancel")}
        zIndex="top"
        onConfirm={() => {
          const action = pending;
          setPending(null);
          action?.();
        }}
        onCancel={() => setPending(null)}
      />
    </FullScreenPanel>
  );
}

function ManagementBody({
  state,
  initialProvider,
  updated,
  settingsDirty,
  parametersDirty,
  busy,
  setBusy,
  guard,
}: {
  state: OmpModelManagementState;
  initialProvider?: string;
  updated: (state: OmpModelManagementState) => void;
  settingsDirty: (v: boolean) => void;
  parametersDirty: (v: boolean) => void;
  busy: boolean;
  setBusy: (v: boolean) => void;
  guard: (action: () => void, dirty?: boolean) => void;
}) {
  const { t } = useTranslation();
  const [filter, setFilter] = useState(initialProvider ?? "");
  const [selected, setSelected] = useState<Pick<
    OmpCatalogModel,
    "provider" | "id"
  > | null>(
    () =>
      state.models.find((model) => model.provider === initialProvider) ??
      (() => {
        const override = state.modelOverrides.find(
          (entry) => entry.provider === initialProvider,
        );
        return override
          ? { provider: override.provider, id: override.modelId }
          : null;
      })(),
  );
  const [parameterDirty, setParameterDirty] = useState(false);
  const [visibleCount, setVisibleCount] = useState(50);
  const models = state.models.filter((m) =>
    `${m.provider}/${m.id} ${m.name}`
      .toLowerCase()
      .includes(filter.toLowerCase()),
  );
  const uncatalogedOverrides = state.modelOverrides.filter(
    (override) =>
      !state.models.some(
        (model) =>
          model.provider === override.provider && model.id === override.modelId,
      ) &&
      `${override.provider}/${override.modelId}`
        .toLowerCase()
        .includes(filter.toLowerCase()),
  );
  const references = useMemo(() => summarizeOmpReferences(state), [state]);
  return (
    <>
      <SettingsEditor
        state={state}
        updated={updated}
        dirtyChanged={settingsDirty}
        busy={busy}
        setBusy={setBusy}
        guard={guard}
      />
      <section className="space-y-3" aria-label={t("omp.management.catalog")}>
        <h2 className="text-section">{t("omp.management.catalog")}</h2>
        <p className="text-caption text-fg-2">
          {t("omp.management.cacheHint")}
        </p>
        {references.unresolved.length > 0 && (
          <details>
            <summary>{t("omp.management.unresolved")}</summary>
            <ul className="space-y-1 break-all font-mono text-caption">
              {references.unresolved.map((value) => (
                <li key={value}>{value}</li>
              ))}
            </ul>
          </details>
        )}
        {state.catalogStatus !== "ready" && (
          <p role="status" className="text-warning-text">
            {t(`omp.management.catalogStatus.${state.catalogStatus}`)}{" "}
            {state.catalogMessage && t(state.catalogMessage)}
          </p>
        )}
        <label className="block space-y-1">
          <span>{t("omp.management.search")}</span>
          <Input
            value={filter}
            onChange={(e) => {
              setFilter(e.target.value);
              setVisibleCount(50);
            }}
          />
        </label>
        <div className="grid gap-4 lg:grid-cols-[minmax(220px,1fr)_minmax(320px,2fr)]">
          <div className="space-y-2">
            {models.length === 0 && <p>{t("omp.management.emptyCatalog")}</p>}
            {models.slice(0, visibleCount).map((model) => (
              <div
                key={`${model.provider}/${model.id}`}
                className="rounded-panel border border-border p-3"
              >
                <Button
                  variant="quiet"
                  className="h-auto max-w-full whitespace-normal text-start"
                  disabled={busy}
                  onClick={() =>
                    guard(() => {
                      setSelected(model);
                      setParameterDirty(false);
                      parametersDirty(false);
                    }, parameterDirty)
                  }
                >
                  {model.name}
                </Button>
                <p className="break-all font-mono text-caption">
                  {model.provider}/{model.id}
                </p>
                <p className="text-caption text-fg-2">
                  {t(`omp.management.source.${model.source}`)}
                  {model.cacheUpdatedAt !== null
                    ? ` · ${new Date(model.cacheUpdatedAt).toLocaleString()}`
                    : ""}
                </p>
                <p className="break-all text-caption text-fg-2">{model.kind}</p>
                <p className="break-all text-caption text-fg-2">
                  {(
                    references.byModel.get(`${model.provider}/${model.id}`) ??
                    []
                  ).join(" · ")}
                </p>
              </div>
            ))}
            {models.length > visibleCount && (
              <Button
                variant="outline"
                onClick={() => setVisibleCount((v) => v + 50)}
              >
                {t("omp.management.showMore")}
              </Button>
            )}
            {uncatalogedOverrides.length > 0 && (
              <section
                className="space-y-2"
                aria-label={t("omp.management.explicitRecords")}
              >
                <h3 className="text-strong">
                  {t("omp.management.explicitRecords")}
                </h3>
                <p className="text-caption text-fg-2">
                  {t("omp.management.noCatalogData")}
                </p>
                {uncatalogedOverrides.slice(0, visibleCount).map((override) => (
                  <Button
                    key={`${override.provider}/${override.modelId}`}
                    variant="outline"
                    disabled={busy}
                    onClick={() =>
                      guard(() => {
                        setSelected({
                          provider: override.provider,
                          id: override.modelId,
                        });
                        setParameterDirty(false);
                        parametersDirty(false);
                      }, parameterDirty)
                    }
                  >
                    {override.provider}/{override.modelId}
                  </Button>
                ))}
                {uncatalogedOverrides.length > visibleCount && (
                  <Button
                    variant="outline"
                    onClick={() => setVisibleCount((v) => v + 50)}
                  >
                    {t("omp.management.showMore")}
                  </Button>
                )}
              </section>
            )}
          </div>
          {selected ? (
            <ParameterEditor
              key={`${selected.provider}/${selected.id}`}
              state={state}
              model={selected}
              catalog={
                state.models.find(
                  (model) =>
                    model.provider === selected.provider &&
                    model.id === selected.id,
                ) ?? null
              }
              updated={updated}
              dirtyChanged={(v) => {
                setParameterDirty(v);
                parametersDirty(v);
              }}
              busy={busy}
              setBusy={setBusy}
              guard={guard}
            />
          ) : (
            <p className="text-fg-2">{t("omp.management.selectModel")}</p>
          )}
        </div>
      </section>
    </>
  );
}

function SettingsEditor({
  state,
  updated,
  dirtyChanged,
  busy,
  setBusy,
  guard,
}: {
  state: OmpModelManagementState;
  updated: (s: OmpModelManagementState) => void;
  dirtyChanged: (v: boolean) => void;
  busy: boolean;
  setBusy: (v: boolean) => void;
  guard: (action: () => void, dirty?: boolean) => void;
}) {
  const { t } = useTranslation();
  const [base, setBase] = useState(state);
  const [roles, setRoles] = useState<Record<string, string | string[]>>({
    ...state.modelRoles,
  });
  const [workers, setWorkers] = useState<Record<string, string | string[]>>({
    ...state.agentModelOverrides,
  });
  const [thinking, setThinking] = useState<OmpThinkingLevel | "">(
    state.defaultThinkingLevel ?? "",
  );
  const [reference, setReference] = useState(state.defaultModel ?? "");
  const [modelChoice, setModelChoice] = useState("");
  const [modelThinking, setModelThinking] = useState("");
  const [newRole, setNewRole] = useState("");
  const [newWorker, setNewWorker] = useState("");
  const [error, setError] = useState("");
  const changes: OmpSettingsEntryPatch[] = [];
  for (const [section, before, after] of [
    ["modelRoles", base.modelRoles, roles],
    ["agentModelOverrides", base.agentModelOverrides, workers],
  ] as const) {
    for (const key of new Set([
      ...Object.keys(before),
      ...Object.keys(after),
    ])) {
      const expected = before[key] ?? null;
      const raw = after[key];
      const value = raw === "" || raw === undefined ? null : raw;
      if (!equal(expected, value))
        changes.push({ section, key, expected, value });
    }
  }
  if ((thinking || null) !== base.defaultThinkingLevel)
    changes.push({
      section: "defaultThinkingLevel",
      key: "defaultThinkingLevel",
      expected: base.defaultThinkingLevel,
      value: thinking || null,
    });
  const invalid = changes.some(
    (c) =>
      c.value !== null &&
      (Array.isArray(c.value)
        ? c.value.length === 0 || c.value.some((v) => !refValid(v))
        : !refValid(c.value)),
  );
  const dirty = changes.length > 0;
  useEffect(() => dirtyChanged(dirty), [dirty, dirtyChanged]);
  const cancel = () => {
    setRoles({ ...base.modelRoles });
    setWorkers({ ...base.agentModelOverrides });
    setThinking(base.defaultThinkingLevel ?? "");
    setReference(base.defaultModel ?? "");
    setError("");
  };
  const save = async () => {
    setBusy(true);
    setError("");
    try {
      const next = await ompApi.patchSettings({
        expectedRevision: base.settingsRevision,
        expectedPath: base.settingsPath,
        changes,
      });
      setBase(next);
      setRoles({ ...next.modelRoles });
      setWorkers({ ...next.agentModelOverrides });
      setThinking(next.defaultThinkingLevel ?? "");
      setReference(next.defaultModel ?? "");
      updated(next);
    } catch (e) {
      setError(extractErrorMessage(e));
    } finally {
      setBusy(false);
    }
  };
  const chosen = state.models.find(
    (m) => `${m.provider}/${m.id}` === modelChoice,
  );
  return (
    <section
      className="space-y-4 rounded-panel border border-border p-4"
      aria-label={t("omp.management.settings")}
    >
      <datalist id="omp-reference-options">
        {state.models.map((model) => (
          <option
            key={`${model.provider}/${model.id}`}
            value={`${model.provider}/${model.id}`}
          >
            {model.name}
          </option>
        ))}
        {[...new Set([...state.roleNames, ...Object.keys(roles)])].map(
          (role) => (
            <option key={`@${role}`} value={`@${role}`} />
          ),
        )}
      </datalist>
      <h2 className="text-section">{t("omp.management.settings")}</h2>
      <p className="break-all text-caption text-fg-2">
        {t("omp.management.configuredDefault")}:{" "}
        <code>
          {Array.isArray(roles.default)
            ? roles.default.join(", ")
            : roles.default || t("omp.management.inherit")}
        </code>
      </p>
      <div className="flex flex-wrap gap-2">
        <label className="flex min-w-52 flex-1 flex-col gap-1">
          <span>{t("omp.management.modelChoice")}</span>
          <select
            className={selectorClass}
            value={modelChoice}
            disabled={busy}
            onChange={(e) => {
              setModelChoice(e.target.value);
              setReference(e.target.value);
              setModelThinking("");
            }}
          >
            <option value="">{t("omp.management.selectModel")}</option>
            {state.models.map((m) => (
              <option
                key={`${m.provider}/${m.id}`}
                value={`${m.provider}/${m.id}`}
              >
                {m.provider}/{m.id} — {m.name}
              </option>
            ))}
          </select>
        </label>
        <label className="flex flex-col gap-1">
          <span>{t("omp.management.modelThinking")}</span>
          <select
            className={selectorClass}
            value={modelThinking}
            disabled={busy || !modelChoice}
            onChange={(e) => {
              setModelThinking(e.target.value);
              setReference(
                `${modelChoice}${e.target.value ? `:${e.target.value}` : ""}`,
              );
            }}
          >
            <option value="">{t("omp.management.inherit")}</option>
            {[
              "off",
              "inherit",
              "auto",
              ...(chosen?.thinking?.efforts ?? []),
            ].map((level) => (
              <option key={level} value={level}>
                {level}
              </option>
            ))}
          </select>
        </label>
      </div>
      <label className="block space-y-1">
        <span>{t("omp.management.defaultReference")}</span>
        <Input
          value={reference}
          disabled={busy}
          onChange={(e) => setReference(e.target.value)}
        />
      </label>
      <Button
        variant="outline"
        disabled={busy || !refValid(reference)}
        onClick={() => setRoles((v) => ({ ...v, default: reference }))}
      >
        {t("omp.management.setDefault")}
      </Button>
      <label className="flex flex-col gap-1">
        <span>{t("omp.management.defaultThinking")}</span>
        <select
          className={selectorClass}
          value={thinking}
          disabled={busy}
          onChange={(e) =>
            setThinking(e.target.value as Exclude<OmpThinkingLevel, "off"> | "")
          }
        >
          <option value="">{t("omp.management.inherit")}</option>
          {OMP_THINKING_LEVELS.filter((level) => level !== "off").map(
            (level) => (
              <option key={level} value={level}>
                {level}
              </option>
            ),
          )}
        </select>
      </label>
      <p className="text-caption text-fg-2">
        {t("omp.management.referenceHint")}
      </p>
      <details className="space-y-3">
        <summary
          role="button"
          aria-label={t("omp.management.assignments")}
          className="cursor-pointer text-strong focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
        >
          {t("omp.management.roles")} / {t("omp.management.workers")}
        </summary>
        <fieldset disabled={busy} className="space-y-2">
          <legend className="mb-2 text-strong">
            {t("omp.management.roles")}
          </legend>
          {[...new Set([...state.roleNames, ...Object.keys(roles)])].map(
            (key) => (
              <ReferenceRow
                key={key}
                name={key}
                value={roles[key] ?? ""}
                onChange={(value) => setRoles((v) => ({ ...v, [key]: value }))}
                onClear={() => setRoles((v) => ({ ...v, [key]: "" }))}
              />
            ),
          )}
          <div className="flex gap-2">
            <Input
              aria-label={t("omp.management.newRole")}
              value={newRole}
              onChange={(e) => setNewRole(e.target.value)}
            />
            <Button
              variant="outline"
              disabled={
                !refValid(newRole) ||
                Object.prototype.hasOwnProperty.call(roles, newRole)
              }
              onClick={() => {
                setRoles((v) => ({ ...v, [newRole]: "" }));
                setNewRole("");
              }}
            >
              {t("omp.management.addRole")}
            </Button>
          </div>
        </fieldset>
        <fieldset disabled={busy} className="space-y-2">
          <legend className="mb-2 text-strong">
            {t("omp.management.workers")}
          </legend>
          {Object.entries(workers).map(([key, value]) => (
            <ReferenceRow
              key={key}
              name={key}
              value={value}
              onChange={(next) => setWorkers((v) => ({ ...v, [key]: next }))}
              onClear={() =>
                setWorkers((v) => {
                  const next = { ...v };
                  delete next[key];
                  return next;
                })
              }
            />
          ))}
          <div className="flex gap-2">
            <Input
              aria-label={t("omp.management.newWorker")}
              value={newWorker}
              onChange={(e) => setNewWorker(e.target.value)}
            />
            <Button
              variant="outline"
              disabled={
                !refValid(newWorker) ||
                Object.prototype.hasOwnProperty.call(workers, newWorker)
              }
              onClick={() => {
                setWorkers((v) => ({ ...v, [newWorker]: "" }));
                setNewWorker("");
              }}
            >
              {t("omp.management.addWorker")}
            </Button>
          </div>
        </fieldset>
      </details>
      {invalid && <p role="alert">{t("omp.management.invalidReference")}</p>}
      {error && (
        <p role="alert" className="text-danger-text">
          {translatePiProviderMutationError(error, t)}
        </p>
      )}
      <div className="flex justify-end gap-2">
        <Button
          variant="neutral"
          disabled={busy || !dirty}
          onClick={() => guard(cancel, dirty)}
        >
          {t("common.cancel")}
        </Button>
        <Button
          variant="solid"
          disabled={
            busy ||
            !dirty ||
            invalid ||
            error.includes("omp.errors.externalChange")
          }
          onClick={() => void save()}
        >
          {t("omp.management.saveSettings")}
        </Button>
      </div>
    </section>
  );
}

const parameterFields = [
  "name",
  "contextWindow",
  "maxContextWindow",
  "maxTokens",
  "input",
  "reasoning",
] as const;
function ParameterEditor({
  state,
  model,
  catalog,
  updated,
  dirtyChanged,
  busy,
  setBusy,
  guard,
}: {
  state: OmpModelManagementState;
  model: Pick<OmpCatalogModel, "provider" | "id">;
  catalog: OmpCatalogModel | null;
  updated: (s: OmpModelManagementState) => void;
  dirtyChanged: (v: boolean) => void;
  busy: boolean;
  setBusy: (v: boolean) => void;
  guard: (action: () => void, dirty?: boolean) => void;
}) {
  const { t } = useTranslation();
  const getParameters = (s: OmpModelManagementState) =>
    s.modelOverrides.find(
      (o) => o.provider === model.provider && o.modelId === model.id,
    )?.parameters ?? {};
  const [base, setBase] = useState({
    revision: state.modelsRevision,
    path: state.modelsPath,
    parameters: getParameters(state),
  });
  const [draft, setDraft] = useState<Record<string, unknown>>({
    ...base.parameters,
  });
  const [error, setError] = useState("");
  const changes: OmpModelParameterPatch[] = parameterFields.flatMap((field) =>
    equal(base.parameters[field] ?? null, draft[field] ?? null)
      ? []
      : [
          {
            field,
            expected: base.parameters[field] ?? null,
            value: draft[field] ?? null,
          },
        ],
  );
  const numericInvalid = [
    "contextWindow",
    "maxContextWindow",
    "maxTokens",
  ].some(
    (field) =>
      draft[field] !== undefined &&
      (!Number.isSafeInteger(Number(draft[field])) ||
        Number(draft[field]) <= 0),
  );
  const invalid =
    numericInvalid ||
    (draft.name !== undefined &&
      (typeof draft.name !== "string" || !refValid(draft.name))) ||
    (draft.input !== undefined &&
      (!Array.isArray(draft.input) ||
        draft.input.some((value) => value !== "text" && value !== "image"))) ||
    (draft.maxContextWindow !== undefined &&
      Number(draft.maxContextWindow) <
        Number(draft.contextWindow ?? catalog?.contextWindow ?? 0));
  const dirty = changes.length > 0;
  useEffect(() => dirtyChanged(dirty), [dirty, dirtyChanged]);
  const save = async () => {
    setBusy(true);
    setError("");
    try {
      const normalized = changes.map((c) => ({
        ...c,
        value:
          c.value !== null &&
          ["contextWindow", "maxContextWindow", "maxTokens"].includes(c.field)
            ? Number(c.value)
            : c.value,
      }));
      const next = await ompApi.patchOverride({
        expectedRevision: base.revision,
        expectedPath: base.path,
        provider: model.provider,
        modelId: model.id,
        changes: normalized,
      });
      const parameters = getParameters(next);
      setBase({
        revision: next.modelsRevision,
        path: next.modelsPath,
        parameters,
      });
      setDraft({ ...parameters });
      updated(next);
    } catch (e) {
      setError(extractErrorMessage(e));
    } finally {
      setBusy(false);
    }
  };
  const set = (field: string, value: unknown) =>
    setDraft((v) => ({ ...v, [field]: value }));
  return (
    <section
      aria-label={t("omp.management.parameters")}
      className="space-y-4 rounded-panel border border-border p-4"
    >
      <h3 className="break-all text-strong">
        {model.provider}/{model.id} · {t("omp.management.parameters")}
      </h3>
      <p className="text-caption text-fg-2">
        {t("omp.management.overrideHint")}
      </p>
      <code className="block break-all text-caption">{state.modelsPath}</code>
      <fieldset disabled={busy} className="space-y-3">
        {parameterFields.map((field) => (
          <div key={field} className="space-y-1 border-b border-border pb-3">
            <div className="flex justify-between gap-2">
              <span className="font-mono text-body">{field}</span>
              <span className="text-caption text-fg-2">
                {draft[field] === undefined
                  ? t("omp.management.noOverride")
                  : t("omp.management.explicitOverride")}
              </span>
            </div>
            <p className="break-all text-caption text-fg-2">
              {t("omp.management.catalogValue")}:{" "}
              {catalog
                ? JSON.stringify(
                    catalog[field as keyof OmpCatalogModel] ?? null,
                  )
                : t("omp.management.noCatalogData")}
            </p>
            {field === "reasoning" ? (
              <label className="flex gap-2">
                <span>{field}</span>
                <select
                  aria-label={field}
                  className={selectorClass}
                  value={draft[field] === undefined ? "" : String(draft[field])}
                  onChange={(e) => {
                    if (e.target.value === "")
                      setDraft((v) => {
                        const next = { ...v };
                        delete next[field];
                        return next;
                      });
                    else set(field, e.target.value === "true");
                  }}
                >
                  <option value="">{t("omp.management.noOverride")}</option>
                  <option value="true">true</option>
                  <option value="false">false</option>
                </select>
              </label>
            ) : field === "input" ? (
              <div className="flex gap-4">
                {["text", "image"].map((kind) => (
                  <label key={kind} className="flex gap-2">
                    <input
                      type="checkbox"
                      checked={
                        Array.isArray(draft.input) && draft.input.includes(kind)
                      }
                      onChange={(e) =>
                        set(
                          "input",
                          e.target.checked
                            ? [
                                ...(Array.isArray(draft.input)
                                  ? draft.input
                                  : []),
                                kind,
                              ]
                            : (Array.isArray(draft.input)
                                ? draft.input
                                : []
                              ).filter((v) => v !== kind),
                        )
                      }
                    />
                    {kind}
                  </label>
                ))}
              </div>
            ) : (
              <Input
                aria-label={field}
                type={field === "name" ? "text" : "number"}
                value={String(draft[field] ?? "")}
                min={field === "name" ? undefined : 1}
                onChange={(e) =>
                  set(
                    field,
                    field === "name"
                      ? e.target.value
                      : e.target.value === ""
                        ? ""
                        : Number(e.target.value),
                  )
                }
              />
            )}
            <Button
              variant="quiet"
              disabled={draft[field] === undefined}
              aria-label={`${t("omp.management.reset")} ${field}`}
              onClick={() =>
                setDraft((v) => {
                  const next = { ...v };
                  delete next[field];
                  return next;
                })
              }
            >
              {t("omp.management.reset")}
            </Button>
          </div>
        ))}
      </fieldset>
      {invalid && (
        <p role="alert" className="text-danger-text">
          {t("omp.management.invalidParameters")}
        </p>
      )}
      {error && (
        <p role="alert" className="text-danger-text">
          {translatePiProviderMutationError(error, t)}
        </p>
      )}
      <div className="flex flex-wrap justify-end gap-2">
        <Button
          variant="neutral"
          disabled={busy || !dirty}
          onClick={() =>
            guard(() => {
              setDraft({ ...base.parameters });
              setError("");
            }, dirty)
          }
        >
          {t("common.cancel")}
        </Button>
        <Button
          variant="solid"
          disabled={
            busy ||
            !dirty ||
            invalid ||
            error.includes("omp.errors.externalChange")
          }
          onClick={() => void save()}
        >
          {t("omp.management.saveParameters")}
        </Button>
      </div>
    </section>
  );
}

function ReferenceRow({
  name,
  value,
  onChange,
  onClear,
}: {
  name: string;
  value: string | string[];
  onChange: (v: string | string[]) => void;
  onClear: () => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="space-y-1 rounded-md bg-subtle p-3">
      <label className="block space-y-1">
        <span className="font-mono text-caption">{name}</span>
        {Array.isArray(value) ? (
          <Textarea
            value={value.join("\n")}
            onChange={(e) => onChange(e.target.value.split("\n"))}
          />
        ) : (
          <Input
            list="omp-reference-options"
            value={value}
            placeholder={t("omp.management.inherit")}
            onChange={(e) => onChange(e.target.value)}
          />
        )}
      </label>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <label className="flex gap-2 text-caption">
          <input
            type="checkbox"
            checked={Array.isArray(value)}
            disabled={Array.isArray(value) && value.length > 1}
            onChange={(e) =>
              onChange(
                e.target.checked
                  ? [value as string]
                  : ((value as string[])[0] ?? ""),
              )
            }
          />
          {t("omp.management.candidateList")}
        </label>
        <Button
          variant="quiet"
          aria-label={`${t("omp.management.clear")} ${name}`}
          onClick={onClear}
        >
          {t("omp.management.clear")}
        </Button>
      </div>
    </div>
  );
}
