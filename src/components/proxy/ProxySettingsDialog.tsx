/**
 * 代理服务设置对话框
 */

import {
  Form,
  FormControl,
  FormField,
  FormItem,
  FormLabel,
  FormDescription,
  FormMessage,
} from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";
import { useProxyConfig } from "@/hooks/useProxyConfig";
import { useEffect, useMemo } from "react";
import { AlertCircle } from "lucide-react";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";
import type { ProxyConfig } from "@/types/proxy";

// 表单数据类型（仅包含可编辑字段）
type ProxyConfigForm = Pick<
  ProxyConfig,
  | "listen_address"
  | "listen_port"
  | "max_retries"
  | "request_timeout"
  | "enable_logging"
  | "streaming_first_byte_timeout"
  | "streaming_idle_timeout"
  | "non_streaming_timeout"
>;

const createProxyConfigSchema = (t: TFunction) => {
  // 流式首字节超时：0 或 1-180 秒
  const streamingFirstByteSchema = z
    .number()
    .min(0)
    .refine((val) => val === 0 || (val >= 1 && val <= 180), {
      message: t("proxy.settings.validation.streamingFirstByteRange", {
        defaultValue: "请输入 0 或 1-180 之间的数值",
      }),
    });

  // 流式静默期超时：0 或 60-600 秒
  const streamingIdleSchema = z
    .number()
    .min(0)
    .refine((val) => val === 0 || (val >= 60 && val <= 600), {
      message: t("proxy.settings.validation.streamingIdleRange", {
        defaultValue: "请输入 0 或 60-600 之间的数值",
      }),
    });

  // 非流式总超时：0 或 60-1800 秒
  const nonStreamingSchema = z
    .number()
    .min(0)
    .refine((val) => val === 0 || (val >= 60 && val <= 1800), {
      message: t("proxy.settings.validation.nonStreamingRange", {
        defaultValue: "请输入 0 或 60-1800 之间的数值",
      }),
    });

  // 旧版请求超时（兼容）
  const requestTimeoutSchema = z.number().min(0);

  return z.object({
    listen_address: z.string().regex(
      /^(\d{1,3}\.){3}\d{1,3}$/,
      t("proxy.settings.validation.addressInvalid", {
        defaultValue: "请输入有效的IP地址",
      }),
    ),
    listen_port: z
      .number()
      .min(
        1024,
        t("proxy.settings.validation.portMin", {
          defaultValue: "端口必须大于1024",
        }),
      )
      .max(
        65535,
        t("proxy.settings.validation.portMax", {
          defaultValue: "端口必须小于65535",
        }),
      ),
    max_retries: z
      .number()
      .min(
        0,
        t("proxy.settings.validation.retryMin", {
          defaultValue: "重试次数不能为负",
        }),
      )
      .max(
        10,
        t("proxy.settings.validation.retryMax", {
          defaultValue: "重试次数不能超过10",
        }),
      ),
    request_timeout: requestTimeoutSchema,
    enable_logging: z.boolean(),
    streaming_first_byte_timeout: streamingFirstByteSchema,
    streaming_idle_timeout: streamingIdleSchema,
    non_streaming_timeout: nonStreamingSchema,
  });
};

interface ProxySettingsDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function ProxySettingsDialog({
  open,
  onOpenChange,
}: ProxySettingsDialogProps) {
  const { config, isLoading, updateConfig, isUpdating } = useProxyConfig();
  const { t } = useTranslation();
  const schema = useMemo(() => createProxyConfigSchema(t), [t]);

  const closePanel = () => onOpenChange(false);

  const form = useForm<ProxyConfigForm>({
    resolver: zodResolver(schema),
    defaultValues: {
      listen_address: "127.0.0.1",
      listen_port: 5000,
      max_retries: 3,
      request_timeout: 300,
      enable_logging: true,
      streaming_first_byte_timeout: 30,
      streaming_idle_timeout: 60,
      non_streaming_timeout: 600,
    },
  });

  // 当配置加载完成后更新表单
  useEffect(() => {
    if (config) {
      form.reset({
        listen_address: config.listen_address,
        listen_port: config.listen_port,
        max_retries: config.max_retries,
        request_timeout: config.request_timeout,
        enable_logging: config.enable_logging,
        streaming_first_byte_timeout: config.streaming_first_byte_timeout ?? 30,
        streaming_idle_timeout: config.streaming_idle_timeout ?? 60,
        non_streaming_timeout: config.non_streaming_timeout ?? 300,
      });
    }
  }, [config, form]);

  const onSubmit = async (data: ProxyConfigForm) => {
    try {
      await updateConfig(data);
      closePanel();
    } catch (error) {
      console.error("Save config failed:", error);
    }
  };

  const formId = "proxy-settings-form";

  return (
    <FullScreenPanel
      isOpen={open}
      title={t("proxy.settings.title", { defaultValue: "代理服务设置" })}
      onClose={closePanel}
      footer={
        <>
          <Button
            type="button"
            variant="outline"
            onClick={closePanel}
            disabled={isUpdating}
          >
            {t("common.cancel", { defaultValue: "取消" })}
          </Button>
          <Button
            type="submit"
            form={formId}
            disabled={isUpdating || isLoading}
          >
            {isUpdating
              ? t("common.saving", { defaultValue: "保存中..." })
              : t("proxy.settings.actions.save", { defaultValue: "保存配置" })}
          </Button>
        </>
      }
    >
      <div className="space-y-6">
        <p className="text-sm text-muted-foreground">
          {t("proxy.settings.description", {
            defaultValue:
              "配置本地代理服务器的监听地址、端口和运行参数，保存后立即生效。",
          })}
        </p>
        <Alert className="border-emerald-500/40 bg-emerald-500/10">
          <AlertCircle className="h-4 w-4" />
          <AlertDescription className="text-sm">
            {t("proxy.settings.alert.autoApply", {
              defaultValue:
                "保存后将自动同步到正在运行的代理服务，无需手动重启。",
            })}
          </AlertDescription>
        </Alert>

        <Form {...form}>
          <form
            id={formId}
            onSubmit={form.handleSubmit(onSubmit)}
            className="space-y-6"
          >
            <section className="space-y-4 rounded-xl border border-white/10 glass-card p-6">
              <div>
                <h3 className="text-base font-semibold text-foreground">
                  {t("proxy.settings.basic.title", {
                    defaultValue: "基础设置",
                  })}
                </h3>
                <p className="text-sm text-muted-foreground">
                  {t("proxy.settings.basic.description", {
                    defaultValue: "配置代理服务监听的地址与端口。",
                  })}
                </p>
              </div>

              <div className="grid gap-4 md:grid-cols-2">
                <FormField
                  control={form.control}
                  name="listen_address"
                  render={({ field }) => (
                    <FormItem>
                      <FormLabel>
                        {t("proxy.settings.fields.listenAddress.label", {
                          defaultValue: "监听地址",
                        })}
                      </FormLabel>
                      <FormControl>
                        <Input
                          {...field}
                          placeholder={t(
                            "proxy.settings.fields.listenAddress.placeholder",
                            { defaultValue: "127.0.0.1" },
                          )}
                          disabled={isLoading}
                        />
                      </FormControl>
                      <FormDescription>
                        {t("proxy.settings.fields.listenAddress.description", {
                          defaultValue:
                            "代理服务器监听的 IP 地址（推荐 127.0.0.1）",
                        })}
                      </FormDescription>
                      <FormMessage />
                    </FormItem>
                  )}
                />

                <FormField
                  control={form.control}
                  name="listen_port"
                  render={({ field }) => (
                    <FormItem>
                      <FormLabel>
                        {t("proxy.settings.fields.listenPort.label", {
                          defaultValue: "监听端口",
                        })}
                      </FormLabel>
                      <FormControl>
                        <Input
                          type="number"
                          inputMode="numeric"
                          {...field}
                          onChange={(e) =>
                            field.onChange(parseInt(e.target.value, 10) || 0)
                          }
                          placeholder={t(
                            "proxy.settings.fields.listenPort.placeholder",
                            { defaultValue: "5000" },
                          )}
                          disabled={isLoading}
                        />
                      </FormControl>
                      <FormDescription>
                        {t("proxy.settings.fields.listenPort.description", {
                          defaultValue:
                            "代理服务器监听的端口号（1024 ~ 65535）",
                        })}
                      </FormDescription>
                      <FormMessage />
                    </FormItem>
                  )}
                />
              </div>
            </section>

            <section className="space-y-4 rounded-xl border border-white/10 glass-card p-6">
              <div>
                <h3 className="text-base font-semibold text-foreground">
                  {t("proxy.settings.advanced.title", {
                    defaultValue: "高级参数",
                  })}
                </h3>
                <p className="text-sm text-muted-foreground">
                  {t("proxy.settings.advanced.description", {
                    defaultValue: "控制请求的稳定性和日志记录。",
                  })}
                </p>
              </div>

              <div className="grid gap-4 md:grid-cols-2">
                <FormField
                  control={form.control}
                  name="max_retries"
                  render={({ field }) => (
                    <FormItem>
                      <FormLabel>
                        {t("proxy.settings.fields.maxRetries.label", {
                          defaultValue: "最大重试次数",
                        })}
                      </FormLabel>
                      <FormControl>
                        <Input
                          type="number"
                          inputMode="numeric"
                          {...field}
                          onChange={(e) =>
                            field.onChange(parseInt(e.target.value, 10) || 0)
                          }
                          placeholder={t(
                            "proxy.settings.fields.maxRetries.placeholder",
                            { defaultValue: "3" },
                          )}
                          disabled={isLoading}
                        />
                      </FormControl>
                      <FormDescription>
                        {t("proxy.settings.fields.maxRetries.description", {
                          defaultValue: "请求失败时的重试次数（0 ~ 10）",
                        })}
                      </FormDescription>
                      <FormMessage />
                    </FormItem>
                  )}
                />

                <FormField
                  control={form.control}
                  name="request_timeout"
                  render={({ field }) => (
                    <FormItem>
                      <FormLabel>
                        {t("proxy.settings.fields.requestTimeout.label", {
                          defaultValue: "请求超时（秒）",
                        })}
                      </FormLabel>
                      <FormControl>
                        <Input
                          type="number"
                          inputMode="numeric"
                          {...field}
                          onChange={(e) =>
                            field.onChange(parseInt(e.target.value, 10) || 0)
                          }
                          placeholder={t(
                            "proxy.settings.fields.requestTimeout.placeholder",
                            { defaultValue: "0（不限）或 300" },
                          )}
                          disabled={isLoading}
                        />
                      </FormControl>
                      <FormDescription>
                        {t("proxy.settings.fields.requestTimeout.description", {
                          defaultValue:
                            "单个请求的最大等待时间（0 表示不限制，或设置 10 ~ 600 秒）",
                        })}
                      </FormDescription>
                      <FormMessage />
                    </FormItem>
                  )}
                />
              </div>

              <FormField
                control={form.control}
                name="enable_logging"
                render={({ field }) => (
                  <FormItem className="flex items-center justify-between rounded-lg border border-white/10 bg-background/60 p-4">
                    <div className="space-y-1">
                      <FormLabel>
                        {t("proxy.settings.fields.enableLogging.label", {
                          defaultValue: "启用日志记录",
                        })}
                      </FormLabel>
                      <FormDescription>
                        {t("proxy.settings.fields.enableLogging.description", {
                          defaultValue: "记录所有代理请求，便于排查问题",
                        })}
                      </FormDescription>
                    </div>
                    <FormControl>
                      <Switch
                        checked={field.value}
                        onCheckedChange={field.onChange}
                      />
                    </FormControl>
                  </FormItem>
                )}
              />
            </section>

            <section className="space-y-4 rounded-xl border border-white/10 glass-card p-6">
              <div>
                <h3 className="text-base font-semibold text-foreground">
                  {t("proxy.settings.timeout.title", {
                    defaultValue: "超时设置",
                  })}
                </h3>
                <p className="text-sm text-muted-foreground">
                  {t("proxy.settings.timeout.description", {
                    defaultValue: "配置流式和非流式请求的超时时间。",
                  })}
                </p>
              </div>

              <div className="grid gap-4 md:grid-cols-3">
                <FormField
                  control={form.control}
                  name="streaming_first_byte_timeout"
                  render={({ field }) => (
                    <FormItem>
                      <FormLabel>
                        {t(
                          "proxy.settings.fields.streamingFirstByteTimeout.label",
                          {
                            defaultValue: "流式首字超时（秒）",
                          },
                        )}
                      </FormLabel>
                      <FormControl>
                        <Input
                          type="number"
                          inputMode="numeric"
                          {...field}
                          onChange={(e) =>
                            field.onChange(parseInt(e.target.value, 10) || 0)
                          }
                          placeholder="30"
                          disabled={isLoading}
                        />
                      </FormControl>
                      <FormDescription>
                        {t(
                          "proxy.settings.fields.streamingFirstByteTimeout.description",
                          {
                            defaultValue: "等待首个数据块的最大时间",
                          },
                        )}
                      </FormDescription>
                      <FormMessage />
                    </FormItem>
                  )}
                />

                <FormField
                  control={form.control}
                  name="streaming_idle_timeout"
                  render={({ field }) => (
                    <FormItem>
                      <FormLabel>
                        {t("proxy.settings.fields.streamingIdleTimeout.label", {
                          defaultValue: "流式静默超时（秒）",
                        })}
                      </FormLabel>
                      <FormControl>
                        <Input
                          type="number"
                          inputMode="numeric"
                          {...field}
                          onChange={(e) =>
                            field.onChange(parseInt(e.target.value, 10) || 0)
                          }
                          placeholder="60"
                          disabled={isLoading}
                        />
                      </FormControl>
                      <FormDescription>
                        {t(
                          "proxy.settings.fields.streamingIdleTimeout.description",
                          {
                            defaultValue: "数据块之间的最大间隔",
                          },
                        )}
                      </FormDescription>
                      <FormMessage />
                    </FormItem>
                  )}
                />

                <FormField
                  control={form.control}
                  name="non_streaming_timeout"
                  render={({ field }) => (
                    <FormItem>
                      <FormLabel>
                        {t("proxy.settings.fields.nonStreamingTimeout.label", {
                          defaultValue: "非流式超时（秒）",
                        })}
                      </FormLabel>
                      <FormControl>
                        <Input
                          type="number"
                          inputMode="numeric"
                          {...field}
                          onChange={(e) =>
                            field.onChange(parseInt(e.target.value, 10) || 0)
                          }
                          placeholder="300"
                          disabled={isLoading}
                        />
                      </FormControl>
                      <FormDescription>
                        {t(
                          "proxy.settings.fields.nonStreamingTimeout.description",
                          {
                            defaultValue: "非流式请求的总超时时间",
                          },
                        )}
                      </FormDescription>
                      <FormMessage />
                    </FormItem>
                  )}
                />
              </div>
            </section>
          </form>
        </Form>
      </div>
    </FullScreenPanel>
  );
}
