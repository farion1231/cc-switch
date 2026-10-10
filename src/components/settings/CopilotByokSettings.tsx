import {
  forwardRef,
  useEffect,
  useImperativeHandle,
  useMemo,
  useRef,
  useState,
} from "react";
import { createPortal } from "react-dom";
import { AnimatePresence, motion } from "framer-motion";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { Plus, RefreshCw, Search, X } from "lucide-react";
import { toast } from "@/lib/toast";
import { cn } from "@/lib/utils";
import { isTextEditableTarget } from "@/utils/domUtils";
import { extractErrorMessage } from "@/utils/errorUtils";
import {
  copilotByokApi,
  type CopilotByokGroup,
  type CopilotByokModel,
} from "@/lib/api/copilotByok";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { ProviderIconBox } from "@/components/ProviderIconBox";
import { ProviderCardActions } from "@/components/providers/ProviderCardActions";
import type { CardPresentation } from "@/components/providers/presentation";
import ApiKeyInput from "@/components/providers/forms/ApiKeyInput";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input, fieldClass } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { HelpTip } from "@/components/ui/help-tip";
import { Notice, NoticeSlot } from "@/components/ui/notice";

const stateKey = ["copilot-byok", "state"];

function newModel(): CopilotByokModel {
  return {
    id: crypto.randomUUID(),
    modelId: "",
    name: "",
    enabled: true,
    toolCalling: true,
    vision: false,
    contextWindow: 128000,
    maxOutputTokens: 8192,
    extra: {},
  };
}

function newGroup(): CopilotByokGroup {
  return {
    id: crypto.randomUUID(),
    name: "",
    url: "",
    apiKey: "",
    apiType: "chat-completions",
    enabled: true,
    requestHeaders: {},
    models: [newModel()],
    extra: {},
  };
}

export interface CopilotByokSettingsHandle {
  openAdd: () => void;
}

interface Props {
  mode?: "catalog" | "targets";
  onOpenWebsite?: (url: string) => void;
  onOpenTargets?: () => void;
  searchOpen?: boolean;
  onSearchOpenChange?: (open: boolean) => void;
}

export const CopilotByokSettings = forwardRef<CopilotByokSettingsHandle, Props>(
  function CopilotByokSettings(
    {
      mode = "catalog",
      onOpenWebsite,
      onOpenTargets,
      searchOpen,
      onSearchOpenChange,
    },
    ref,
  ) {
    const { t } = useTranslation();
    const queryClient = useQueryClient();
    const query = useQuery({
      queryKey: stateKey,
      queryFn: copilotByokApi.getState,
    });
    const [editing, setEditing] = useState<CopilotByokGroup | null>(null);
    const [customPath, setCustomPath] = useState("");
    const [searchTerm, setSearchTerm] = useState("");
    const [internalSearchOpen, setInternalSearchOpen] = useState(false);
    const isSearchOpen = searchOpen ?? internalSearchOpen;
    const setIsSearchOpen = onSearchOpenChange ?? setInternalSearchOpen;
    const searchInputRef = useRef<HTMLInputElement>(null);
    const [errorKey, setErrorKey] = useState("copilotByok.errors.save");
    const [confirmation, setConfirmation] = useState<{
      title: string;
      message: string;
      action: () => Promise<unknown>;
    } | null>(null);
    useEffect(() => {
      if (mode !== "catalog" || editing || confirmation) return;
      const handleKeyDown = (event: KeyboardEvent) => {
        if (event.defaultPrevented) return;
        const key = event.key.toLowerCase();
        if ((event.metaKey || event.ctrlKey) && key === "f") {
          if (isTextEditableTarget(document.activeElement)) return;
          event.preventDefault();
          setIsSearchOpen(true);
        } else if (key === "escape") {
          setIsSearchOpen(false);
        }
      };
      globalThis.addEventListener("keydown", handleKeyDown);
      return () => globalThis.removeEventListener("keydown", handleKeyDown);
    }, [mode, editing, confirmation, setIsSearchOpen]);
    useEffect(() => {
      if (
        !isSearchOpen ||
        query.isPending ||
        mode !== "catalog" ||
        editing ||
        confirmation
      )
        return;
      const frame = requestAnimationFrame(() => {
        searchInputRef.current?.focus();
        searchInputRef.current?.select();
      });
      return () => cancelAnimationFrame(frame);
    }, [isSearchOpen, query.isPending, mode, editing, confirmation]);
    const filteredGroups = useMemo(() => {
      const keyword = searchTerm.trim().toLowerCase();
      return (query.data?.groups ?? []).filter(
        (group) =>
          !keyword ||
          [group.name, group.notes, group.websiteUrl, group.url].some((value) =>
            value?.toLowerCase().includes(keyword),
          ),
      );
    }, [query.data?.groups, searchTerm]);
    const mutation = useMutation({
      mutationFn: (action: () => Promise<unknown>) => action(),
      onSuccess: async () => {
        await queryClient.invalidateQueries({ queryKey: stateKey });
        toast.success(t("copilotByok.saved"));
      },
      onError: (error) =>
        toast.error(t(errorKey), { description: extractErrorMessage(error) }),
    });
    const busy = mutation.isPending || query.isPending;
    useImperativeHandle(
      ref,
      () => ({
        openAdd: () => {
          if (!busy) setEditing(newGroup());
        },
      }),
      [busy],
    );

    const run = async (action: () => Promise<unknown>, operation = "save") => {
      if (busy) return false;
      setErrorKey(`copilotByok.errors.${operation}`);
      try {
        await mutation.mutateAsync(action);
        return true;
      } catch {
        return false;
      }
    };

    if (query.isPending) return <p role="status">{t("common.loading")}</p>;
    const state = query.data;
    const error = mutation.error ?? query.error;
    const sections = [
      { key: "added", groups: filteredGroups.filter((group) => group.enabled) },
      {
        key: "available",
        groups: filteredGroups.filter((group) => !group.enabled),
      },
    ];

    return (
      <div className="space-y-4">
        {error && (
          <Alert variant="destructive">
            <AlertTitle>
              {t(query.error ? "copilotByok.errors.load" : errorKey)}
            </AlertTitle>
            <AlertDescription>{extractErrorMessage(error)}</AlertDescription>
            <Button
              variant="quiet"
              onClick={() => {
                mutation.reset();
                void query.refetch();
              }}
            >
              {t("common.retry")}
            </Button>
          </Alert>
        )}
        {mode === "catalog" ? (
          <>
            {createPortal(
              <AnimatePresence>
                {isSearchOpen && !editing && !confirmation && (
                  <motion.div
                    key="copilot-provider-search"
                    initial={{ opacity: 0, y: -8, scale: 0.98 }}
                    animate={{ opacity: 1, y: 0, scale: 1 }}
                    exit={{ opacity: 0, y: -8, scale: 0.98 }}
                    transition={{ duration: 0.18, ease: "easeOut" }}
                    className="fixed end-6 top-[6.5rem] z-40 w-[min(90vw,26rem)]"
                  >
                    <div className="space-y-3 rounded-panel border border-border bg-surface p-4 shadow-v7-lg">
                      <div className="relative flex items-center gap-2">
                        <Search className="pointer-events-none absolute start-3 top-1/2 h-4 w-4 -translate-y-1/2 text-fg-3" />
                        <Input
                          ref={searchInputRef}
                          value={searchTerm}
                          onChange={(event) =>
                            setSearchTerm(event.target.value)
                          }
                          placeholder={t("provider.searchPlaceholder")}
                          aria-label={t("provider.searchAriaLabel")}
                          className="pe-16 ps-9"
                        />
                        {searchTerm && (
                          <Button
                            variant="quiet"
                            size="compact"
                            className="absolute end-11 top-1/2 -translate-y-1/2"
                            onClick={() => setSearchTerm("")}
                          >
                            {t("common.clear")}
                          </Button>
                        )}
                        <Button
                          variant="quiet"
                          size="icon-compact"
                          className="ms-auto"
                          onClick={() => setIsSearchOpen(false)}
                          aria-label={t("provider.searchCloseAriaLabel")}
                        >
                          <X className="h-4 w-4" />
                        </Button>
                      </div>
                      <div className="flex flex-wrap items-center justify-between gap-2 text-caption text-fg-3">
                        <span>{t("provider.searchScopeHint")}</span>
                        <span>{t("provider.searchCloseHint")}</span>
                      </div>
                    </div>
                  </motion.div>
                )}
              </AnimatePresence>,
              document.body,
            )}
            <div className="flex items-center justify-between gap-2">
              <span className="text-caption text-fg-3">
                {t("copilotByok.modelCount", {
                  count: state?.managedModelCount ?? 0,
                })}
              </span>
              <Button
                variant="quiet"
                disabled={busy || !state?.selectedTargetIds.length}
                onClick={() => void run(copilotByokApi.sync, "sync")}
              >
                <RefreshCw className="h-4 w-4" />
                {t("copilotByok.sync")}
              </Button>
            </div>
            <NoticeSlot>
              {!state?.selectedTargetIds.length && (
                <Notice
                  title={t("copilotByok.noTargets")}
                  actions={
                    onOpenTargets && (
                      <Button
                        variant="neutral"
                        size="compact"
                        onClick={onOpenTargets}
                      >
                        {t("copilotByok.targets")}
                      </Button>
                    )
                  }
                />
              )}
            </NoticeSlot>
            {!state ? null : state.groups.length === 0 ? (
              <div className="flex flex-col items-center justify-center px-6 py-16 text-center">
                <h2 className="text-section text-fg-1">
                  {t("provider.noProviders")}
                </h2>
                <p className="mt-2 max-w-lg text-body text-fg-2">
                  {t("copilotByok.empty")}
                </p>
                <Button
                  variant="neutral"
                  className="mt-5"
                  disabled={busy}
                  onClick={() => setEditing(newGroup())}
                >
                  <Plus className="h-4 w-4" />
                  {t("provider.addProvider")}
                </Button>
              </div>
            ) : filteredGroups.length === 0 ? (
              <div className="rounded-panel border border-dashed border-border px-6 py-8 text-center text-body text-fg-2">
                {t("provider.noSearchResults")}
              </div>
            ) : (
              sections.map((section) => (
                <section
                  key={section.key}
                  aria-labelledby={`copilot-section-${section.key}`}
                  className="space-y-2"
                >
                  <div className="flex items-center gap-1 pt-1 text-caption font-semibold text-fg-2">
                    <h2
                      id={`copilot-section-${section.key}`}
                      className="m-0 text-caption font-semibold"
                    >
                      {t(`providerCard.section.${section.key}`, {
                        count: section.groups.length,
                      })}
                    </h2>
                    {section.key === "added" && (
                      <HelpTip title={t("providerCard.section.addedHelpTitle")}>
                        {t("copilotByok.addedProvidersHelp")}
                      </HelpTip>
                    )}
                  </div>
                  <div className="space-y-2">
                    {section.groups.map((group) => (
                      <CopilotGroupCard
                        key={group.id}
                        group={group}
                        busy={busy}
                        onOpenWebsite={onOpenWebsite}
                        onToggle={() =>
                          void run(() =>
                            copilotByokApi.upsertGroup({
                              ...group,
                              enabled: !group.enabled,
                            }),
                          )
                        }
                        onEdit={() => setEditing(structuredClone(group))}
                        onDelete={() =>
                          setConfirmation({
                            title: t("copilotByok.deleteTitle"),
                            message: t("copilotByok.deleteMessage", {
                              name: group.name,
                            }),
                            action: () => copilotByokApi.deleteGroup(group.id),
                          })
                        }
                      />
                    ))}
                  </div>
                </section>
              ))
            )}
          </>
        ) : (
          <>
            <p className="text-caption text-fg-3">
              {t("copilotByok.targetsDescription")}
            </p>
            {state?.targets.map((target) => (
              <div
                key={target.id}
                className="space-y-2 rounded-lg border border-border p-4"
              >
                <label className="flex items-center gap-3">
                  <Checkbox
                    checked={target.selected}
                    disabled={busy}
                    onCheckedChange={(checked) =>
                      void run(
                        () =>
                          copilotByokApi.setTargets(
                            checked
                              ? [...(state?.selectedTargetIds ?? []), target.id]
                              : (state?.selectedTargetIds ?? []).filter(
                                  (id) => id !== target.id,
                                ),
                          ),
                        "targets",
                      )
                    }
                  />
                  <span>
                    {target.editionName ? `${target.editionName} — ` : ""}
                    {target.isDefault
                      ? t("copilotByok.defaultProfile")
                      : target.profileName || t("copilotByok.customTarget")}
                  </span>
                </label>
                <p className="break-all text-caption text-fg-3">
                  {target.languageModelsPath}
                </p>
                {target.readError && (
                  <p role="alert" className="text-caption text-destructive">
                    {target.readError}
                  </p>
                )}
                <div className="flex gap-2">
                  {target.source === "custom" && (
                    <Button
                      variant="quiet"
                      disabled={busy}
                      onClick={() =>
                        setConfirmation({
                          title: t("copilotByok.removeTarget"),
                          message: t("copilotByok.removeTargetMessage"),
                          action: () =>
                            copilotByokApi.removeCustomTarget(target.id),
                        })
                      }
                    >
                      {t("copilotByok.removeTarget")}
                    </Button>
                  )}
                  {target.backupExists && (
                    <Button
                      variant="quiet"
                      disabled={busy}
                      onClick={() =>
                        setConfirmation({
                          title: t("copilotByok.restore"),
                          message: t("copilotByok.restoreMessage"),
                          action: () => copilotByokApi.restoreBackup(target.id),
                        })
                      }
                    >
                      {t("copilotByok.restore")}
                    </Button>
                  )}
                </div>
              </div>
            ))}
            <form
              className="space-y-2"
              onSubmit={async (event) => {
                event.preventDefault();
                if (
                  await run(
                    () => copilotByokApi.addCustomTarget(customPath),
                    "addTarget",
                  )
                )
                  setCustomPath("");
              }}
            >
              <Label htmlFor="copilot-custom-path">
                {t("copilotByok.customPath")}
              </Label>
              <div className="flex gap-2">
                <Input
                  id="copilot-custom-path"
                  required
                  value={customPath}
                  disabled={busy}
                  onChange={(event) => setCustomPath(event.target.value)}
                />
                <Button type="submit" disabled={busy || !customPath.trim()}>
                  {t("copilotByok.addTarget")}
                </Button>
              </div>
            </form>
          </>
        )}
        {editing && (
          <GroupEditor
            key={editing.id}
            group={editing}
            busy={busy}
            onClose={() => setEditing(null)}
            onSave={async (group) => {
              if (await run(() => copilotByokApi.upsertGroup(group)))
                setEditing(null);
            }}
          />
        )}
        <ConfirmDialog
          isOpen={!!confirmation}
          title={confirmation?.title ?? ""}
          message={confirmation?.message ?? ""}
          pending={busy}
          onCancel={() => setConfirmation(null)}
          onConfirm={() => {
            if (confirmation)
              void run(confirmation.action).then((ok) => {
                if (ok) setConfirmation(null);
              });
          }}
        />
      </div>
    );
  },
);

function CopilotGroupCard({
  group,
  busy,
  onToggle,
  onEdit,
  onDelete,
  onOpenWebsite,
}: {
  group: CopilotByokGroup;
  busy: boolean;
  onToggle: () => void;
  onEdit: () => void;
  onDelete: () => void;
  onOpenWebsite?: (url: string) => void;
}) {
  const { t } = useTranslation();
  const description =
    group.notes?.trim() || group.websiteUrl?.trim() || group.url;
  const candidateLink = group.notes?.trim()
    ? undefined
    : group.websiteUrl?.trim() || group.url;
  const link =
    candidateLink && /^https?:\/\//i.test(candidateLink)
      ? candidateLink
      : undefined;
  const presentation: CardPresentation = {
    buttons: [
      {
        key: group.enabled ? "remove" : "add",
        label: t(
          group.enabled ? "provider.removeFromConfig" : "provider.addToConfig",
        ),
        onClick: onToggle,
        disabledReason: busy ? t("common.loading") : undefined,
      },
    ],
    chips: [],
    editDisabledReason: busy ? t("common.loading") : undefined,
    deleteDisabledReason: busy ? t("common.loading") : undefined,
  };
  return (
    <div
      role="group"
      aria-label={group.name}
      className={cn(
        "group relative rounded-panel border transition-[border-color,box-shadow,background-color] duration-150",
        group.enabled
          ? "border-border-strong bg-subtle"
          : "border-border bg-surface hover:border-border-strong hover:shadow-v7-sm",
      )}
    >
      <div className="flex min-h-[58px] items-center gap-3 py-2.5 pe-3 ps-2">
        <span className="w-4 shrink-0" />
        <ProviderIconBox
          icon={group.icon ?? undefined}
          name={group.name}
          color={group.iconColor ?? undefined}
          className="bg-white text-neutral-900"
          fallbackClassName="bg-transparent text-neutral-600"
        />
        <div className="min-w-0 flex-1">
          <h3
            className="m-0 min-w-0 truncate text-strong text-fg-1"
            title={group.name}
          >
            {group.name}
          </h3>
          {onOpenWebsite && link ? (
            <button
              type="button"
              onClick={() => onOpenWebsite(link)}
              className="mt-0.5 inline-flex max-w-full cursor-pointer items-center overflow-hidden text-start text-caption text-fg-2 decoration-border-strong underline-offset-[3px] hover:underline"
              title={description}
            >
              <span className="min-w-0 truncate">{description}</span>
            </button>
          ) : (
            <p
              className="mt-0.5 truncate text-caption text-fg-2"
              title={description}
            >
              {description}
            </p>
          )}
        </div>
        <ProviderCardActions
          providerName={group.name}
          presentation={presentation}
          onEdit={onEdit}
          onDelete={onDelete}
        />
      </div>
    </div>
  );
}

function GroupEditor({
  group,
  busy,
  onClose,
  onSave,
}: {
  group: CopilotByokGroup;
  busy: boolean;
  onClose: () => void;
  onSave: (group: CopilotByokGroup) => Promise<void>;
}) {
  const { t } = useTranslation();
  const [draft, setDraft] = useState(group);
  const [confirmClose, setConfirmClose] = useState(false);
  const patchModel = (id: string, patch: Partial<CopilotByokModel>) =>
    setDraft((current) => ({
      ...current,
      models: current.models.map((model) =>
        model.id === id ? { ...model, ...patch } : model,
      ),
    }));
  return (
    <FullScreenPanel
      isOpen
      title={t("copilotByok.editorTitle")}
      onClose={() => {
        if (busy) return;
        if (JSON.stringify(draft) !== JSON.stringify(group))
          setConfirmClose(true);
        else onClose();
      }}
      trackUnsavedChanges
      actions={
        <Button type="submit" form="copilot-byok-form" disabled={busy}>
          {t("common.save")}
        </Button>
      }
    >
      <ConfirmDialog
        isOpen={confirmClose}
        title={t("common.unsavedLeaveTitle")}
        message={t("common.unsavedLeaveMessage")}
        confirmText={t("common.unsavedLeaveConfirm")}
        cancelText={t("common.unsavedLeaveCancel")}
        onCancel={() => setConfirmClose(false)}
        onConfirm={onClose}
      />
      <form
        id="copilot-byok-form"
        className="space-y-5"
        onSubmit={(event) => {
          event.preventDefault();
          void onSave(draft);
        }}
      >
        <fieldset disabled={busy} className="space-y-5">
          <div className="space-y-2">
            <Label htmlFor="copilot-group-name">{t("copilotByok.name")}</Label>
            <Input
              id="copilot-group-name"
              required
              value={draft.name}
              onChange={(event) =>
                setDraft({ ...draft, name: event.target.value })
              }
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="copilot-group-url">{t("copilotByok.url")}</Label>
            <Input
              id="copilot-group-url"
              required
              type="url"
              value={draft.url}
              onChange={(event) =>
                setDraft({ ...draft, url: event.target.value })
              }
            />
          </div>
          <ApiKeyInput
            id="copilot-group-key"
            label={t("copilotByok.apiKey")}
            value={draft.apiKey}
            onChange={(apiKey) => setDraft({ ...draft, apiKey })}
          />
          <div className="space-y-2">
            <Label htmlFor="copilot-group-protocol">
              {t("copilotByok.protocol")}
            </Label>
            <select
              id="copilot-group-protocol"
              className={fieldClass}
              value={draft.apiType}
              onChange={(event) =>
                setDraft({
                  ...draft,
                  apiType: event.target.value as CopilotByokGroup["apiType"],
                })
              }
            >
              {(["chat-completions", "responses", "messages"] as const).map(
                (protocol) => (
                  <option key={protocol} value={protocol}>
                    {t(`copilotByok.protocols.${protocol}`)}
                  </option>
                ),
              )}
            </select>
          </div>
          {draft.models.map((model) => (
            <div
              key={model.id}
              className="space-y-3 rounded-lg border border-border p-4"
            >
              <label className="flex items-center gap-2">
                <Checkbox
                  checked={model.enabled}
                  onCheckedChange={(enabled) =>
                    patchModel(model.id, { enabled })
                  }
                />
                {t("copilotByok.enabled")}
              </label>
              <div className="space-y-2">
                <Label htmlFor={`model-id-${model.id}`}>
                  {t("copilotByok.modelId")}
                </Label>
                <Input
                  id={`model-id-${model.id}`}
                  required
                  value={model.modelId}
                  onChange={(event) =>
                    patchModel(model.id, { modelId: event.target.value })
                  }
                />
              </div>
              <div className="space-y-2">
                <Label htmlFor={`model-name-${model.id}`}>
                  {t("copilotByok.modelName")}
                </Label>
                <Input
                  id={`model-name-${model.id}`}
                  required
                  value={model.name}
                  onChange={(event) =>
                    patchModel(model.id, { name: event.target.value })
                  }
                />
              </div>
              <div className="grid grid-cols-2 gap-3">
                {(["contextWindow", "maxOutputTokens"] as const).map((key) => (
                  <div key={key} className="space-y-2">
                    <Label htmlFor={`${key}-${model.id}`}>
                      {t(`copilotByok.${key}`)}
                    </Label>
                    <Input
                      id={`${key}-${model.id}`}
                      type="number"
                      min={1}
                      step={1}
                      value={model[key] ?? ""}
                      onChange={(event) =>
                        patchModel(model.id, {
                          [key]: event.target.value
                            ? Number(event.target.value)
                            : null,
                        })
                      }
                    />
                  </div>
                ))}
              </div>
              <div className="flex gap-4">
                {(["toolCalling", "vision"] as const).map((key) => (
                  <label key={key} className="flex items-center gap-2">
                    <Checkbox
                      checked={model[key] ?? false}
                      onCheckedChange={(value) =>
                        patchModel(model.id, { [key]: value })
                      }
                    />
                    {t(`copilotByok.${key}`)}
                  </label>
                ))}
              </div>
              <Button
                variant="quiet"
                type="button"
                data-unsaved-edit
                disabled={draft.models.length <= 1}
                onClick={() => {
                  setDraft({
                    ...draft,
                    models: draft.models.filter((item) => item.id !== model.id),
                  });
                }}
              >
                {t("copilotByok.removeModel")}
              </Button>
            </div>
          ))}
          <Button
            type="button"
            variant="quiet"
            data-unsaved-edit
            onClick={() => {
              setDraft({ ...draft, models: [...draft.models, newModel()] });
            }}
          >
            <Plus className="h-4 w-4" />
            {t("copilotByok.addModel")}
          </Button>
        </fieldset>
      </form>
    </FullScreenPanel>
  );
}
