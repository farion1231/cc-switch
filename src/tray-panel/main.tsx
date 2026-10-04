import React from "react";
import ReactDOM from "react-dom/client";
import { QueryClientProvider } from "@tanstack/react-query";
import "../index.css";
import "./tray-panel.css";
import "../i18n";
import { queryClient } from "@/lib/query";
import { ThemeProvider } from "@/components/theme-provider";
import { FrontendErrorBoundary } from "@/components/FrontendErrorBoundary";
import { installGlobalErrorHandlers } from "@/lib/frontendLogger";
import { TrayPanel } from "./TrayPanel";

installGlobalErrorHandlers();

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <FrontendErrorBoundary>
      <QueryClientProvider client={queryClient}>
        <ThemeProvider defaultTheme="system" storageKey="cc-switch-theme">
          <TrayPanel />
        </ThemeProvider>
      </QueryClientProvider>
    </FrontendErrorBoundary>
  </React.StrictMode>,
);
