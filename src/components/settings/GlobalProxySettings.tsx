/**
 * 全局出站代理设置组件
 *
 * 提供配置全局代理的输入界面。
 */

import { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { Loader2, TestTube2, Search } from "lucide-react";
import {
  useGlobalProxyUrl,
  useSetGlobalProxyUrl,
  useTestProxy,
  useScanProxies,
  type DetectedProxy,
} from "@/hooks/useGlobalProxy";

export function GlobalProxySettings() {
  const { t } = useTranslation();
  const { data: savedUrl, isLoading } = useGlobalProxyUrl();
  const setMutation = useSetGlobalProxyUrl();
  const testMutation = useTestProxy();
  const scanMutation = useScanProxies();

  const [url, setUrl] = useState("");
  const [dirty, setDirty] = useState(false);
  const [detected, setDetected] = useState<DetectedProxy[]>([]);

  // 同步远程配置
  useEffect(() => {
    if (savedUrl !== undefined) {
      setUrl(savedUrl || "");
      setDirty(false);
    }
  }, [savedUrl]);

  const handleSave = async () => {
    await setMutation.mutateAsync(url.trim());
    setDirty(false);
  };

  const handleTest = async () => {
    if (url.trim()) {
      await testMutation.mutateAsync(url.trim());
    }
  };

  const handleScan = async () => {
    const result = await scanMutation.mutateAsync();
    setDetected(result);
  };

  const handleSelect = (proxyUrl: string) => {
    setUrl(proxyUrl);
    setDirty(true);
    setDetected([]);
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && dirty && !setMutation.isPending) {
      handleSave();
    }
  };

  // 只在首次加载且无数据时显示加载状态
  if (isLoading && savedUrl === undefined) {
    return (
      <div className="flex items-center justify-center p-4">
        <Loader2 className="h-5 w-5 animate-spin text-muted-foreground" />
      </div>
    );
  }

  return (
    <div className="space-y-3">
      {/* 描述 */}
      <p className="text-sm text-muted-foreground">
        {t("settings.globalProxy.hint")}
      </p>

      {/* 输入框和按钮 */}
      <div className="flex gap-2">
        <Input
          placeholder="http://127.0.0.1:7890 / socks5://127.0.0.1:1080"
          value={url}
          onChange={(e) => {
            setUrl(e.target.value);
            setDirty(true);
          }}
          onKeyDown={handleKeyDown}
          className="font-mono text-sm flex-1"
        />
        <Button
          variant="outline"
          size="icon"
          disabled={scanMutation.isPending}
          onClick={handleScan}
          title={t("settings.globalProxy.scan")}
        >
          {scanMutation.isPending ? (
            <Loader2 className="h-4 w-4 animate-spin" />
          ) : (
            <Search className="h-4 w-4" />
          )}
        </Button>
        <Button
          variant="outline"
          size="icon"
          disabled={!url.trim() || testMutation.isPending}
          onClick={handleTest}
          title={t("settings.globalProxy.test")}
        >
          {testMutation.isPending ? (
            <Loader2 className="h-4 w-4 animate-spin" />
          ) : (
            <TestTube2 className="h-4 w-4" />
          )}
        </Button>
        <Button
          onClick={handleSave}
          disabled={!dirty || setMutation.isPending}
          size="sm"
        >
          {setMutation.isPending && (
            <Loader2 className="mr-2 h-4 w-4 animate-spin" />
          )}
          {t("common.save")}
        </Button>
      </div>

      {/* 扫描结果 */}
      {detected.length > 0 && (
        <div className="flex flex-wrap gap-2">
          {detected.map((p) => (
            <Button
              key={p.url}
              variant="secondary"
              size="sm"
              onClick={() => handleSelect(p.url)}
              className="font-mono text-xs"
            >
              {p.url}
            </Button>
          ))}
        </div>
      )}
    </div>
  );
}
