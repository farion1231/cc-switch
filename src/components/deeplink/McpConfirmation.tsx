import { useMemo } from "react";
import { DeepLinkImportRequest } from "../../lib/api/deeplink";
import { decodeBase64Utf8 } from "../../lib/utils/base64";

export function McpConfirmation({
  request,
}: {
  request: DeepLinkImportRequest;
}) {
  const mcpServers = useMemo(() => {
    if (!request.config) return null;
    try {
      const decoded = decodeBase64Utf8(request.config);
      const parsed = JSON.parse(decoded);
      return parsed.mcpServers || {};
    } catch (e) {
      console.error("Failed to parse MCP config:", e);
      return null;
    }
  }, [request.config]);

  const targetApps = request.apps?.split(",") || [];

  return (
    <div className="space-y-4">
      <h3 className="text-lg font-semibold">批量导入 MCP Servers</h3>

      <div>
        <label className="block text-sm font-medium text-muted-foreground">
          目标应用
        </label>
        <div className="mt-1 flex gap-2 flex-wrap">
          {targetApps.map((app) => (
            <span
              key={app}
              className="px-2 py-1 bg-primary/10 text-primary text-xs rounded capitalize"
            >
              {app.trim()}
            </span>
          ))}
        </div>
      </div>

      <div>
        <label className="block text-sm font-medium text-muted-foreground">
          MCP Servers ({Object.keys(mcpServers || {}).length} 个)
        </label>
        <div className="mt-1 space-y-2 max-h-64 overflow-auto border rounded p-2 bg-muted/30">
          {mcpServers &&
            Object.entries(mcpServers).map(([id, spec]: [string, any]) => (
              <div key={id} className="p-2 bg-background rounded border">
                <div className="font-semibold text-sm">{id}</div>
                <div className="text-xs text-muted-foreground mt-1 font-mono truncate">
                  {spec.command
                    ? `Command: ${spec.command} `
                    : `URL: ${spec.url} `}
                </div>
              </div>
            ))}
        </div>
      </div>

      {request.enabled && (
        <div className="text-yellow-600 dark:text-yellow-500 text-sm flex items-center gap-2">
          <span>⚠️</span>
          <span>导入后将立即写入所有指定应用的配置文件</span>
        </div>
      )}
    </div>
  );
}
