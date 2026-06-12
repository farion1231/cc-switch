import { useMemo, useState } from "react";
import { Plus, Save, Trash2, Route } from "lucide-react";
import { useTranslation } from "react-i18next";
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
import { Switch } from "@/components/ui/switch";
import { useProvidersQuery } from "@/lib/query/queries";
import {
  useCreateModelRoute,
  useDeleteModelRoute,
  useModelRoutes,
  useUpdateModelRoute,
} from "@/lib/query/proxy";
import type { ModelRoute, ModelRouteInput } from "@/types/proxy";

const APP_TYPE = "claude";

interface EditableRoute {
  pattern: string;
  providerId: string;
  priority: string;
  enabled: boolean;
}

const emptyRoute: EditableRoute = {
  pattern: "",
  providerId: "",
  priority: "100",
  enabled: true,
};

export function ModelRouteTable() {
  const { t } = useTranslation();
  const { data: routes = [] } = useModelRoutes(APP_TYPE);
  const { data: providersData } = useProvidersQuery(APP_TYPE);
  const createRoute = useCreateModelRoute();
  const updateRoute = useUpdateModelRoute();
  const deleteRoute = useDeleteModelRoute(APP_TYPE);
  const [draft, setDraft] = useState<EditableRoute>(emptyRoute);
  const [edits, setEdits] = useState<Record<string, EditableRoute>>({});

  const providers = useMemo(
    () => Object.values(providersData?.providers ?? {}),
    [providersData?.providers],
  );

  const toInput = (
    route: EditableRoute,
    existingRouteId?: string,
  ): ModelRouteInput | null => {
    const priority = Number.parseInt(route.priority.trim(), 10);
    if (
      route.pattern.trim().length === 0 ||
      route.providerId.length === 0 ||
      Number.isNaN(priority)
    ) {
      return null;
    }
    if (hasPriorityConflict(route, routes, edits, existingRouteId)) {
      return null;
    }

    return {
      appType: APP_TYPE,
      pattern: route.pattern.trim(),
      providerId: route.providerId,
      priority,
      enabled: route.enabled,
    };
  };

  const handleCreate = async () => {
    const input = toInput(draft);
    if (!input) return;
    await createRoute.mutateAsync(input);
    setDraft(emptyRoute);
  };

  const handleUpdate = async (route: ModelRoute) => {
    const edit = edits[route.id] ?? routeToEditable(route);
    const input = toInput(edit, route.id);
    if (!input) return;
    await updateRoute.mutateAsync({ routeId: route.id, route: input });
  };

  const updateEdit = (route: ModelRoute, patch: Partial<EditableRoute>) => {
    setEdits((current) => ({
      ...current,
      [route.id]: {
        ...(current[route.id] ?? routeToEditable(route)),
        ...patch,
      },
    }));
  };

  return (
    <div className="rounded-lg border border-border bg-muted/40 p-4 space-y-4">
      <div className="flex items-center gap-2">
        <Route className="h-4 w-4 text-muted-foreground" />
        <div>
          <h4 className="text-sm font-semibold">
            {t("proxy.modelRoutes.title", {
              defaultValue: "模型路由",
            })}
          </h4>
          <p className="text-xs text-muted-foreground">
            {t("proxy.modelRoutes.description", {
              defaultValue:
                "按 Claude Code 请求中的 model 字段选择现有 Provider；未匹配时保持默认路由。",
            })}
          </p>
        </div>
      </div>

      <div className="grid gap-3 md:grid-cols-[minmax(0,1.2fr)_minmax(0,1fr)_100px_80px_auto] md:items-end">
        <RouteFields
          route={draft}
          providers={providers}
          conflict={hasPriorityConflict(draft, routes, edits)}
          onChange={(patch) =>
            setDraft((current) => ({ ...current, ...patch }))
          }
        />
        <Button
          size="sm"
          onClick={handleCreate}
          disabled={!toInput(draft) || createRoute.isPending}
        >
          <Plus className="mr-2 h-4 w-4" />
          {t("common.add", { defaultValue: "添加" })}
        </Button>
      </div>

      <div className="space-y-2">
        {routes.length === 0 ? (
          <p className="rounded-md border border-dashed border-border px-3 py-4 text-sm text-muted-foreground">
            {t("proxy.modelRoutes.empty", {
              defaultValue:
                "还没有模型路由。添加一条规则后，匹配的模型会走指定 Provider。",
            })}
          </p>
        ) : (
          routes.map((route) => {
            const edit = edits[route.id] ?? routeToEditable(route);
            return (
              <div
                key={route.id}
                className="grid gap-3 rounded-md border border-border bg-background/60 p-3 md:grid-cols-[minmax(0,1.2fr)_minmax(0,1fr)_100px_80px_auto] md:items-end"
              >
                <RouteFields
                  route={edit}
                  providers={providers}
                  conflict={hasPriorityConflict(edit, routes, edits, route.id)}
                  onChange={(patch) => updateEdit(route, patch)}
                />
                <div className="flex gap-2">
                  <Button
                    size="icon"
                    variant="outline"
                    onClick={() => handleUpdate(route)}
                    disabled={!toInput(edit, route.id) || updateRoute.isPending}
                    title={t("common.save", { defaultValue: "保存" })}
                  >
                    <Save className="h-4 w-4" />
                  </Button>
                  <Button
                    size="icon"
                    variant="outline"
                    onClick={() => deleteRoute.mutateAsync(route.id)}
                    disabled={deleteRoute.isPending}
                    title={t("common.delete", { defaultValue: "删除" })}
                  >
                    <Trash2 className="h-4 w-4" />
                  </Button>
                </div>
              </div>
            );
          })
        )}
      </div>
    </div>
  );
}

function RouteFields({
  route,
  providers,
  conflict = false,
  onChange,
}: {
  route: EditableRoute;
  providers: Array<{ id: string; name: string }>;
  conflict?: boolean;
  onChange: (patch: Partial<EditableRoute>) => void;
}) {
  const { t } = useTranslation();

  return (
    <>
      <div className="space-y-1.5">
        <Label className="text-xs">
          {t("proxy.modelRoutes.pattern", { defaultValue: "Pattern" })}
        </Label>
        <Input
          value={route.pattern}
          onChange={(event) => onChange({ pattern: event.target.value })}
          placeholder="*opus*"
        />
        {conflict ? (
          <p className="text-xs text-destructive">
            {t("proxy.modelRoutes.conflict", {
              defaultValue: "Pattern 与 Priority 不能同时重复。",
            })}
          </p>
        ) : null}
      </div>
      <div className="space-y-1.5">
        <Label className="text-xs">
          {t("proxy.modelRoutes.provider", { defaultValue: "Provider" })}
        </Label>
        <Select
          value={route.providerId}
          onValueChange={(providerId) => onChange({ providerId })}
        >
          <SelectTrigger>
            <SelectValue
              placeholder={t("proxy.modelRoutes.selectProvider", {
                defaultValue: "选择 Provider",
              })}
            />
          </SelectTrigger>
          <SelectContent>
            {providers.map((provider) => (
              <SelectItem key={provider.id} value={provider.id}>
                {provider.name}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
      <div className="space-y-1.5">
        <Label className="text-xs">
          {t("proxy.modelRoutes.priority", { defaultValue: "Priority" })}
        </Label>
        <Input
          value={route.priority}
          inputMode="numeric"
          onChange={(event) => onChange({ priority: event.target.value })}
        />
      </div>
      <div className="space-y-1.5">
        <Label className="text-xs">
          {t("proxy.modelRoutes.enabled", { defaultValue: "Enabled" })}
        </Label>
        <div className="flex h-9 items-center">
          <Switch
            checked={route.enabled}
            onCheckedChange={(enabled) => onChange({ enabled })}
          />
        </div>
      </div>
    </>
  );
}

function routeToEditable(route: ModelRoute): EditableRoute {
  return {
    pattern: route.pattern,
    providerId: route.providerId,
    priority: String(route.priority),
    enabled: route.enabled,
  };
}

function hasPriorityConflict(
  route: EditableRoute,
  routes: ModelRoute[],
  edits: Record<string, EditableRoute>,
  existingRouteId?: string,
): boolean {
  if (!route.enabled) return false;

  const pattern = route.pattern.trim();
  const priority = Number.parseInt(route.priority.trim(), 10);
  if (!pattern || Number.isNaN(priority)) return false;

  return routes.some((existing) => {
    if (existing.id === existingRouteId) return false;
    const existingEdit = edits[existing.id] ?? routeToEditable(existing);
    if (!existingEdit.enabled) return false;
    return (
      existingEdit.pattern.trim() === pattern &&
      Number.parseInt(existingEdit.priority.trim(), 10) === priority
    );
  });
}
