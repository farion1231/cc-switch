import { useId, useState } from "react";
import { RefreshCw, RotateCcw, SlidersHorizontal, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

import {
  WorkerModelPicker,
  type CatalogModel,
  type CatalogState,
  type WorkerSelector,
} from "./WorkerModelPicker";

type Workers = { sonic: WorkerSelector; task: WorkerSelector };
type RoleMap = Record<string, WorkerSelector>;
type Preset = { workers: Workers; roles: RoleMap };
type Template = { name: string; workers: Workers; roles: RoleMap };
type Snapshot = {
  configPath: string;
  revision: string;
  savedRevision: number;
  current: Workers;
  fallback: Workers;
  roles: Record<string, string>;
  allRoles: string[];
  original: Preset | null;
  templates: Template[];
};
type Operation =
  | { action: "save"; name: string; workers: Workers; roles: RoleMap }
  | { action: "apply" | "delete"; name: string }
  | { action: "restore" };
type Change = Operation & Pick<Snapshot, "revision" | "savedRevision">;
export type VibeProfilesClient = {
  get(): Promise<Snapshot>;
  change(input: Change): Promise<Snapshot>;
  listModels(): Promise<CatalogModel[]>;
};

const workerTiers = [
  { key: "sonic", title: "Fast · sonic", inherit: "@smol" },
  { key: "task", title: "Good · task", inherit: "@task" },
] as const;
const text = (value: WorkerSelector) =>
  Array.isArray(value) ? value.join(" → ") : (value ?? "");
const hasRole = (roles: RoleMap, role: string) =>
  Object.prototype.hasOwnProperty.call(roles, role);
const roleValue = (roles: RoleMap, role: string) =>
  hasRole(roles, role) ? roles[role] : null;
const sameWorkers = (left: Workers, right: Workers) =>
  workerTiers.every(({ key }) => JSON.stringify(left[key]) === JSON.stringify(right[key]));
const matchesCurrent = (preset: Preset, state: Snapshot) =>
  sameWorkers(preset.workers, state.current) &&
  Object.entries(preset.roles).every(([role, value]) => value === roleValue(state.roles, role));
const fromCurrent = (state: Snapshot): Preset => ({ workers: state.current, roles: {} });

export function OmpVibeProfilesAction({
  client,
}: {
  client: VibeProfilesClient;
}) {
  const id = useId();
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [data, setData] = useState<Snapshot>();
  const [name, setName] = useState("");
  const [roleName, setRoleName] = useState("");
  const [addedRoles, setAddedRoles] = useState<string[]>([]);
  const [source, setSource] = useState("current");
  const [draft, setDraft] = useState<Preset>({ workers: { sonic: null, task: null }, roles: {} });
  const [catalog, setCatalog] = useState<CatalogState>({ models: [], loading: false, error: "" });
  const [error, setError] = useState("");
  const [status, setStatus] = useState("");
  const active = data?.templates.find((item) => `saved:${item.name}` === source);
  const base = active ?? (data && fromCurrent(data));
  const dirty = Boolean(base && (
    !sameWorkers(base.workers, draft.workers) ||
    Object.keys(base.roles).length !== Object.keys(draft.roles).length ||
    Object.entries(draft.roles).some(([role, value]) => !hasRole(base.roles, role) || base.roles[role] !== value)
  ));
  const named = data?.templates.find((item) => item.name === name.trim());
  const roleNames = [...new Set([...(data?.allRoles ?? []), ...addedRoles])];
  const newRole = roleName.trim().replace(/^@/, "");
  const canAddRole = /^[A-Za-z][\w-]*$/.test(newRole) &&
    newRole !== "default" && !roleNames.includes(newRole);

  function load(nextSource: string, nextPreset: Preset, nextName = "") {
    setSource(nextSource);
    setName(nextName);
    setDraft({ workers: nextPreset.workers, roles: { ...nextPreset.roles } });
  }

  function addRole() {
    if (!data || busy || !canAddRole) return;
    setAddedRoles((roles) => [...roles, newRole]);
    setRoleName("");
  }

  async function run(job: () => Promise<void>) {
    setBusy(true);
    setError("");
    setStatus("");
    try {
      await job();
    } catch (error) {
      setError(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  }

  async function refreshCatalog() {
    if (catalog.loading) return;
    setCatalog((old) => ({ ...old, loading: true, error: "" }));
    try {
      setCatalog({
        models: await client.listModels(),
        loading: false,
        error: "",
      });
    } catch (error) {
      setCatalog((old) => ({
        ...old,
        loading: false,
        error: error instanceof Error ? error.message : String(error),
      }));
    }
  }

  function refresh() {
    return run(async () => {
      const state = await client.get();
      setData(state);
      load("current", fromCurrent(state));
    });
  }

  function change(input: Operation) {
    if (!data) return;
    return run(async () => {
      const next = await client.change({ ...input, revision: data.revision, savedRevision: data.savedRevision });
      setData(next);
      if (input.action === "delete" || input.action === "restore") {
        load("current", fromCurrent(next));
      } else {
        const item = next.templates.find((item) => item.name === input.name);
        if (item) load(`saved:${item.name}`, item, item.name);
      }
      setStatus({ save: "模板已保存", delete: "模板已删除", apply: "模板已应用", restore: "已恢复原设置" }[input.action]);
    });
  }

  return (
    <Dialog
      open={open}
      onOpenChange={(value) => {
        if (busy) return;
        setOpen(value);
        if (value) {
          setData(undefined);
          void refresh();
          if (!catalog.models.length) void refreshCatalog();
        }
      }}
    >
      <DialogTrigger asChild>
        <Button variant="quiet" size="regular">
          <SlidersHorizontal className="h-4 w-4" aria-hidden="true" />
          Vibe 模板
        </Button>
      </DialogTrigger>
      <DialogContent className="w-[calc(100%-1.5rem)] max-w-xl">
        <DialogHeader>
          <DialogTitle>Vibe 模板</DialogTitle>
          <DialogDescription>
            Vibe Worker 与选定的 OMP 角色；默认模型不变。
          </DialogDescription>
        </DialogHeader>
        <div className="min-h-0 flex-auto space-y-4 overflow-y-auto px-6 pb-2">
          {data && (
            <>
              <section
                aria-label="当前 OMP 配置"
                className="space-y-2 border-b border-border pb-3"
              >
                <div className="flex items-center justify-between gap-2">
                  <h3 className="text-body font-semibold">当前 OMP 配置</h3>
                  <Button
                    variant="quiet"
                    size="icon-compact"
                    title="刷新配置"
                    aria-label="刷新配置"
                    disabled={busy}
                    onClick={() => void refresh()}
                  >
                    <RefreshCw className="h-4 w-4" />
                  </Button>
                </div>
                <dl className="grid gap-x-4 gap-y-1 sm:grid-cols-[auto_minmax(0,1fr)]">
                  {workerTiers.map(({ key, title }) => (
                    <div key={key} className="contents text-xs">
                      <dt className="text-fg-2">{title}</dt>
                      <dd className="min-w-0 break-all font-mono">
                        {data.current[key] === null
                          ? `继承 ${text(data.fallback[key])}`
                          : text(data.current[key]).split("\n").join(", ")}
                      </dd>
                    </div>
                  ))}
                  <div className="contents text-xs">
                    <dt className="text-fg-2">@default</dt>
                    <dd className="min-w-0 break-all font-mono">{data.roles.default ?? "未设置"}</dd>
                  </div>
                </dl>
                <p className="break-all text-xs text-fg-3">{data.configPath}</p>
              </section>
              <section aria-label="模板编辑" className="space-y-3">
                <div className="grid gap-3 sm:grid-cols-2">
                  <label
                    htmlFor={`${id}-source`}
                    className="space-y-1 text-body font-medium"
                  >
                    <span>载入</span>
                    <Select
                      value={source}
                      disabled={busy}
                      onValueChange={(value) => {
                        const item = data.templates.find(
                          (item) => `saved:${item.name}` === value,
                        );
                        load(value, item ?? fromCurrent(data), item?.name);
                        setStatus("");
                      }}
                    >
                      <SelectTrigger id={`${id}-source`}>
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="current">当前配置</SelectItem>
                        {data.templates.map((item) => (
                          <SelectItem
                            key={item.name}
                            value={`saved:${item.name}`}
                          >
                            {item.name}
                            {matchesCurrent(item, data) ? "（当前）" : ""}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  </label>
                  <label className="space-y-1 text-body font-medium">
                    <span>模板名称</span>
                    <Input
                      value={name}
                      maxLength={80}
                      disabled={busy}
                      onChange={(event) => setName(event.target.value)}
                      placeholder="例如：智谱 + GPT"
                    />
                  </label>
                </div>
                <div
                  aria-label="Worker 模型"
                  className="divide-y divide-border border-y border-border"
                >
                  {workerTiers.map(({ key, title, inherit }) => (
                    <WorkerModelPicker
                      key={key}
                      title={title}
                      value={draft.workers[key]}
                      inherit={inherit}
                      roles={data.roles}
                      catalog={catalog}
                      disabled={busy}
                      onRefresh={() => void refreshCatalog()}
                      onChange={(value) =>
                        setDraft((old) => ({ ...old, workers: { ...old.workers, [key]: value } }))
                      }
                    />
                  ))}
                </div>
                <section aria-label="OMP 角色" className="divide-y divide-border border-y border-border">
                  {roleNames.filter((role) => role !== "default").map((role) => {
                    const included = hasRole(draft.roles, role);
                    const current = roleValue(data.roles, role);
                    return (
                      <div key={role} className="flex min-w-0 items-center gap-2">
                        <input
                          type="checkbox"
                          aria-label={`纳入 @${role}`}
                          checked={included}
                          disabled={busy}
                          onChange={(event) => {
                            const checked = event.target.checked;
                            setDraft((old) => {
                              const roles = { ...old.roles };
                              if (checked) roles[role] = current;
                              else delete roles[role];
                              return { ...old, roles };
                            });
                          }}
                        />
                        <div className="min-w-0 flex-1">
                          <WorkerModelPicker
                            title={`@${role}`}
                            value={included ? roleValue(draft.roles, role) : current}
                            emptyLabel={included ? "清除显式设置" : "不修改（当前未设置）"}
                            roles={{}}
                            catalog={catalog}
                            disabled={busy || !included}
                            onRefresh={() => void refreshCatalog()}
                            onChange={(value) => setDraft((old) => ({ ...old, roles: { ...old.roles, [role]: value } }))}
                          />
                        </div>
                      </div>
                    );
                  })}
                  <form
                    className="flex items-center gap-2 py-2"
                    onSubmit={(event) => {
                      event.preventDefault();
                      addRole();
                    }}
                  >
                    <Input
                      aria-label="OMP 角色名"
                      placeholder="@角色名"
                      value={roleName}
                      disabled={busy}
                      onChange={(event) => setRoleName(event.target.value)}
                    />
                    <Button type="submit" variant="neutral" size="compact" disabled={busy || !canAddRole}>
                      添加角色
                    </Button>
                  </form>
                </section>
                <div className="flex flex-wrap items-center gap-2">
                  <Button
                    variant="neutral"
                    size="compact"
                    disabled={
                      busy ||
                      !name.trim() ||
                      Boolean(named && named !== active) ||
                      Boolean(named && !dirty)
                    }
                    onClick={() =>
                      void change({
                        action: "save",
                        name: name.trim(),
                        ...draft,
                      })
                    }
                  >
                    {named === active && named ? "更新模板" : "保存模板"}
                  </Button>
                  {active && (
                    <Button
                      variant="quiet"
                      size="icon-compact"
                      title="删除所选模板"
                      aria-label="删除所选模板"
                      disabled={busy}
                      onClick={() =>
                        void change({ action: "delete", name: active.name })
                      }
                    >
                      <Trash2 className="h-4 w-4" />
                    </Button>
                  )}
                  {data.original && (
                    <Button
                      variant="quiet"
                      size="compact"
                      className="ms-auto"
                      disabled={busy || matchesCurrent(data.original, data)}
                      onClick={() => void change({ action: "restore" })}
                    >
                      <RotateCcw className="h-3.5 w-3.5" />
                      恢复原设置
                    </Button>
                  )}
                </div>
                {named && named !== active && (
                  <p className="text-xs text-fg-2">模板名称已存在。</p>
                )}
              </section>
            </>
          )}
          {busy && !data && (
            <p role="status" className="text-body text-fg-2">
              正在读取 OMP 配置…
            </p>
          )}
          {error && (
            <p role="alert" className="break-words text-body text-danger">
              {error}
            </p>
          )}
          {status && (
            <p role="status" className="text-body text-fg-2">
              {status}
            </p>
          )}
        </div>
        <DialogFooter>
          <DialogClose asChild>
            <Button variant="neutral" size="compact" disabled={busy}>
              关闭
            </Button>
          </DialogClose>
          {!data && error && (
            <Button size="compact" onClick={() => void refresh()}>
              重试
            </Button>
          )}
          {data && (
            <Button
              variant="solid"
              size="compact"
              disabled={
                busy ||
                !active ||
                Boolean(dirty) ||
                name.trim() !== active.name ||
                matchesCurrent(active, data)
              }
              onClick={() => {
                if (active) void change({ action: "apply", name: active.name });
              }}
            >
              应用模板
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
