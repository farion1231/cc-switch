import React from "react";
import ReactDOM from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { ThemeProvider } from "@/components/theme-provider";
import { Toaster } from "@/components/ui/sonner";
import "@/index.css";
import { demoTaskGateway } from "../api/demoTaskGateway";
import { TaskGatewayProvider } from "../api/TaskGatewayProvider";
import { TandemShell } from "../components/TandemShell";
import { DemoLegacyConfigApp } from "./DemoLegacyConfigApp";

const queryClient = new QueryClient({
  defaultOptions: { queries: { retry: false } },
});

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <QueryClientProvider client={queryClient}>
      <ThemeProvider defaultTheme="system" storageKey="tandem-demo-theme">
        <TaskGatewayProvider gateway={demoTaskGateway}>
          <TandemShell LegacyConfigApp={DemoLegacyConfigApp} />
        </TaskGatewayProvider>
        <Toaster />
      </ThemeProvider>
    </QueryClientProvider>
  </React.StrictMode>,
);
