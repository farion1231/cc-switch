import React from "react";
import ReactDOM from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import "../index.css";
import "./tray-panel.css";
import "../i18n";
import { FrontendErrorBoundary } from "@/components/FrontendErrorBoundary";
import { installGlobalErrorHandlers } from "@/lib/frontendLogger";
import { TrayPanel } from "./TrayPanel";

installGlobalErrorHandlers();

const queryClient = new QueryClient({
  defaultOptions: {
    queries: { retry: 1, refetchOnWindowFocus: false },
  },
});

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <FrontendErrorBoundary>
      <QueryClientProvider client={queryClient}>
        <TrayPanel />
      </QueryClientProvider>
    </FrontendErrorBoundary>
  </React.StrictMode>,
);
