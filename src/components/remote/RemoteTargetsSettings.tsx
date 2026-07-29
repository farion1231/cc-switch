import { useEffect, useMemo, useState } from "react";
import {
  ChevronDown,
  CirclePlus,
  LoaderCircle,
  Pencil,
  PlugZap,
  Server,
  Trash2,
  X,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { Button } from "@/components/ui/button";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useRuntimeTarget } from "@/contexts/RuntimeTargetContext";
import { remoteApi } from "@/lib/api/remote";
import type {
  DiscoveredSshTarget,
  RemoteTargetConfig,
} from "@/lib/runtime/types";
import { extractErrorMessage } from "@/utils/errorUtils";

interface TargetFormState {
  id: string;
  name: string;
  hostAlias: string;
  username: string;
  port: string;
  identityFile: string;
}

const EMPTY_FORM: TargetFormState = {
  id: "",
  name: "",
  hostAlias: "",
  username: "",
  port: "",
  identityFile: "",
};

function toForm(target?: RemoteTargetConfig): TargetFormState {
  if (!target) return EMPTY_FORM;
  return {
    id: target.id,
    name: target.name,
    hostAlias: target.hostAlias,
    username: target.username ?? "",
    port: target.port?.toString() ?? "",
    identityFile: target.identityFile ?? "",
  };
}

function toTarget(form: TargetFormState): RemoteTargetConfig {
  const port = form.port.trim() ? Number(form.port) : undefined;
  return {
    // 草稿 ID 只在保存时生成；测试连接使用固定临时 ID，不会落盘。
    id: form.id || crypto.randomUUID(),
    name: form.name.trim(),
    hostAlias: form.hostAlias.trim(),
    username: form.username.trim() || undefined,
    port,
    identityFile: form.identityFile.trim() || undefined,
  };
}

export function RemoteTargetsSettings() {
  const { t } = useTranslation();
  const { snapshot, targets, upsertTarget, deleteTarget, setActiveTarget } =
    useRuntimeTarget();
  const [dialogOpen, setDialogOpen] = useState(false);
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const [form, setForm] = useState<TargetFormState>(EMPTY_FORM);
  const [saving, setSaving] = useState(false);
  const [testing, setTesting] = useState(false);
  const [discovering, setDiscovering] = useState(true);
  const [discoveredTargets, setDiscoveredTargets] = useState<
    DiscoveredSshTarget[]
  >([]);
  const [discoveryError, setDiscoveryError] = useState<string>();
  const [connectingId, setConnectingId] = useState<string>();
  const [deleteCandidate, setDeleteCandidate] = useState<RemoteTargetConfig>();

  const valid = useMemo(() => {
    const port = form.port.trim();
    const portValid =
      !port ||
      (/^\d+$/.test(port) && Number(port) >= 1 && Number(port) <= 65535);
    return Boolean(form.name.trim() && form.hostAlias.trim() && portValid);
  }, [form]);

  const importableTargets = useMemo(() => {
    const savedAliases = new Set(
      targets.map((target) => target.hostAlias.toLocaleLowerCase()),
    );
    return discoveredTargets.filter(
      (target) => !savedAliases.has(target.hostAlias.toLocaleLowerCase()),
    );
  }, [discoveredTargets, targets]);

  useEffect(() => {
    let cancelled = false;
    // 设置页加载时主动读取本机 SSH 配置，打开弹窗后无需用户再执行扫描动作。
    void remoteApi
      .discoverTargets()
      .then((items) => {
        if (cancelled) return;
        setDiscoveredTargets(items);
        setDiscoveryError(undefined);
      })
      .catch((error) => {
        if (cancelled) return;
        setDiscoveryError(extractErrorMessage(error));
      })
      .finally(() => {
        if (!cancelled) setDiscovering(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const openForm = (target?: RemoteTargetConfig) => {
    setForm(toForm(target));
    setAdvancedOpen(
      Boolean(target?.username || target?.port || target?.identityFile),
    );
    setDialogOpen(true);
  };

  const updateField = (field: keyof TargetFormState, value: string) => {
    setForm((current) => ({ ...current, [field]: value }));
  };

  const importDiscoveredTarget = (target: DiscoveredSshTarget) => {
    // HostAlias 保留配置中的别名，让 ProxyJump、证书和其他 OpenSSH 规则继续生效；
    // 已解析出的常用字段作为可编辑覆盖项预填，用户保存前仍可调整。
    setForm({
      id: "",
      name: target.name,
      hostAlias: target.hostAlias,
      username: target.username ?? "",
      port: target.port?.toString() ?? "",
      identityFile: target.identityFile ?? "",
    });
    setAdvancedOpen(
      Boolean(target.username || target.port || target.identityFile),
    );
  };

  const handleSave = async () => {
    if (!valid) return;
    setSaving(true);
    try {
      await upsertTarget(toTarget(form));
      setDialogOpen(false);
      toast.success(t("remote.saved", { defaultValue: "Server saved" }));
    } catch (error) {
      toast.error(extractErrorMessage(error));
    } finally {
      setSaving(false);
    }
  };

  const handleTest = async () => {
    if (!valid) return;
    setTesting(true);
    try {
      const target = toTarget({ ...form, id: form.id || "connection-test" });
      const platform = await remoteApi.testTarget(target);
      toast.success(
        t("remote.testSucceeded", {
          defaultValue: `Connected: ${platform.os} ${platform.architecture}`,
          os: platform.os,
          architecture: platform.architecture,
        }),
      );
    } catch (error) {
      toast.error(extractErrorMessage(error));
    } finally {
      setTesting(false);
    }
  };

  const handleConnect = async (targetId: string) => {
    setConnectingId(targetId);
    try {
      await setActiveTarget(targetId);
    } finally {
      setConnectingId(undefined);
    }
  };

  return (
    <section className="space-y-5" aria-labelledby="remote-targets-title">
      <div className="flex items-start justify-between gap-4 border-b border-border/60 pb-4">
        <div className="min-w-0">
          <h2 id="remote-targets-title" className="text-base font-semibold">
            {t("remote.title", { defaultValue: "Remote servers" })}
          </h2>
          <p className="mt-1 text-sm text-muted-foreground">
            {t("remote.description", {
              defaultValue: "Use your local OpenSSH configuration and keys.",
            })}
          </p>
        </div>
        <Button size="sm" onClick={() => openForm()}>
          <CirclePlus className="mr-2 h-4 w-4" />
          {t("remote.add", { defaultValue: "Add server" })}
        </Button>
      </div>

      {targets.length === 0 ? (
        <div className="flex min-h-40 flex-col items-center justify-center border border-dashed border-border px-6 text-center">
          <Server className="mb-3 h-8 w-8 text-muted-foreground" />
          <p className="text-sm font-medium">
            {t("remote.empty", { defaultValue: "No remote servers" })}
          </p>
          <p className="mt-1 text-sm text-muted-foreground">
            {t("remote.emptyHint", {
              defaultValue: "Add an SSH Host from your OpenSSH configuration.",
            })}
          </p>
        </div>
      ) : (
        <div className="divide-y divide-border/60 border-y border-border/60">
          {targets.map((target) => {
            const active = snapshot.activeTargetId === target.id;
            const connecting = connectingId === target.id;
            return (
              <div
                key={target.id}
                className="flex min-h-16 items-center gap-3 py-3"
              >
                <Server className="h-5 w-5 shrink-0 text-muted-foreground" />
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2">
                    <p className="truncate text-sm font-medium">
                      {target.name}
                    </p>
                    {active && (
                      <span className="inline-flex items-center gap-1 text-xs text-emerald-600 dark:text-emerald-400">
                        <span className="h-1.5 w-1.5 rounded-full bg-emerald-500" />
                        {t("remote.active", { defaultValue: "Active" })}
                      </span>
                    )}
                  </div>
                  <p className="truncate text-xs text-muted-foreground">
                    {target.username ? `${target.username}@` : ""}
                    {target.hostAlias}
                    {target.port ? `:${target.port}` : ""}
                  </p>
                </div>
                <div className="flex shrink-0 items-center gap-1">
                  <Button
                    variant="ghost"
                    size="icon"
                    disabled={active || connecting}
                    onClick={() => void handleConnect(target.id)}
                    aria-label={t("remote.connectNamed", {
                      defaultValue: `Connect ${target.name}`,
                      name: target.name,
                    })}
                    title={t("remote.connect", { defaultValue: "Connect" })}
                  >
                    {connecting ? (
                      <LoaderCircle className="h-4 w-4 animate-spin" />
                    ) : (
                      <PlugZap className="h-4 w-4" />
                    )}
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon"
                    onClick={() => openForm(target)}
                    aria-label={t("remote.editNamed", {
                      defaultValue: `Edit ${target.name}`,
                      name: target.name,
                    })}
                    title={t("common.edit", { defaultValue: "Edit" })}
                  >
                    <Pencil className="h-4 w-4" />
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon"
                    onClick={() => setDeleteCandidate(target)}
                    aria-label={t("remote.deleteNamed", {
                      defaultValue: `Delete ${target.name}`,
                      name: target.name,
                    })}
                    title={t("common.delete", { defaultValue: "Delete" })}
                  >
                    <Trash2 className="h-4 w-4 text-destructive" />
                  </Button>
                </div>
              </div>
            );
          })}
        </div>
      )}

      <Dialog open={dialogOpen} onOpenChange={setDialogOpen}>
        <DialogContent className="max-w-lg overflow-hidden">
          <DialogClose asChild>
            <Button
              variant="ghost"
              size="icon"
              className="absolute right-4 top-4 z-10"
              aria-label={t("common.close", { defaultValue: "Close" })}
              title={t("common.close", { defaultValue: "Close" })}
            >
              <X className="h-4 w-4" />
            </Button>
          </DialogClose>
          <DialogHeader className="pr-16">
            <DialogTitle>
              {form.id
                ? t("remote.edit", { defaultValue: "Edit server" })
                : t("remote.add", { defaultValue: "Add server" })}
            </DialogTitle>
            <DialogDescription>
              {t("remote.formDescription", {
                defaultValue:
                  "Connection details stay on this computer and use OpenSSH authentication.",
              })}
            </DialogDescription>
          </DialogHeader>
          <div className="min-h-0 space-y-4 overflow-y-auto px-6 py-5">
            {!form.id && (
              <div className="space-y-2 border-b border-border/60 pb-4">
                <p className="text-sm font-medium">
                  {t("remote.discovery.title", {
                    defaultValue: "Servers in SSH config",
                  })}
                </p>
                {discovering ? (
                  <div className="flex h-12 items-center justify-center text-muted-foreground">
                    <LoaderCircle className="h-4 w-4 animate-spin" />
                  </div>
                ) : discoveryError ? (
                  <p className="text-sm text-destructive">{discoveryError}</p>
                ) : importableTargets.length === 0 ? (
                  <p className="text-sm text-muted-foreground">
                    {t("remote.discovery.empty", {
                      defaultValue: "No other configured servers found.",
                    })}
                  </p>
                ) : (
                  <div className="max-h-40 divide-y divide-border/60 overflow-y-auto border-y border-border/60">
                    {importableTargets.map((target) => {
                      const endpoint = `${target.username ? `${target.username}@` : ""}${target.hostname ?? target.hostAlias}${target.port ? `:${target.port}` : ""}`;
                      return (
                        <div
                          key={target.hostAlias.toLocaleLowerCase()}
                          className="flex min-h-12 items-center gap-3 py-2"
                        >
                          <Server className="h-4 w-4 shrink-0 text-muted-foreground" />
                          <div className="min-w-0 flex-1">
                            <p className="truncate text-sm font-medium">
                              {target.hostAlias}
                            </p>
                            <p className="truncate text-xs text-muted-foreground">
                              {endpoint}
                            </p>
                          </div>
                          <Button
                            variant="ghost"
                            size="sm"
                            onClick={() => importDiscoveredTarget(target)}
                            aria-label={t("remote.discovery.useNamed", {
                              defaultValue: `Use ${target.name}`,
                              name: target.name,
                            })}
                          >
                            <CirclePlus className="h-4 w-4" />
                            {t("remote.discovery.use", {
                              defaultValue: "Use",
                            })}
                          </Button>
                        </div>
                      );
                    })}
                  </div>
                )}
              </div>
            )}
            <div className="space-y-2">
              <Label htmlFor="remote-name">
                {t("remote.fields.name", { defaultValue: "Name" })}
              </Label>
              <Input
                id="remote-name"
                value={form.name}
                onChange={(event) => updateField("name", event.target.value)}
                autoComplete="off"
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="remote-host">
                {t("remote.fields.hostAlias", { defaultValue: "SSH Host" })}
              </Label>
              <Input
                id="remote-host"
                value={form.hostAlias}
                onChange={(event) =>
                  updateField("hostAlias", event.target.value)
                }
                placeholder="production"
                autoComplete="off"
              />
              <p className="text-xs text-muted-foreground">
                {t("remote.fields.hostAliasHint", {
                  defaultValue: "Matches a Host entry in ~/.ssh/config.",
                })}
              </p>
            </div>

            <Collapsible open={advancedOpen} onOpenChange={setAdvancedOpen}>
              <CollapsibleTrigger asChild>
                <Button
                  variant="ghost"
                  className="w-full justify-between px-0 hover:bg-transparent"
                >
                  {t("remote.advanced", { defaultValue: "Advanced options" })}
                  <ChevronDown
                    className={`h-4 w-4 transition-transform ${advancedOpen ? "rotate-180" : ""}`}
                  />
                </Button>
              </CollapsibleTrigger>
              <CollapsibleContent className="space-y-4 border-t border-border/60 pt-4">
                <div className="grid grid-cols-[minmax(0,1fr)_8rem] gap-3">
                  <div className="space-y-2">
                    <Label htmlFor="remote-username">
                      {t("remote.fields.username", {
                        defaultValue: "Username",
                      })}
                    </Label>
                    <Input
                      id="remote-username"
                      value={form.username}
                      onChange={(event) =>
                        updateField("username", event.target.value)
                      }
                      autoComplete="off"
                    />
                  </div>
                  <div className="space-y-2">
                    <Label htmlFor="remote-port">
                      {t("remote.fields.port", { defaultValue: "Port" })}
                    </Label>
                    <Input
                      id="remote-port"
                      type="number"
                      min={1}
                      max={65535}
                      value={form.port}
                      onChange={(event) =>
                        updateField("port", event.target.value)
                      }
                      placeholder="22"
                    />
                  </div>
                </div>
                <div className="space-y-2">
                  <Label htmlFor="remote-key">
                    {t("remote.fields.identityFile", {
                      defaultValue: "Private key",
                    })}
                  </Label>
                  <Input
                    id="remote-key"
                    value={form.identityFile}
                    onChange={(event) =>
                      updateField("identityFile", event.target.value)
                    }
                    placeholder="~/.ssh/id_ed25519"
                    autoComplete="off"
                  />
                </div>
              </CollapsibleContent>
            </Collapsible>
          </div>
          <DialogFooter>
            <Button
              variant="outline"
              onClick={() => void handleTest()}
              disabled={!valid || testing || saving}
            >
              {testing && (
                <LoaderCircle className="mr-2 h-4 w-4 animate-spin" />
              )}
              {t("remote.test", { defaultValue: "Test connection" })}
            </Button>
            <Button
              onClick={() => void handleSave()}
              disabled={!valid || testing || saving}
            >
              {saving && <LoaderCircle className="mr-2 h-4 w-4 animate-spin" />}
              {t("common.save", { defaultValue: "Save" })}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <ConfirmDialog
        isOpen={Boolean(deleteCandidate)}
        title={t("remote.deleteTitle", { defaultValue: "Delete server" })}
        message={t("remote.deleteMessage", {
          defaultValue: `Delete ${deleteCandidate?.name ?? ""}?`,
          name: deleteCandidate?.name ?? "",
        })}
        confirmText={t("common.delete", { defaultValue: "Delete" })}
        onCancel={() => setDeleteCandidate(undefined)}
        onConfirm={() => {
          if (!deleteCandidate) return;
          // 删除完成前保留候选项，失败时确认框仍可给用户明确上下文。
          void deleteTarget(deleteCandidate.id)
            .then(() => setDeleteCandidate(undefined))
            .catch((error) => toast.error(extractErrorMessage(error)));
        }}
      />
    </section>
  );
}
