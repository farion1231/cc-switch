import { forwardRef, useImperativeHandle, useRef } from "react";

import {
  CopilotByokSettings,
  type CopilotByokSettingsHandle,
} from "./CopilotByokSettings";

export interface CopilotCliSettingsHandle {
  openAdd: () => void;
}

interface CopilotCliSettingsProps {
  onOpenWebsite?: (url: string) => void;
}

export const CopilotCliSettings = forwardRef<
  CopilotCliSettingsHandle,
  CopilotCliSettingsProps
>(function CopilotCliSettings({ onOpenWebsite }, ref) {
  const catalogRef = useRef<CopilotByokSettingsHandle>(null);

  useImperativeHandle(
    ref,
    () => ({ openAdd: () => catalogRef.current?.openAdd() }),
    [],
  );

  return (
    <CopilotByokSettings
      ref={catalogRef}
      mode="catalog"
      catalogApp="copilot-cli"
      onOpenWebsite={onOpenWebsite}
    />
  );
});
