/**
 * 全局出站代理设置组件
 *
 * 提供配置全局代理的输入界面。
 */

import { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { Loader2, TestTube2, Globe } from "lucide-react";
import {
    useGlobalProxyUrl,
    useSetGlobalProxyUrl,
    useTestProxy,
} from "@/hooks/useGlobalProxy";

export function GlobalProxySettings() {
    const { t } = useTranslation();
    const { data: savedUrl, isLoading } = useGlobalProxyUrl();
    const setMutation = useSetGlobalProxyUrl();
    const testMutation = useTestProxy();

    const [url, setUrl] = useState("");
    const [dirty, setDirty] = useState(false);

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

    const handleKeyDown = (e: React.KeyboardEvent) => {
        if (e.key === "Enter" && dirty && !setMutation.isPending) {
            handleSave();
        }
    };

    if (isLoading) {
        return (
            <div className="flex items-center justify-center p-4">
                <Loader2 className="h-5 w-5 animate-spin text-muted-foreground" />
            </div>
        );
    }

    return (
        <div className="space-y-3">
            {/* 标题 */}
            <div className="flex items-center gap-2">
                <Globe className="h-4 w-4 text-muted-foreground" />
                <Label className="text-sm font-medium">
                    {t("settings.globalProxy.label")}
                </Label>
            </div>

            {/* 描述 */}
            <p className="text-xs text-muted-foreground">
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
        </div>
    );
}
