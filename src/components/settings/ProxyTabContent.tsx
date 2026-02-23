import { useState } from "react";
import * as AccordionPrimitive from "@radix-ui/react-accordion";
import { Server, Activity, ChevronDown, Zap, Globe } from "lucide-react";
import { motion } from "framer-motion";
import { useTranslation } from "react-i18next";
import {
  Accordion,
  AccordionContent,
  AccordionItem,
  AccordionTrigger,
} from "@/components/ui/accordion";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Switch } from "@/components/ui/switch";
import { ToggleRow } from "@/components/ui/toggle-row";
import { Badge } from "@/components/ui/badge";
import { ProxyPanel } from "@/components/proxy";
import { AutoFailoverConfigPanel } from "@/components/proxy/AutoFailoverConfigPanel";
import { FailoverQueueManager } from "@/components/proxy/FailoverQueueManager";
import { RectifierConfigPanel } from "@/components/settings/RectifierConfigPanel";
import { GlobalProxySettings } from "@/components/settings/GlobalProxySettings";
import { ConfirmDialog } from "@/components/ConfirmDialog";
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

  const {
    isRunning,
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
          className="rounded-xl glass-card overflow-hidden [&[data-state=open]>.accordion-header]:bg-muted/50"
        >
          <AccordionPrimitive.Header className="accordion-header flex items-center justify-between px-6 py-4 hover:bg-muted/50">
            <AccordionPrimitive.Trigger className="flex flex-1 items-center justify-between hover:no-underline [&[data-state=open]>svg]:rotate-180">
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
              </div>
              <ChevronDown className="h-4 w-4 shrink-0 transition-transform duration-200" />
            </AccordionPrimitive.Trigger>

            <div className="flex items-center gap-4 pl-4">
              <Badge
                variant={isRunning ? "default" : "secondary"}
                className="gap-1.5 h-6"
              >
                <Activity
                  className={`h-3 w-3 ${isRunning ? "animate-pulse" : ""}`}
                />
                {isRunning
                  ? t("settings.advanced.proxy.running")
                  : t("settings.advanced.proxy.stopped")}
              </Badge>
              <Switch
                checked={isRunning}
                onCheckedChange={handleToggleProxy}
                disabled={isProxyPending}
              />
            </div>
          </AccordionPrimitive.Header>
          <AccordionContent className="px-6 pb-6 pt-4 border-t border-border/50">
            <ToggleRow
              icon={<Zap className="h-4 w-4 text-green-500" />}
              title={t("settings.advanced.proxy.enableFeature")}
              description={t(
                "settings.advanced.proxy.enableFeatureDescription",
              )}
              checked={settings?.enableLocalProxy ?? false}
              onCheckedChange={(checked) =>
                onAutoSave({ enableLocalProxy: checked })
              }
            />
            <div className="mt-4">
              <ProxyPanel />
            </div>
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
                <TabsContent value="claude" className="mt-4 space-y-6">
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
                      appType="claude"
                      disabled={!isRunning}
                    />
                  </div>
                  <div className="border-t border-border/50 pt-6">
                    <AutoFailoverConfigPanel
                      appType="claude"
                      disabled={!isRunning}
                    />
                  </div>
                </TabsContent>
                <TabsContent value="codex" className="mt-4 space-y-6">
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
                      appType="codex"
                      disabled={!isRunning}
                    />
                  </div>
                  <div className="border-t border-border/50 pt-6">
                    <AutoFailoverConfigPanel
                      appType="codex"
                      disabled={!isRunning}
                    />
                  </div>
                </TabsContent>
                <TabsContent value="gemini" className="mt-4 space-y-6">
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
                      appType="gemini"
                      disabled={!isRunning}
                    />
                  </div>
                  <div className="border-t border-border/50 pt-6">
                    <AutoFailoverConfigPanel
                      appType="gemini"
                      disabled={!isRunning}
                    />
                  </div>
                </TabsContent>
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
    </motion.div>
  );
}
