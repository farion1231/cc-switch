import { cn } from "@/lib/utils";

import type { TabKey } from "./constants";

type TabSwitcherItem = {
  key: TabKey;
  label: string;
};

type TabSwitcherProps = {
  tabs: TabSwitcherItem[];
  activeTab: TabKey;
  onSelect: (tab: TabKey) => void;
};

export const TabSwitcher = ({
  tabs,
  activeTab,
  onSelect,
}: TabSwitcherProps) => (
  <div className="px-4 py-3 border-b border-slate-100 bg-white/60 backdrop-blur-sm">
    <div className="flex gap-2 overflow-x-auto scrollbar-thin" data-tauri-no-drag>
      {tabs.map((tab) => {
        const isActive = tab.key === activeTab;
        return (
          <button
            key={tab.key}
            onClick={() => onSelect(tab.key)}
            className={cn(
              "px-3 h-7 rounded-lg text-[12px] leading-4 font-medium transition-all whitespace-nowrap flex-shrink-0",
              isActive
                ? "bg-slate-900 text-white shadow-lg shadow-slate-900/20"
                : "bg-white/60 text-slate-600 hover:bg-white hover:text-slate-900 border border-slate-200/60"
            )}
          >
            {tab.label}
          </button>
        );
      })}
    </div>
  </div>
);
