import { useState } from "react";
import { useTranslation } from "react-i18next";
import { motion } from "framer-motion";
import { toast } from "sonner";
import { Key, Loader2, Terminal } from "lucide-react";
import logoSrc from "@tauri-icons/icon.png";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { authApi } from "@/lib/api";
import { setAuthToken } from "@/lib/api/web-client";
import { webLog } from "@/lib/webLogger";

interface LoginPageProps {
  onLogin: () => void;
}

export function LoginPage({ onLogin }: LoginPageProps) {
  const { t } = useTranslation();
  const [token, setToken] = useState("");
  const [isLoading, setIsLoading] = useState(false);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();

    if (!token.trim()) {
      toast.error(
        t("login.tokenRequired", {
          defaultValue: "Please enter your auth token",
        }),
      );
      return;
    }

    setIsLoading(true);
    webLog.info("login: verifying token");

    try {
      const jwt = await authApi.login(token.trim());
      setAuthToken(jwt);
      webLog.info("login: success");
      toast.success(t("login.success", { defaultValue: "Login successful" }));
      onLogin();
    } catch (error) {
      webLog.warn("login: failed", {
        error: error instanceof Error ? error.message : String(error),
      });
      toast.error(
        t("login.error", {
          defaultValue: "Login failed",
          error: error instanceof Error ? error.message : "Unknown error",
        }),
      );
    } finally {
      setIsLoading(false);
    }
  };

  return (
    <div className="min-h-screen flex items-center justify-center bg-gradient-to-br from-background to-muted p-4">
      <motion.div
        initial={{ opacity: 0, y: 20 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ duration: 0.5 }}
        className="w-full max-w-md"
      >
        <Card className="border-2 shadow-xl">
          <CardHeader className="space-y-1 text-center">
            <div className="flex justify-center mb-4">
              <img
                src={logoSrc}
                alt={t("login.logoAlt", { defaultValue: "CC Switch" })}
                className="w-16 h-16 rounded-full object-cover shadow-lg"
              />
            </div>
            <CardTitle className="text-2xl font-bold">CC Switch</CardTitle>
            <CardDescription>
              {t("login.subtitle", { defaultValue: "Web Management Console" })}
            </CardDescription>
          </CardHeader>
          <CardContent>
            <form onSubmit={handleSubmit} className="space-y-4">
              <div className="space-y-2">
                <Label htmlFor="token">
                  {t("login.token", { defaultValue: "Auth Token" })}
                </Label>
                <div className="relative">
                  <Key className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
                  <Input
                    id="token"
                    type="password"
                    placeholder={t("login.tokenPlaceholder", {
                      defaultValue: "Paste your auth token here",
                    })}
                    value={token}
                    onChange={(e) => setToken(e.target.value)}
                    className="pl-10"
                    disabled={isLoading}
                  />
                </div>
              </div>
              <Button type="submit" className="w-full" disabled={isLoading}>
                {isLoading ? (
                  <>
                    <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                    {t("login.loggingIn", { defaultValue: "Verifying..." })}
                  </>
                ) : (
                  t("login.submit", { defaultValue: "Sign In" })
                )}
              </Button>
            </form>

            <div className="mt-6 p-4 bg-muted rounded-lg">
              <div className="flex items-start gap-3">
                <Terminal className="h-5 w-5 text-muted-foreground mt-0.5" />
                <div className="text-sm text-muted-foreground">
                  <p className="font-medium text-foreground mb-1">
                    {t("login.cliInstructions", {
                      defaultValue: "Find or rotate your auth token:",
                    })}
                  </p>
                  <code className="block bg-background px-2 py-1 rounded text-xs">
                    cc-switch rotate-token
                  </code>
                </div>
              </div>
            </div>
          </CardContent>
        </Card>

        <p className="text-center text-sm text-muted-foreground mt-6">
          {t("login.tokenHelp", {
            defaultValue:
              "Check the server logs for the initial token, or run rotate-token to generate a new one",
          })}
        </p>
      </motion.div>
    </div>
  );
}

export default LoginPage;
