import { useEffect, useMemo, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import {
  ChevronDown,
  ListOrdered,
  Loader2,
  Plus,
  RefreshCw,
  RotateCcw,
  X,
} from "lucide-react";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import {
  SettingsBlock,
  SettingsCard,
  SettingsRow,
} from "@/components/settings/SettingsLayout";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { HelpTip } from "@/components/ui/help-tip";
import { HoverTip } from "@/components/ui/hover-tip";
import { Notice } from "@/components/ui/notice";
import { PageTabs } from "@/components/ui/page-tabs";
import { SearchField } from "@/components/ui/search-field";
import { SegmentedControl } from "@/components/ui/segmented-control";
import { Switch } from "@/components/ui/switch";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import {
  Command,
  CommandEmpty,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";
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
import { cn } from "@/lib/utils";
import { summarizeOmpReferences } from "./ompModelReferences";

export const ompManagementKey = ["omp", "modelManagement"];
const equal = (a: unknown, b: unknown) =>
  JSON.stringify(a) === JSON.stringify(b);
const refValid = (value: string) =>
  value.trim().length > 0 && !/[\u0000-\u001f\u007f]/.test(value);
const parameterFields = [
  "name",
  "contextWindow",
  "maxContextWindow",
  "maxTokens",
  "input",
  "reasoning",
] as const;
const builtinRoles: Record<string, true> = {
  default: true,
  smol: true,
  slow: true,
  vision: true,
  plan: true,
  commit: true,
  tiny: true,
  memory: true,
  task: true,
  advisor: true,
  widget: true,
  image: true,
  web: true,
  speech: true,
  dictation: true,
  judge: true,
};
const builtinWorkers: Record<string, true> = {
  task: true,
  scout: true,
  sonic: true,
  "security-reviewer": true,
};
type Tab = "models" | "assignments";
type ModelIdentity = Pick<OmpCatalogModel, "provider" | "id">;
type EditorStatus = { dirty: boolean; invalid: boolean; conflict: boolean };
const cleanStatus: EditorStatus = {
  dirty: false,
  invalid: false,
  conflict: false,
};
type Guard = (action: () => void, dirty?: boolean) => void;
const formId = (tab: Tab) => `omp-${tab}-form`;

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
  const [tab, setTab] = useState<Tab>("models");
  const [settings, setSettings] = useState(cleanStatus);
  const [parameters, setParameters] = useState(cleanStatus);
  const [busy, setBusy] = useState(false);
  const [pending, setPending] = useState<(() => void) | null>(null);
  const [generation, setGeneration] = useState(0);
  const dirty = settings.dirty || parameters.dirty;
  const active = tab === "models" ? parameters : settings;
  const otherDirty = tab === "models" ? settings.dirty : parameters.dirty;
  const guard: Guard = (action, discard = dirty) => {
    if (busy) return;
    if (discard) setPending(() => action);
    else action();
  };
  const updated = (state: OmpModelManagementState) => {
    client.setQueryData(ompManagementKey, state);
    void invalidatePiProviderCaches(client, "omp");
  };
  const reload = () =>
    guard(() => {
      setBusy(true);
      void query
        .refetch()
        .then((result) => {
          if (result.isSuccess) {
            setGeneration((value) => value + 1);
            setSettings(cleanStatus);
            setParameters(cleanStatus);
          }
        })
        .finally(() => setBusy(false));
    });
  return (
    <FullScreenPanel
      isOpen
      title={t("omp.management.title")}
      subtitle="Oh My Pi"
      onClose={() => guard(onClose)}
      trackUnsavedChanges={dirty}
      isDirty={dirty}
      contentClassName="space-y-4 pt-3"
      actions={
        <div className="flex items-center gap-2" data-unsaved-ignore>
          <HelpTip title={t("omp.management.scopeTitle")}>
            <div className="space-y-2">
              <p>{t("omp.management.scope")}</p>
              <p>{t("omp.management.cacheHint")}</p>
              <code className="block break-all">
                {query.data?.settingsPath}
              </code>
              <code className="block break-all">{query.data?.modelsPath}</code>
            </div>
          </HelpTip>
          <HoverTip content={t("omp.management.reload")}>
            <Button
              variant="quiet"
              size="icon-compact"
              aria-label={t("omp.management.reload")}
              disabled={busy || query.isFetching}
              onClick={reload}
            >
              <RefreshCw className="h-4 w-4" strokeWidth={1.5} />
            </Button>
          </HoverTip>
        </div>
      }
      footer={
        query.data && (
          <>
            <div className="me-auto min-w-0 text-caption text-fg-2">
              <span>
                {(tab === "models"
                  ? query.data.modelsPath
                  : query.data.settingsPath
                )
                  .split(/[\\/]/)
                  .pop()}
              </span>
              {otherDirty && (
                <span role="status" className="ms-3">
                  {t("omp.management.otherDraft")}
                </span>
              )}
            </div>
            <Button
              variant="neutral"
              size="regular"
              type="reset"
              form={formId(tab)}
              disabled={busy || !active.dirty}
            >
              {t("common.cancel")}
            </Button>
            <Button
              variant="solid"
              size="regular"
              type="submit"
              form={formId(tab)}
              disabled={
                busy || !active.dirty || active.invalid || active.conflict
              }
            >
              {busy && <Loader2 className="h-4 w-4 animate-spin" />}
              {t(
                tab === "models"
                  ? "omp.management.saveParameters"
                  : "omp.management.saveSettings",
              )}
            </Button>
          </>
        )
      }
    >
      <div data-unsaved-ignore>
        <PageTabs
          items={[
            {
              value: "models",
              label: t("omp.management.modelsTab"),
              disabled: busy,
            },
            {
              value: "assignments",
              label: t("omp.management.assignmentsTab"),
              disabled: busy,
            },
          ]}
          value={tab}
          onValueChange={setTab}
          aria-label={t("omp.management.title")}
          idPrefix="omp-management-tab"
          controls="omp-management-content"
          trailing={
            <span className="text-caption text-fg-3">
              {t("omp.management.globalConfig")}
            </span>
          }
        />
      </div>
      {query.isPending && <p role="status">{t("common.loading")}</p>}
      {query.error && (
        <Notice
          tone="danger"
          title={translatePiProviderMutationError(
            extractErrorMessage(query.error),
            t,
          )}
        />
      )}
      {query.data && (
        <ManagementBody
          key={generation}
          state={query.data}
          initialProvider={initialProvider}
          tab={tab}
          setTab={setTab}
          updated={updated}
          settingsStatus={setSettings}
          parameterStatus={setParameters}
          parameterDirty={parameters.dirty}
          busy={busy}
          setBusy={setBusy}
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
  tab,
  setTab,
  updated,
  settingsStatus,
  parameterStatus,
  parameterDirty,
  busy,
  setBusy,
  guard,
}: {
  state: OmpModelManagementState;
  initialProvider?: string;
  tab: Tab;
  setTab: (tab: Tab) => void;
  updated: (state: OmpModelManagementState) => void;
  settingsStatus: (status: EditorStatus) => void;
  parameterStatus: (status: EditorStatus) => void;
  parameterDirty: boolean;
  busy: boolean;
  setBusy: (busy: boolean) => void;
  guard: Guard;
}) {
  const { t } = useTranslation();
  const [filter, setFilter] = useState(initialProvider ?? "");
  const [visibleCount, setVisibleCount] = useState(50);
  const [selected, setSelected] = useState<ModelIdentity | null>(() => {
    const model = state.models.find(
      (entry) => entry.provider === initialProvider,
    );
    const override = state.modelOverrides.find(
      (entry) => entry.provider === initialProvider,
    );
    return (
      model ??
      (override
        ? { provider: override.provider, id: override.modelId }
        : (state.models[0] ??
          (state.modelOverrides[0]
            ? {
                provider: state.modelOverrides[0].provider,
                id: state.modelOverrides[0].modelId,
              }
            : null)))
    );
  });
  const known = useMemo(
    () => new Set(state.models.map((model) => `${model.provider}/${model.id}`)),
    [state.models],
  );
  const models = useMemo(
    () =>
      state.models.filter((model) =>
        `${model.provider}/${model.id} ${model.name}`
          .toLowerCase()
          .includes(filter.toLowerCase()),
      ),
    [state.models, filter],
  );
  const uncataloged = useMemo(
    () =>
      state.modelOverrides.filter(
        (entry) =>
          !known.has(`${entry.provider}/${entry.modelId}`) &&
          `${entry.provider}/${entry.modelId}`
            .toLowerCase()
            .includes(filter.toLowerCase()),
      ),
    [state.modelOverrides, known, filter],
  );
  const references = useMemo(() => summarizeOmpReferences(state), [state]);
  const choose = (model: ModelIdentity) => {
    if (model.provider === selected?.provider && model.id === selected.id)
      return;
    guard(() => {
      parameterStatus(cleanStatus);
      setSelected(model);
    }, parameterDirty);
  };
  const renderModel = (
    model: ModelIdentity,
    catalog: OmpCatalogModel | null,
  ) => {
    const selectedRow =
      selected?.provider === model.provider && selected.id === model.id;
    const overridden = state.modelOverrides.some(
      (entry) =>
        entry.provider === model.provider && entry.modelId === model.id,
    );
    return (
      <button
        key={`${model.provider}/${model.id}`}
        type="button"
        aria-pressed={selectedRow}
        aria-label={
          catalog
            ? `${catalog.name} ${model.provider}/${model.id}`
            : `${model.provider}/${model.id}`
        }
        title={`${model.provider}/${model.id}`}
        disabled={busy}
        onClick={() => choose(model)}
        className={cn(
          "block w-full min-w-0 px-3.5 py-2.5 text-start focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring",
          selectedRow ? "bg-selected" : "hover:bg-subtle",
        )}
      >
        <span className="block truncate text-body font-medium text-fg-1">
          {catalog?.name ?? model.id}
        </span>
        <span className="mt-0.5 flex min-w-0 items-center gap-1.5">
          <span className="min-w-0 flex-1 truncate font-mono text-caption text-fg-2">
            {model.id}
          </span>
          {catalog && (
            <Badge
              variant="outline"
              className="shrink-0 rounded-control px-1.5 py-0 text-badge font-medium text-fg-2"
            >
              {t(`omp.management.shortSource.${catalog.source}`)}
            </Badge>
          )}
          {overridden && (
            <Badge
              variant="outline"
              className="shrink-0 rounded-control px-1.5 py-0 text-badge font-medium text-fg-2"
            >
              {t("omp.management.overridden")}
            </Badge>
          )}
        </span>
      </button>
    );
  };
  const catalog = selected
    ? (state.models.find(
        (model) =>
          model.provider === selected.provider && model.id === selected.id,
      ) ?? null)
    : null;
  return (
    <div
      id="omp-management-content"
      role="tabpanel"
      aria-labelledby={`omp-management-tab-${tab}`}
    >
      <div hidden={tab !== "models"}>
        <section aria-label={t("omp.management.catalog")} className="space-y-3">
          {state.catalogStatus !== "ready" && (
            <Notice
              tone="warning"
              title={t(`omp.management.catalogStatus.${state.catalogStatus}`)}
            >
              {state.catalogMessage ? t(state.catalogMessage) : undefined}
            </Notice>
          )}
          <div className="flex items-center gap-3" data-unsaved-ignore>
            <SearchField
              value={filter}
              onValueChange={(value) => {
                setFilter(value);
                setVisibleCount(50);
              }}
              clearLabel={t("common.clear")}
              aria-label={t("omp.management.search")}
              placeholder={t("omp.management.search")}
              containerClassName="min-w-0 flex-1"
            />
            <span className="shrink-0 text-caption tabular-nums text-fg-3">
              {t("omp.management.modelCount", { count: models.length })}
            </span>
          </div>
          <div className="grid min-w-0 items-start gap-5 min-[900px]:grid-cols-[minmax(240px,290px)_minmax(0,1fr)]">
            <div
              className="max-h-[calc(100vh-250px)] min-h-32 overflow-y-auto rounded-panel border border-border bg-surface"
              data-unsaved-ignore
            >
              <div className="divide-y divide-border">
                {models
                  .slice(0, visibleCount)
                  .map((model) => renderModel(model, model))}
              </div>
              {models.length === 0 && (
                <p className="px-3.5 py-6 text-body text-fg-2">
                  {t("omp.management.emptyCatalog")}
                </p>
              )}
              {models.length > visibleCount && (
                <Button
                  variant="quiet"
                  className="w-full"
                  onClick={() => setVisibleCount((value) => value + 50)}
                >
                  {t("omp.management.showMore")}
                </Button>
              )}
              {uncataloged.length > 0 && (
                <>
                  <h3 className="border-y border-border bg-subtle px-3.5 py-2 text-caption font-medium text-fg-2">
                    {t("omp.management.explicitRecords")}
                  </h3>
                  <div className="divide-y divide-border">
                    {uncataloged
                      .slice(0, visibleCount)
                      .map((entry) =>
                        renderModel(
                          { provider: entry.provider, id: entry.modelId },
                          null,
                        ),
                      )}
                  </div>
                  {uncataloged.length > visibleCount && (
                    <Button
                      variant="quiet"
                      className="w-full"
                      onClick={() => setVisibleCount((value) => value + 50)}
                    >
                      {t("omp.management.showMore")}
                    </Button>
                  )}
                </>
              )}
            </div>
            {selected ? (
              <ParameterEditor
                key={`${selected.provider}/${selected.id}`}
                state={state}
                model={selected}
                catalog={catalog}
                updated={updated}
                statusChanged={parameterStatus}
                busy={busy}
                setBusy={setBusy}
                guard={guard}
                onAssignments={() => setTab("assignments")}
              />
            ) : (
              <p className="py-8 text-body text-fg-2">
                {t("omp.management.selectModel")}
              </p>
            )}
          </div>
        </section>
      </div>
      <div hidden={tab !== "assignments"}>
        <SettingsEditor
          state={state}
          updated={updated}
          statusChanged={settingsStatus}
          busy={busy}
          setBusy={setBusy}
          guard={guard}
        />
        {references.unresolved.length > 0 && (
          <details className="mt-5 text-caption text-fg-2" data-unsaved-ignore>
            <summary className="cursor-pointer">
              {t("omp.management.unresolved")} ({references.unresolved.length})
            </summary>
            <ul className="mt-2 space-y-1 break-words">
              {references.unresolved.map((value) => (
                <li key={value}>{value}</li>
              ))}
            </ul>
          </details>
        )}
      </div>
    </div>
  );
}

function SettingsEditor({
  state,
  updated,
  statusChanged,
  busy,
  setBusy,
  guard,
}: {
  state: OmpModelManagementState;
  updated: (state: OmpModelManagementState) => void;
  statusChanged: (status: EditorStatus) => void;
  busy: boolean;
  setBusy: (busy: boolean) => void;
  guard: Guard;
}) {
  const { t } = useTranslation();
  const [base, setBase] = useState(state);
  const [roles, setRoles] = useState({ ...state.modelRoles });
  const [workers, setWorkers] = useState({ ...state.agentModelOverrides });
  const [thinking, setThinking] = useState<
    Exclude<OmpThinkingLevel, "off"> | ""
  >(state.defaultThinkingLevel ?? "");
  const [kind, setKind] = useState<"roles" | "workers">("roles");
  const [newKey, setNewKey] = useState("");
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
    (change) =>
      change.value !== null &&
      (Array.isArray(change.value)
        ? change.value.length === 0 ||
          change.value.some((value) => !refValid(value))
        : !refValid(change.value)),
  );
  const dirty = changes.length > 0;
  const conflict = error.includes("omp.errors.externalChange");
  useEffect(
    () => statusChanged({ dirty, invalid, conflict }),
    [dirty, invalid, conflict, statusChanged],
  );
  const cancel = () => {
    setRoles({ ...base.modelRoles });
    setWorkers({ ...base.agentModelOverrides });
    setThinking(base.defaultThinkingLevel ?? "");
    setError("");
  };
  const save = async () => {
    if (busy || !dirty || invalid || conflict) return;
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
      updated(next);
    } catch (error) {
      setError(extractErrorMessage(error));
    } finally {
      setBusy(false);
    }
  };
  const roleNames = [...new Set([...state.roleNames, ...Object.keys(roles)])];
  const referenceProps = { state, roleNames, disabled: busy };
  const entries =
    kind === "roles"
      ? roleNames
          .filter((key) => key !== "default")
          .map((key) => [key, roles[key] ?? ""] as const)
      : Object.entries(workers);
  const current = kind === "roles" ? roles : workers;
  const changeEntry = (key: string, value: string | string[]) =>
    kind === "roles"
      ? setRoles((entries) => ({ ...entries, [key]: value }))
      : setWorkers((entries) => ({ ...entries, [key]: value }));
  return (
    <form
      id={formId("assignments")}
      onSubmit={(event) => {
        event.preventDefault();
        void save();
      }}
      onReset={(event) => {
        event.preventDefault();
        guard(cancel, dirty);
      }}
      className="space-y-5"
      aria-label={t("omp.management.settings")}
    >
      <SettingsBlock
        title={t("omp.management.configuredDefault")}
        help={{
          title: t("omp.management.referenceHelp"),
          body: t("omp.management.referenceHint"),
        }}
      >
        <SettingsCard>
          <div className="px-4 py-3">
            <ReferenceField
              {...referenceProps}
              name={t("omp.management.defaultReference")}
              value={roles.default ?? ""}
              onChange={(value) =>
                setRoles((entries) => ({ ...entries, default: value }))
              }
              onClear={() =>
                setRoles((entries) => ({ ...entries, default: "" }))
              }
            />
            <DefaultThinkingSuffix
              state={state}
              value={roles.default}
              disabled={busy}
              onChange={(value) =>
                setRoles((entries) => ({ ...entries, default: value }))
              }
            />
          </div>
          <SettingsRow
            label={t("omp.management.defaultThinking")}
            control={
              <Select
                value={thinking || "__native"}
                onValueChange={(value) =>
                  setThinking(
                    value === "__native"
                      ? ""
                      : (value as Exclude<OmpThinkingLevel, "off">),
                  )
                }
                disabled={busy}
              >
                <SelectTrigger
                  className="w-48"
                  aria-label={t("omp.management.defaultThinking")}
                >
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {[
                    "__native",
                    ...OMP_THINKING_LEVELS.filter((value) => value !== "off"),
                  ].map((value) => (
                    <SelectItem key={value} value={value}>
                      {value === "__native"
                        ? t("omp.management.usingNative")
                        : value}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            }
          />
        </SettingsCard>
      </SettingsBlock>
      <SettingsBlock
        actions={
          <div data-unsaved-ignore>
            <SegmentedControl
              size="sm"
              value={kind}
              onValueChange={(value) => {
                setKind(value);
                setNewKey("");
              }}
              aria-label={t("omp.management.assignments")}
              items={[
                { value: "roles", label: t("omp.management.roles") },
                { value: "workers", label: t("omp.management.workers") },
              ]}
            />
          </div>
        }
        title={t("omp.management.assignments")}
      >
        <SettingsCard>
          {entries.map(([key, value]) => (
            <div
              key={`${kind}-${key}`}
              className="flex min-w-0 items-start gap-4 px-4 py-3"
            >
              <div className="w-28 shrink-0 pt-1.5">
                <div
                  className="truncate text-body font-medium text-fg-1"
                  title={key}
                >
                  {Object.prototype.hasOwnProperty.call(
                    kind === "roles" ? builtinRoles : builtinWorkers,
                    key,
                  )
                    ? t(
                        `omp.management.${kind === "roles" ? "roleLabels" : "workerLabels"}.${key}`,
                      )
                    : key}
                </div>
                <code
                  className="block truncate text-caption text-fg-3"
                  title={key}
                >
                  {key}
                </code>
              </div>
              <div className="min-w-0 flex-1">
                <ReferenceField
                  {...referenceProps}
                  name={key}
                  value={value}
                  onChange={(next) => changeEntry(key, next)}
                  onClear={() => {
                    if (kind === "roles")
                      setRoles((entries) => ({ ...entries, [key]: "" }));
                    else
                      setWorkers((entries) => {
                        const next = { ...entries };
                        delete next[key];
                        return next;
                      });
                  }}
                />
              </div>
            </div>
          ))}
          <div className="flex min-w-0 items-center gap-2 px-4 py-3">
            <Input
              aria-label={t(
                kind === "roles"
                  ? "omp.management.newRole"
                  : "omp.management.newWorker",
              )}
              placeholder={t(
                kind === "roles"
                  ? "omp.management.newRole"
                  : "omp.management.newWorker",
              )}
              value={newKey}
              disabled={busy}
              onChange={(event) => setNewKey(event.target.value)}
            />
            <Button
              type="button"
              variant="neutral"
              size="compact"
              disabled={
                busy ||
                !refValid(newKey) ||
                Object.prototype.hasOwnProperty.call(current, newKey)
              }
              onClick={() => {
                changeEntry(newKey, "");
                setNewKey("");
              }}
            >
              <Plus className="h-3.5 w-3.5" strokeWidth={1.5} />
              {t(
                kind === "roles"
                  ? "omp.management.addRole"
                  : "omp.management.addWorker",
              )}
            </Button>
          </div>
        </SettingsCard>
      </SettingsBlock>
      {invalid && (
        <p role="alert" className="text-body text-danger-text">
          {t("omp.management.invalidReference")}
        </p>
      )}
      {error && (
        <p role="alert" className="text-body text-danger-text">
          {translatePiProviderMutationError(error, t)}
        </p>
      )}
    </form>
  );
}

function DefaultThinkingSuffix({
  state,
  value,
  disabled,
  onChange,
}: {
  state: OmpModelManagementState;
  value: string | string[] | undefined;
  disabled: boolean;
  onChange: (value: string) => void;
}) {
  const { t } = useTranslation();
  const match =
    typeof value === "string"
      ? (state.models.find(
          (model) => value === `${model.provider}/${model.id}`,
        ) ??
        state.models.find((model) =>
          [...OMP_THINKING_LEVELS, "inherit"].some(
            (level) => value === `${model.provider}/${model.id}:${level}`,
          ),
        ))
      : undefined;
  const selector = match ? `${match.provider}/${match.id}` : "";
  const suffix =
    match && value !== selector
      ? (value as string).slice(selector.length + 1)
      : "__native";
  const levels = [
    ...new Set([
      "__native",
      "off",
      "inherit",
      "auto",
      ...(match?.thinking?.efforts ?? []),
      suffix,
    ]),
  ];
  return (
    <div
      className="mt-2 flex items-center justify-end gap-2"
      data-unsaved-ignore
    >
      <span className="text-caption text-fg-2">
        {t("omp.management.modelThinking")}
      </span>
      <Select
        value={suffix}
        disabled={disabled || !match}
        onValueChange={(level) => {
          if (disabled || !match || !level || !levels.includes(level)) return;
          onChange(`${selector}${level === "__native" ? "" : `:${level}`}`);
        }}
      >
        <SelectTrigger
          className="w-48"
          aria-label={t("omp.management.modelThinking")}
        >
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {levels.map((level) => (
            <SelectItem key={level} value={level}>
              {level === "__native" ? t("omp.management.usingNative") : level}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </div>
  );
}

function ParameterEditor({
  state,
  model,
  catalog,
  updated,
  statusChanged,
  busy,
  setBusy,
  guard,
  onAssignments,
}: {
  state: OmpModelManagementState;
  model: ModelIdentity;
  catalog: OmpCatalogModel | null;
  updated: (state: OmpModelManagementState) => void;
  statusChanged: (status: EditorStatus) => void;
  busy: boolean;
  setBusy: (busy: boolean) => void;
  guard: Guard;
  onAssignments: () => void;
}) {
  const { t } = useTranslation();
  const getParameters = (snapshot: OmpModelManagementState) =>
    snapshot.modelOverrides.find(
      (entry) =>
        entry.provider === model.provider && entry.modelId === model.id,
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
  const conflict = error.includes("omp.errors.externalChange");
  useEffect(
    () => statusChanged({ dirty, invalid, conflict }),
    [dirty, invalid, conflict, statusChanged],
  );
  const reset = (field: string) =>
    setDraft((entries) => {
      const next = { ...entries };
      delete next[field];
      return next;
    });
  const set = (field: string, value: unknown) =>
    setDraft((entries) => ({ ...entries, [field]: value }));
  const save = async () => {
    if (busy || !dirty || invalid || conflict) return;
    setBusy(true);
    setError("");
    try {
      const next = await ompApi.patchOverride({
        expectedRevision: base.revision,
        expectedPath: base.path,
        provider: model.provider,
        modelId: model.id,
        changes: changes.map((change) => ({
          ...change,
          value:
            change.value !== null &&
            ["contextWindow", "maxContextWindow", "maxTokens"].includes(
              change.field,
            )
              ? Number(change.value)
              : change.value,
        })),
      });
      const parameters = getParameters(next);
      setBase({
        revision: next.modelsRevision,
        path: next.modelsPath,
        parameters,
      });
      setDraft({ ...parameters });
      updated(next);
    } catch (error) {
      setError(extractErrorMessage(error));
    } finally {
      setBusy(false);
    }
  };
  return (
    <form
      id={formId("models")}
      onSubmit={(event) => {
        event.preventDefault();
        void save();
      }}
      onReset={(event) => {
        event.preventDefault();
        guard(() => {
          setDraft({ ...base.parameters });
          setError("");
        }, dirty);
      }}
      aria-label={t("omp.management.parameters")}
      className="min-w-0 space-y-4"
    >
      <div className="flex min-w-0 items-start gap-2">
        <div className="min-w-0 flex-1">
          <h2
            className="m-0 truncate text-section text-fg-1"
            title={catalog?.name ?? model.id}
          >
            {catalog?.name ?? model.id}
          </h2>
          <p className="mt-1 break-all font-mono text-caption text-fg-2">
            {model.provider}/{model.id}
          </p>
          <p className="mt-1 text-caption text-fg-3">
            {catalog
              ? t(`omp.management.source.${catalog.source}`)
              : t("omp.management.noCatalogData")}
            {catalog?.cacheUpdatedAt != null
              ? ` · ${new Date(catalog.cacheUpdatedAt).toLocaleString()}`
              : ""}
          </p>
        </div>
        <HelpTip title={t("omp.management.parameters")}>
          <div className="space-y-2">
            <p>{t("omp.management.overrideHint")}</p>
            <code className="break-all">{state.modelsPath}</code>
          </div>
        </HelpTip>
      </div>
      <SettingsCard>
        {parameterFields.map((field) => {
          const label = t(`omp.management.parameterLabels.${field}`);
          const value =
            draft[field] !== undefined
              ? draft[field]
              : (catalog?.[field] ?? undefined);
          return (
            <SettingsRow
              key={field}
              label={label}
              htmlFor={field === "input" ? undefined : `omp-parameter-${field}`}
              description={t(
                draft[field] === undefined
                  ? "omp.management.usingNative"
                  : "omp.management.explicitOverride",
              )}
              control={
                <>
                  {field === "reasoning" ? (
                    <Select
                      value={
                        draft.reasoning === undefined
                          ? "__native"
                          : String(draft.reasoning)
                      }
                      disabled={busy}
                      onValueChange={(value) =>
                        value === "__native"
                          ? reset(field)
                          : set(field, value === "true")
                      }
                    >
                      <SelectTrigger
                        id={`omp-parameter-${field}`}
                        className="w-44"
                        aria-label={label}
                      >
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="__native">
                          {t("omp.management.usingNative")}
                          {catalog
                            ? ` · ${t(catalog.reasoning ? "omp.management.enabled" : "omp.management.disabled")}`
                            : ""}
                        </SelectItem>
                        <SelectItem value="true">
                          {t("omp.management.enabled")}
                        </SelectItem>
                        <SelectItem value="false">
                          {t("omp.management.disabled")}
                        </SelectItem>
                      </SelectContent>
                    </Select>
                  ) : field === "input" ? (
                    <div
                      role="group"
                      aria-label={label}
                      className="flex w-44 flex-wrap gap-x-3 gap-y-1"
                    >
                      {["text", "image"].map((input) => (
                        <label
                          key={input}
                          className="flex items-center gap-1.5 text-caption text-fg-1"
                        >
                          <Switch
                            aria-label={t(
                              `omp.management.inputLabels.${input}`,
                            )}
                            checked={
                              Array.isArray(value) && value.includes(input)
                            }
                            disabled={busy}
                            onCheckedChange={(checked) => {
                              const values = Array.isArray(value) ? value : [];
                              set(
                                "input",
                                checked
                                  ? [...values, input]
                                  : values.filter((entry) => entry !== input),
                              );
                            }}
                          />
                          {t(`omp.management.inputLabels.${input}`)}
                        </label>
                      ))}
                    </div>
                  ) : (
                    <Input
                      id={`omp-parameter-${field}`}
                      aria-label={label}
                      className="w-44"
                      disabled={busy}
                      type={field === "name" ? "text" : "number"}
                      min={field === "name" ? undefined : 1}
                      value={String(value ?? "")}
                      placeholder={t("omp.management.unknownValue")}
                      onChange={(event) =>
                        set(
                          field,
                          field === "name"
                            ? event.target.value
                            : event.target.value === ""
                              ? ""
                              : Number(event.target.value),
                        )
                      }
                    />
                  )}
                  <HoverTip content={t("omp.management.reset")}>
                    <Button
                      type="button"
                      variant="quiet"
                      size="icon-compact"
                      disabled={busy || draft[field] === undefined}
                      aria-label={`${t("omp.management.reset")} ${label}`}
                      onClick={() => reset(field)}
                    >
                      <RotateCcw className="h-3.5 w-3.5" strokeWidth={1.5} />
                    </Button>
                  </HoverTip>
                </>
              }
            />
          );
        })}
      </SettingsCard>
      <Button
        type="button"
        variant="quiet"
        size="compact"
        onClick={onAssignments}
        data-unsaved-ignore
      >
        {t("omp.management.configureAssignments")}
      </Button>
      {invalid && (
        <p role="alert" className="text-body text-danger-text">
          {t("omp.management.invalidParameters")}
        </p>
      )}
      {error && (
        <p role="alert" className="text-body text-danger-text">
          {translatePiProviderMutationError(error, t)}
        </p>
      )}
    </form>
  );
}

function ReferenceField({
  name,
  value,
  state,
  roleNames,
  disabled,
  onChange,
  onClear,
}: {
  name: string;
  value: string | string[];
  state: OmpModelManagementState;
  roleNames: string[];
  disabled: boolean;
  onChange: (value: string | string[]) => void;
  onClear: () => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="flex min-w-0 items-start gap-1.5">
      {Array.isArray(value) ? (
        <Textarea
          aria-label={name}
          value={value.join("\n")}
          rows={Math.max(2, Math.min(4, value.length))}
          disabled={disabled}
          className="min-h-16 min-w-0 flex-1 text-body"
          onChange={(event) => onChange(event.target.value.split("\n"))}
        />
      ) : (
        <Input
          aria-label={name}
          value={value}
          disabled={disabled}
          className="min-w-0 flex-1"
          placeholder={t("omp.management.usingNative")}
          onChange={(event) => onChange(event.target.value)}
        />
      )}
      {!Array.isArray(value) && (
        <ReferencePicker
          state={state}
          roleNames={roleNames}
          disabled={disabled}
          name={name}
          onSelect={onChange}
        />
      )}
      <HoverTip content={t("omp.management.candidateList")}>
        <Button
          type="button"
          variant="quiet"
          size="icon-compact"
          aria-label={`${t("omp.management.candidateList")} ${name}`}
          aria-pressed={Array.isArray(value)}
          disabled={disabled || (Array.isArray(value) && value.length > 1)}
          onClick={() =>
            onChange(Array.isArray(value) ? (value[0] ?? "") : [value])
          }
        >
          <ListOrdered className="h-4 w-4" strokeWidth={1.5} />
        </Button>
      </HoverTip>
      <HoverTip content={t("omp.management.clear")}>
        <Button
          type="button"
          variant="quiet"
          size="icon-compact"
          aria-label={`${t("omp.management.clear")} ${name}`}
          disabled={disabled || value === ""}
          onClick={onClear}
        >
          <X className="h-3.5 w-3.5" strokeWidth={1.5} />
        </Button>
      </HoverTip>
    </div>
  );
}

function ReferencePicker({
  state,
  roleNames,
  disabled,
  name,
  onSelect,
}: {
  state: OmpModelManagementState;
  roleNames: string[];
  disabled: boolean;
  name: string;
  onSelect: (selector: string) => void;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const options = useMemo(
    () => [
      ...roleNames.map((role) => ({ value: `@${role}`, name: `@${role}` })),
      ...state.models.map((model) => ({
        value: `${model.provider}/${model.id}`,
        name: model.name,
      })),
    ],
    [roleNames, state.models],
  );
  const matches = options
    .filter((option) =>
      `${option.value} ${option.name}`
        .toLowerCase()
        .includes(query.toLowerCase()),
    )
    .slice(0, 50);
  return (
    <Popover
      modal
      open={open}
      onOpenChange={(value) => {
        setOpen(value);
        if (!value) setQuery("");
      }}
    >
      <HoverTip content={t("omp.management.modelChoice")}>
        <PopoverTrigger asChild>
          <Button
            type="button"
            variant="neutral"
            size="icon"
            disabled={disabled}
            aria-label={`${t("omp.management.modelChoice")} ${name}`}
            data-unsaved-ignore
          >
            <ChevronDown className="h-4 w-4" strokeWidth={1.5} />
          </Button>
        </PopoverTrigger>
      </HoverTip>
      <PopoverContent
        align="end"
        className="z-[200] w-80 p-0"
        data-unsaved-ignore
      >
        <Command shouldFilter={false} label={t("omp.management.modelChoice")}>
          <CommandInput
            value={query}
            onValueChange={setQuery}
            placeholder={t("omp.management.search")}
          />
          <CommandList className="max-h-64">
            <CommandEmpty>{t("omp.management.emptyCatalog")}</CommandEmpty>
            {matches.map((option) => (
              <CommandItem
                key={option.value}
                value={option.value}
                onSelect={() => {
                  onSelect(option.value);
                  setOpen(false);
                  setQuery("");
                }}
              >
                <span className="min-w-0">
                  <span className="block truncate text-body">
                    {option.name}
                  </span>
                  {option.value !== option.name && (
                    <span className="block truncate font-mono text-caption text-fg-2">
                      {option.value}
                    </span>
                  )}
                </span>
              </CommandItem>
            ))}
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  );
}
