import React from "react";
import NativePromptPanel, {
  type PromptPrimaryAction,
  type NativePromptTab,
} from "./NativePromptPanel";

export type OhMyPiPromptTab = NativePromptTab;
export type { PromptPrimaryAction } from "./NativePromptPanel";

export interface OhMyPiPromptPanelHandle {
  openAdd: () => void;
}

interface OhMyPiPromptPanelProps {
  open: boolean;
  onInteractionBlockedChange?: (blocked: boolean) => void;
  onNavigationBlockedChange?: (blocked: boolean) => void;
  onPrimaryActionChange?: (action: PromptPrimaryAction) => void;
}

const OhMyPiPromptPanel = React.forwardRef<
  OhMyPiPromptPanelHandle,
  OhMyPiPromptPanelProps
>((props, ref) => <NativePromptPanel ref={ref} appId="ohmypi" {...props} />);

OhMyPiPromptPanel.displayName = "OhMyPiPromptPanel";

export default OhMyPiPromptPanel;
