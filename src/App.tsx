import { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Provider } from "./types";
import { AppType } from "./lib/query";
import { useProvidersQuery, useAddProviderMutation, useUpdateProviderMutation, useVSCodeSyncMutation } from "./lib/query";
import ProviderList from "./components/ProviderList";
import AddProviderModal from "./components/AddProviderModal";
import EditProviderModal from "./components/EditProviderModal";
import { ConfirmDialog } from "./components/ConfirmDialog";
import { AppSwitcher } from "./components/AppSwitcher";
import SettingsModal from "./components/SettingsModal";
import { UpdateBadge } from "./components/UpdateBadge";
import { Plus, Settings } from "lucide-react";
import { buttonStyles } from "./lib/styles";
import { ModeToggle } from "./components/mode-toggle";
import { useTheme } from "./components/theme-provider";
import { extractErrorMessage } from "./utils/errorUtils";
import { useVSCodeAutoSync } from "./hooks/useVSCodeAutoSync";
import { useQueryClient } from "@tanstack/react-query";
import tauriAPI from "./lib/tauri-api";
import { Toaster } from "./components/ui/sonner";
import { Tabs, TabsList, TabsTrigger, TabsContent } from "./components/ui/tabs";

import { Claude, OpenAI } from '@lobehub/icons'

function App() {
  const { t } = useTranslation();
  const [activeApp, setActiveApp] = useState<AppType | string>("claude");

  const providersQuery = useProvidersQuery(activeApp as AppType);

  return (
    <div>
      <div className="p-5">
        <Tabs onValueChange={setActiveApp} value={activeApp} className="w-[400px]">
          <TabsList>
            <TabsTrigger value="claude" className="flex items-center gap-1">
              <Claude.Color />
              Claude Code</TabsTrigger>
            <TabsTrigger value="codex" className="flex items-center gap-1">
              <OpenAI />
              Codex</TabsTrigger>
          </TabsList>
        </Tabs>

        <ProviderList
          appType={activeApp as AppType}
          providers={providersQuery.data?.providers || {}}
        />
      </div>
    </div>
  )
}

export default App;
