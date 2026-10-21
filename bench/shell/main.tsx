import React, { useState } from "react";
import { createRoot } from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { Sidebar } from "@/components/shell/Sidebar";
import { PageFrame } from "@/components/shell/PageFrame";
import { APP_IDS } from "@/config/appConfig";
import type { AppId } from "@/lib/api";
import type { View, SettingsSection } from "@/lib/navigation";
import type { VisibleApps } from "@/types";
import i18n from "@/i18n";
import "@/index.css";

const query = new QueryClient({
  defaultOptions: {
    queries: { staleTime: Infinity, retry: false, refetchOnWindowFocus: false },
  },
});
query.setQueryData(["settings"], {});
const apps = Object.fromEntries(APP_IDS.map((id) => [id, true])) as VisibleApps;
const frame = () =>
  new Promise<number>((resolve) => requestAnimationFrame(resolve));
const pause = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
const percentile = (values: number[], p: number) =>
  [...values].sort((a, b) => a - b)[
    Math.min(values.length - 1, Math.floor(values.length * p))
  ] ?? 0;
const summary = (values: number[]) => ({
  n: values.length,
  p50: +percentile(values, 0.5).toFixed(2),
  p95: +percentile(values, 0.95).toFixed(2),
  max: +Math.max(0, ...values).toFixed(2),
});

function Benchmark() {
  const [app, setApp] = useState<AppId>("claude");
  const [view, setView] = useState<View>("providers");
  const [section, setSection] = useState<SettingsSection>("general");
  const [result, setResult] = useState("");
  const [running, setRunning] = useState(false);
  const [rows, setRows] = useState(300);
  const selectApp = (id: AppId) => {
    setApp(id);
    setView("providers");
  };
  const run = async () => {
    setRunning(true);
    setResult("");
    await pause(500);
    const phases: Record<string, unknown> = {};
    let backgroundFrames = 0;
    for (const phase of ["navigation", "resize"] as const) {
      const frames: number[] = [],
        paints: number[] = [];
      let collecting = true,
        previous = await frame();
      const sample = (time: number) => {
        if (document.hidden || !document.hasFocus()) backgroundFrames++;
        frames.push(time - previous);
        previous = time;
        if (collecting) requestAnimationFrame(sample);
      };
      requestAnimationFrame(sample);

      if (phase === "navigation") {
        for (let n = 0; n < 60; n++) {
          const start = performance.now();
          if (n % 6 === 4) setView("apps");
          else if (n % 6 === 5) {
            setView("settings");
            setSection(n % 2 ? "network" : "general");
          } else selectApp(APP_IDS[n % APP_IDS.length]);
          await frame();
          await frame();
          paints.push(performance.now() - start);
          await pause(220);
        }
      } else {
        const nav = document.querySelector("nav")!;
        // Equal layout workload before and after. Pointer interaction is tested separately.
        for (let n = 0; n < 180; n++) {
          await frame();
          nav.style.width = `${200 + Math.round(90 * Math.sin((n / 180) * Math.PI))}px`;
        }
        nav.style.width = "200px";
      }
      collecting = false;
      await frame();
      phases[phase] = {
        frameMs: summary(frames),
        inputToTwoFramesMs: summary(paints),
        framesOver50ms: frames.filter((n) => n > 50).length,
      };
    }
    setResult(
      JSON.stringify(
        {
          timestamp: new Date().toISOString(),
          userAgent: navigator.userAgent,
          viewport: [innerWidth, innerHeight],
          reducedMotion: matchMedia("(prefers-reduced-motion: reduce)").matches,
          rows,
          backgroundFrames,
          validForegroundRun: backgroundFrames === 0,
          phases,
        },
        null,
        2,
      ),
    );
    setRunning(false);
  };
  return (
    <div className="flex h-screen overflow-hidden bg-app text-fg-1">
      <Sidebar
        activeApp={app}
        view={view}
        visibleApps={apps}
        settingsSection={section}
        onSelectApp={selectApp}
        onSelectPage={setView}
        onSelectSettingsSection={setSection}
        onExitSettings={() => setView("providers")}
      />
      <main
        id="content-area"
        className="relative isolate flex min-w-0 flex-1 flex-col overflow-hidden bg-app"
      >
        <PageFrame
          key={view === "providers" ? "app" : view}
          transitionKey={`${view}:${view === "providers" ? app : section}`}
        >
          <header className="flex h-12 shrink-0 items-center justify-between border-b border-border px-6">
            <strong>
              {view} / {view === "providers" ? app : section}
            </strong>
            <span>Controlled shell fixture · no IPC</span>
          </header>
          <div className="flex-1 overflow-auto p-6" data-bench-content>
            {Array.from({ length: rows }, (_, n) => (
              <div
                key={n}
                className="mb-2 flex h-16 items-center justify-between rounded-panel border border-border px-4"
              >
                <span>
                  Provider {n + 1} · {app}
                </span>
                <span className="text-fg-2">
                  Model · https://example.test/v1
                </span>
                <button>Configure</button>
              </div>
            ))}
          </div>
        </PageFrame>
      </main>

      <aside className="fixed bottom-4 right-4 z-50 max-h-[60vh] w-[360px] overflow-auto rounded-panel border border-border bg-surface p-4 shadow-lg">
        <label>
          Rows{" "}
          <select
            disabled={running}
            value={rows}
            onChange={(e) => setRows(+e.target.value)}
          >
            <option>30</option>
            <option>300</option>
            <option>1000</option>
          </select>
        </label>
        <button
          className="ml-4 rounded-control bg-action px-3 py-2 text-action-fg"
          disabled={running}
          onClick={run}
        >
          {running ? "Running…" : "Run benchmark"}
        </button>
        <pre
          className="mt-2 whitespace-pre-wrap text-xs"
          aria-label="Benchmark result"
        >
          {result.split("\n").map((line, index) => (
            <span key={index} style={{ display: "block" }}>
              {line}
            </span>
          ))}
        </pre>
      </aside>
    </div>
  );
}
void i18n.changeLanguage("en").then(() =>
  createRoot(document.getElementById("root")!).render(
    <QueryClientProvider client={query}>
      <Benchmark />
    </QueryClientProvider>,
  ),
);
