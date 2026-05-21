import { useState } from "react";
import { Server, Activity, Zap, Globe, ShieldAlert } from "lucide-react";
import { motion } from "framer-motion";
import { useTranslation } from "react-i18next";
import {
  Accordion,
  AccordionContent,
  AccordionItem,
  AccordionTrigger,
} from "@/components/ui/accordion";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Badge } from "@/components/ui/badge";
import { ProxyPanel } from "@/components/proxy";
import { AutoFailoverConfigPanel } from "@/components/proxy/AutoFailoverConfigPanel";
import { FailoverQueueManager } from "@/components/proxy/FailoverQueueManager";
import { RectifierConfigPanel } from "@/components/settings/RectifierConfigPanel";
import { GlobalProxySettings } from "@/components/settings/GlobalProxySettings";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { ToggleRow } from "@/components/ui/toggle-row";
import { useProxyStatus } from "@/hooks/useProxyStatus";
import type { SettingsFormState } from "@/hooks/useSettings";

interface ProxyTabContentProps {
  settings: SettingsFormState;
  onAutoSave: (updates: Partial<SettingsFormState>) => Promise<void>;
}

export function ProxyTabContent({
  settings,
  onAutoSave,
}: ProxyTabContentProps) {
  const { t } = useTranslation();
  const [showProxyConfirm, setShowProxyConfirm] = useState(false);
  const [showFailoverConfirm, setShowFailoverConfirm] = useState(false);

  const {
    isRunning,
    takeoverStatus,
    startProxyServer,
    stopWithRestore,
    isPending: isProxyPending,
  } = useProxyStatus();

  const handleToggleProxy = async (checked: boolean) => {
    try {
      if (!checked) {
        await stopWithRestore();
      } else if (!settings?.proxyConfirmed) {
        setShowProxyConfirm(true);
      } else {
        await startProxyServer();
      }
    } catch (error) {
      console.error("Toggle proxy failed:", error);
    }
  };

  const handleProxyConfirm = async () => {
    setShowProxyConfirm(false);
    try {
      await onAutoSave({ proxyConfirmed: true });
      await startProxyServer();
    } catch (error) {
      console.error("Proxy confirm failed:", error);
    }
  };

  const handleFailoverToggleChange = (checked: boolean) => {
    if (checked && !settings?.failoverConfirmed) {
      setShowFailoverConfirm(true);
    } else {
      void onAutoSave({ enableFailoverToggle: checked });
    }
  };

  const handleFailoverConfirm = async () => {
    setShowFailoverConfirm(false);
    try {
      await onAutoSave({ failoverConfirmed: true, enableFailoverToggle: true });
    } catch (error) {
      console.error("Failover confirm failed:", error);
    }
  };

  return (
    <motion.div
      initial={{ opacity: 0, y: 10 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.3 }}
      className="space-y-4"
    >
      <Accordion type="multiple" defaultValue={[]} className="w-full space-y-4">
        {/* Local Proxy */}
        <AccordionItem
          value="proxy"
          className="rounded-xl glass-card overflow-hidden"
        >
          <AccordionTrigger className="px-6 py-4 hover:no-underline hover:bg-muted/50 data-[state=open]:bg-muted/50">
            <div className="flex items-center gap-3">
              <Server className="h-5 w-5 text-green-500" />
              <div className="text-left">
                <h3 className="text-base font-semibold">
                  {t("settings.advanced.proxy.title")}
                </h3>
                <p className="text-sm text-muted-foreground font-normal">
                  {t("settings.advanced.proxy.description")}
                </p>
              </div>
              <Badge
                variant={isRunning ? "default" : "secondary"}
                className="gap-1.5 h-6 ml-auto mr-2"
              >
                <Activity
                  className={`h-3 w-3 ${isRunning ? "animate-pulse" : ""}`}
                />
                {isRunning
                  ? t("settings.advanced.proxy.running")
                  : t("settings.advanced.proxy.stopped")}
              </Badge>
            </div>
          </AccordionTrigger>
          <AccordionContent className="px-6 pb-6 pt-4 border-t border-border/50">
            <ProxyPanel
              enableLocalProxy={settings?.enableLocalProxy ?? false}
              onEnableLocalProxyChange={(checked) =>
                onAutoSave({ enableLocalProxy: checked })
              }
              onToggleProxy={handleToggleProxy}
              isProxyPending={isProxyPending}
            />
          </AccordionContent>
        </AccordionItem>

        {/* Auto Failover */}
        <AccordionItem
          value="failover"
          className="rounded-xl glass-card overflow-hidden"
        >
          <AccordionTrigger className="px-6 py-4 hover:no-underline hover:bg-muted/50 data-[state=open]:bg-muted/50">
            <div className="flex items-center gap-3">
              <Activity className="h-5 w-5 text-orange-500" />
              <div className="text-left">
                <h3 className="text-base font-semibold">
                  {t("settings.advanced.failover.title")}
                </h3>
                <p className="text-sm text-muted-foreground font-normal">
                  {t("settings.advanced.failover.description")}
                </p>
              </div>
            </div>
          </AccordionTrigger>
          <AccordionContent className="px-6 pb-6 pt-4 border-t border-border/50">
            <div className="space-y-6">
              <ToggleRow
                icon={<ShieldAlert className="h-4 w-4 text-orange-500" />}
                title={t("settings.advanced.proxy.enableFailoverToggle")}
                description={t(
                  "settings.advanced.proxy.enableFailoverToggleDescription",
                )}
                checked={settings?.enableFailoverToggle ?? false}
                onCheckedChange={handleFailoverToggleChange}
              />

              {!isRunning && (
                <div className="p-4 rounded-lg bg-yellow-500/10 border border-yellow-500/20">
                  <p className="text-sm text-yellow-600 dark:text-yellow-400">
                    {t("proxy.failover.proxyRequired", {
                      defaultValue: "需要先启动代理服务才能配置故障转移",
                    })}
                  </p>
                </div>
              )}

              <Tabs defaultValue="claude" className="w-full">
                <TabsList className="grid w-full grid-cols-3">
                  <TabsTrigger value="claude">Claude</TabsTrigger>
                  <TabsTrigger value="codex">Codex</TabsTrigger>
                  <TabsTrigger value="gemini">Gemini</TabsTrigger>
                </TabsList>
                {(["claude", "codex", "gemini"] as const).map((appType) => {
                  const failoverDisabled =
                    !isRunning || !(takeoverStatus?.[appType] ?? false);
                  return (
                    <TabsContent
                      key={appType}
                      value={appType}
                      className="mt-4 space-y-6"
                    >
                      <div className="space-y-4">
                        <div>
                          <h4 className="text-sm font-semibold">
                            {t("proxy.failoverQueue.title")}
                          </h4>
                          <p className="text-xs text-muted-foreground">
                            {t("proxy.failoverQueue.description")}
                          </p>
                        </div>
                        <FailoverQueueManager
                          appType={appType}
                          disabled={failoverDisabled}
                        />
                      </div>
                      <div className="border-t border-border/50 pt-6">
                        <AutoFailoverConfigPanel
                          appType={appType}
                          disabled={failoverDisabled}
                        />
                      </div>
                    </TabsContent>
                  );
                })}
              </Tabs>
            </div>
          </AccordionContent>
        </AccordionItem>

        {/* Rectifier */}
        <AccordionItem
          value="rectifier"
          className="rounded-xl glass-card overflow-hidden"
        >
          <AccordionTrigger className="px-6 py-4 hover:no-underline hover:bg-muted/50 data-[state=open]:bg-muted/50">
            <div className="flex items-center gap-3">
              <Zap className="h-5 w-5 text-purple-500" />
              <div className="text-left">
                <h3 className="text-base font-semibold">
                  {t("settings.advanced.rectifier.title")}
                </h3>
                <p className="text-sm text-muted-foreground font-normal">
                  {t("settings.advanced.rectifier.description")}
                </p>
              </div>
            </div>
          </AccordionTrigger>
          <AccordionContent className="px-6 pb-6 pt-4 border-t border-border/50">
            <RectifierConfigPanel />
          </AccordionContent>
        </AccordionItem>

        {/* Global Outbound Proxy */}
        <AccordionItem
          value="globalProxy"
          className="rounded-xl glass-card overflow-hidden"
        >
          <AccordionTrigger className="px-6 py-4 hover:no-underline hover:bg-muted/50 data-[state=open]:bg-muted/50">
            <div className="flex items-center gap-3">
              <Globe className="h-5 w-5 text-cyan-500" />
              <div className="text-left">
                <h3 className="text-base font-semibold">
                  {t("settings.advanced.globalProxy.title")}
                </h3>
                <p className="text-sm text-muted-foreground font-normal">
                  {t("settings.advanced.globalProxy.description")}
                </p>
              </div>
            </div>
          </AccordionTrigger>
          <AccordionContent className="px-6 pb-6 pt-4 border-t border-border/50">
            <GlobalProxySettings />
          </AccordionContent>
        </AccordionItem>
      </Accordion>

      <ConfirmDialog
        isOpen={showProxyConfirm}
        variant="info"
        title={t("confirm.proxy.title")}
        message={t("confirm.proxy.message")}
        confirmText={t("confirm.proxy.confirm")}
        onConfirm={() => void handleProxyConfirm()}
        onCancel={() => setShowProxyConfirm(false)}
      />

      <ConfirmDialog
        isOpen={showFailoverConfirm}
        variant="info"
        title={t("confirm.failover.title")}
        message={t("confirm.failover.message")}
        confirmText={t("confirm.failover.confirm")}
        onConfirm={() => void handleFailoverConfirm()}
        onCancel={() => setShowFailoverConfirm(false)}
      />
    </motion.div>
  );
}
