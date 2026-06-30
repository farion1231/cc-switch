import { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import {
  Globe,
  Play,
  Square,
  ExternalLink,
  Copy,
  Check,
  AlertCircle,
  Loader2,
  Key,
} from "lucide-react";
import { toast } from "sonner";
import { invoke } from "@tauri-apps/api/core";
import { isTauri } from "@/lib/environment";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { cn } from "@/lib/utils";

export function WebServerSettings() {
  const { t } = useTranslation();
  const desktopMode = isTauri();
  const [isRunning, setIsRunning] = useState(false);
  const [serverUrl, setServerUrl] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [copied, setCopied] = useState(false);
  const [copiedToken, setCopiedToken] = useState(false);
  const [bindAll, setBindAll] = useState(false);
  const [port, setPort] = useState(3001);
  const [token, setToken] = useState<string | null>(null);

  useEffect(() => {
    if (!desktopMode) {
      // In web mode the server is already running (that's how we got here).
      setIsRunning(true);
      setServerUrl(window.location.href);
      setPort(
        Number(window.location.port) ||
          (window.location.protocol === "https:" ? 443 : 80),
      );
      return;
    }

    checkServerStatus();
    const interval = setInterval(checkServerStatus, 5000);
    return () => clearInterval(interval);
  }, [desktopMode]);

  const checkServerStatus = async () => {
    if (!desktopMode) return;

    try {
      const config = await invoke<{
        running: boolean;
        port: number;
        bindAll: boolean;
        url: string | null;
        defaultPort: number;
        defaultBindAll: boolean;
      }>("get_web_server_config");

      setIsRunning(config.running);
      setServerUrl(config.url);

      if (config.running) {
        setBindAll(config.bindAll);
        setPort(config.port);
      } else {
        // Show defaults when not running so user can configure before start
        setPort(config.defaultPort);
        setBindAll(config.defaultBindAll);
      }
    } catch (error) {
      console.error("Failed to check web server status:", error);
    }
  };

  const handleStart = async () => {
    if (!desktopMode) return;
    setIsLoading(true);
    try {
      const url = await invoke<string>("start_web_server", {
        port,
        bindAll,
      });
      setServerUrl(url);
      setIsRunning(true);
      toast.success(
        t("settings.webServer.started", {
          defaultValue: "Web server started",
        }),
      );
    } catch (error) {
      console.error("Failed to start web server:", error);
      toast.error(
        t("settings.webServer.startFailed", {
          defaultValue: "Failed to start web server",
          error: String(error),
        }),
      );
    } finally {
      setIsLoading(false);
    }
  };

  const handleStop = async () => {
    if (!desktopMode) return;
    setIsLoading(true);
    try {
      await invoke("stop_web_server");
      setIsRunning(false);
      setServerUrl(null);
      setToken(null);
      toast.success(
        t("settings.webServer.stopped", {
          defaultValue: "Web server stopped",
        }),
      );
    } catch (error) {
      console.error("Failed to stop web server:", error);
      toast.error(
        t("settings.webServer.stopFailed", {
          defaultValue: "Failed to stop web server",
          error: String(error),
        }),
      );
    } finally {
      setIsLoading(false);
    }
  };

  const handleGenerateToken = async () => {
    if (!desktopMode) return;
    try {
      const newToken = await invoke<string>("generate_web_token");
      setToken(newToken);
      toast.success(
        t("settings.webServer.tokenGenerated", {
          defaultValue: "Access token generated",
        }),
      );
    } catch (error) {
      console.error("Failed to generate token:", error);
      toast.error(
        t("settings.webServer.tokenFailed", {
          defaultValue: "Failed to generate token",
        }),
      );
    }
  };

  const handleCopyToken = async () => {
    if (token) {
      try {
        await navigator.clipboard.writeText(token);
        setCopiedToken(true);
        setTimeout(() => setCopiedToken(false), 2000);
        toast.success(
          t("settings.webServer.tokenCopied", {
            defaultValue: "Token copied to clipboard",
          }),
        );
      } catch (error) {
        console.error("Failed to copy token:", error);
      }
    }
  };

  const handleOpenInBrowser = async () => {
    if (serverUrl) {
      try {
        const browserUrl = serverUrl.replace("0.0.0.0", "localhost");
        if (isTauri()) {
          const { openUrl } = await import("@tauri-apps/plugin-opener");
          await openUrl(browserUrl);
        } else {
          window.open(browserUrl, "_blank", "noopener,noreferrer");
        }
      } catch (error) {
        console.error("Failed to open browser:", error);
        toast.error(
          t("settings.webServer.openFailed", {
            defaultValue: "Failed to open browser",
          }),
        );
      }
    }
  };

  const handleCopyUrl = async () => {
    if (serverUrl) {
      try {
        await navigator.clipboard.writeText(serverUrl);
        setCopied(true);
        setTimeout(() => setCopied(false), 2000);
        toast.success(
          t("settings.webServer.urlCopied", {
            defaultValue: "URL copied to clipboard",
          }),
        );
      } catch (error) {
        console.error("Failed to copy URL:", error);
      }
    }
  };

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <Globe className="w-5 h-5" />
          {t("settings.webServer.title", {
            defaultValue: "Web Interface",
          })}
        </CardTitle>
        <CardDescription>
          {t("settings.webServer.description", {
            defaultValue:
              "Run a web server alongside the desktop app to access CC Switch from a browser",
          })}
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        {!desktopMode ? (
          <div className="rounded-md border border-blue-500/30 bg-blue-500/10 p-3 text-sm text-blue-800 dark:text-blue-300">
            {t("settings.webServer.webModeInfo", {
              defaultValue:
                "You are using the web interface. The server is managed by the host process.",
            })}
          </div>
        ) : (
          <div className="rounded-md border border-amber-500/30 bg-amber-500/10 p-3 text-sm text-amber-800 dark:text-amber-300">
            {t("settings.webServer.desktopOnly", {
              defaultValue:
                "Web server controls are available in desktop mode only. In Docker/web mode, use docker compose to manage service lifecycle.",
            })}
          </div>
        )}

        {/* Status */}
        <div className="flex items-center justify-between">
          <div className="space-y-1">
            <p className="text-sm font-medium">
              {t("settings.webServer.status", { defaultValue: "Status" })}
            </p>
            <p className="text-sm text-muted-foreground">
              {isRunning
                ? t("settings.webServer.running", {
                    defaultValue: "Web server is running",
                  })
                : t("settings.webServer.notRunning", {
                    defaultValue: "Web server is stopped",
                  })}
            </p>
          </div>
          <Badge
            variant={isRunning ? "default" : "secondary"}
            className={cn(
              isRunning &&
                "bg-emerald-500 hover:bg-emerald-600 dark:bg-emerald-600 dark:hover:bg-emerald-700",
            )}
          >
            {isRunning
              ? t("settings.webServer.statusRunning", {
                  defaultValue: "Running",
                })
              : t("settings.webServer.statusStopped", {
                  defaultValue: "Stopped",
                })}
          </Badge>
        </div>

        {/* Configuration — only editable when stopped */}
        {!isRunning && (
          <div className="space-y-3 p-3 border rounded-lg">
            <div className="space-y-2">
              <Label htmlFor="web-port">
                {t("settings.webServer.port", { defaultValue: "Port" })}
              </Label>
              <Input
                id="web-port"
                type="number"
                min={1024}
                max={65535}
                value={port}
                onChange={(e) => setPort(Number(e.target.value))}
                className="w-32"
              />
            </div>
            <div className="flex items-center justify-between">
              <div className="space-y-0.5">
                <Label htmlFor="bind-all">
                  {t("settings.webServer.bindAllLabel", {
                    defaultValue: "Allow remote access",
                  })}
                </Label>
                <p className="text-xs text-muted-foreground">
                  {t("settings.webServer.bindAllDescription", {
                    defaultValue:
                      "Bind to 0.0.0.0 to allow access from other devices on the network",
                  })}
                </p>
              </div>
              <Switch
                id="bind-all"
                checked={bindAll}
                onCheckedChange={setBindAll}
              />
            </div>
          </div>
        )}

        {/* Running state: URL + actions */}
        {isRunning && serverUrl && (
          <div className="space-y-2">
            <p className="text-sm font-medium">
              {t("settings.webServer.url", { defaultValue: "Server URL" })}
            </p>
            <div className="flex items-center gap-2">
              <code className="flex-1 px-3 py-2 text-sm bg-muted rounded-md font-mono">
                {serverUrl}
              </code>
              <Button
                variant="outline"
                size="icon"
                onClick={handleCopyUrl}
                title={t("settings.webServer.copyUrl", {
                  defaultValue: "Copy URL",
                })}
              >
                {copied ? (
                  <Check className="w-4 h-4" />
                ) : (
                  <Copy className="w-4 h-4" />
                )}
              </Button>
              <Button
                variant="outline"
                size="icon"
                onClick={handleOpenInBrowser}
                title={t("settings.webServer.openInBrowser", {
                  defaultValue: "Open in browser",
                })}
              >
                <ExternalLink className="w-4 h-4" />
              </Button>
            </div>
            {bindAll && (
              <p className="text-xs text-muted-foreground flex items-center gap-1">
                <AlertCircle className="w-3 h-3" />
                {t("settings.webServer.bindAllWarning", {
                  defaultValue:
                    "Server is accessible from any device on the network",
                })}
              </p>
            )}
          </div>
        )}

        {/* Access Token */}
        {desktopMode && isRunning && (
          <div className="space-y-2">
            <div className="flex items-center justify-between">
              <p className="text-sm font-medium">
                {t("settings.webServer.accessToken", {
                  defaultValue: "Access Token",
                })}
              </p>
              <Button variant="outline" size="sm" onClick={handleGenerateToken}>
                <Key className="w-3 h-3 mr-1" />
                {t("settings.webServer.generateToken", {
                  defaultValue: "Generate",
                })}
              </Button>
            </div>
            {token && (
              <div className="flex items-center gap-2">
                <code className="flex-1 px-3 py-2 text-xs bg-muted rounded-md font-mono break-all select-all">
                  {token}
                </code>
                <Button variant="outline" size="icon" onClick={handleCopyToken}>
                  {copiedToken ? (
                    <Check className="w-4 h-4" />
                  ) : (
                    <Copy className="w-4 h-4" />
                  )}
                </Button>
              </div>
            )}
            <p className="text-xs text-muted-foreground">
              {t("settings.webServer.tokenInfo", {
                defaultValue:
                  "Use this token to authenticate API requests to the web server. Tokens expire after 24 hours.",
              })}
            </p>
          </div>
        )}

        {/* Start/Stop button */}
        {desktopMode && (
          <div className="flex items-center gap-2">
            {isRunning ? (
              <Button
                variant="destructive"
                onClick={handleStop}
                disabled={isLoading}
                className="w-full"
              >
                {isLoading ? (
                  <Loader2 className="w-4 h-4 mr-2 animate-spin" />
                ) : (
                  <Square className="w-4 h-4 mr-2" />
                )}
                {t("settings.webServer.stop", { defaultValue: "Stop Server" })}
              </Button>
            ) : (
              <Button
                onClick={handleStart}
                disabled={isLoading}
                className="w-full"
              >
                {isLoading ? (
                  <Loader2 className="w-4 h-4 mr-2 animate-spin" />
                ) : (
                  <Play className="w-4 h-4 mr-2" />
                )}
                {t("settings.webServer.start", {
                  defaultValue: "Start Server",
                })}
              </Button>
            )}
          </div>
        )}

        <div className="p-3 bg-muted rounded-lg">
          <p className="text-xs text-muted-foreground">
            {t("settings.webServer.info", {
              defaultValue:
                "The web interface allows you to access CC Switch from any browser. CLI: cc-switch --enable-web --web-port 3001 --bind-all",
            })}
          </p>
        </div>
      </CardContent>
    </Card>
  );
}
