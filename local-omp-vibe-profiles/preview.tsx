import { useState } from "react";
import { createRoot } from "react-dom/client";
import { Moon, Sun } from "lucide-react";
import { AppGlyph } from "@/components/shell/AppGlyph";
import { AppPageHeader } from "@/components/shell/AppPageHeader";
import { Button } from "@/components/ui/button";
import "@/index.css";
import {
  OmpVibeProfilesAction,
  type VibeProfilesClient,
} from "./OmpVibeProfilesAction";

async function request(route: string, init?: RequestInit) {
  const response = await fetch(route, init);
  const payload = await response.json();
  if (!response.ok) throw new Error(payload.error);
  return payload;
}

const client: VibeProfilesClient = {
  get: () => request("/vibe-profiles"),
  listModels: () => request("/vibe-models"),
  change: (input) =>
    request("/vibe-profiles", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(input),
    }),
};

function Preview() {
  const [dark, setDark] = useState(false);
  return (
    <div className="min-h-screen bg-app text-fg-1">
      <AppPageHeader
        variant="app"
        icon={<AppGlyph app="omp" size={20} badgeClassName="bg-app" />}
        title="Oh My Pi"
        actions={
          <>
            <OmpVibeProfilesAction client={client} />
            <Button
              variant="quiet"
              size="icon-compact"
              aria-label="切换预览主题"
              title="切换预览主题"
              onClick={() => {
                document.documentElement.classList.toggle("dark", !dark);
                setDark(!dark);
              }}
            >
              {dark ? (
                <Sun className="h-4 w-4" />
              ) : (
                <Moon className="h-4 w-4" />
              )}
            </Button>
          </>
        }
      />
      <main className="px-6 pt-5">
        <h1 className="text-body font-semibold">供应商</h1>
        <p className="mt-2 text-xs text-fg-3">
          {__VIBE_LIVE__ ? "本地 OMP 配置" : "隔离预览 · 不读写真实 OMP 配置"}
        </p>
      </main>
    </div>
  );
}

createRoot(document.getElementById("root")!).render(<Preview />);

declare const __VIBE_LIVE__: boolean;
