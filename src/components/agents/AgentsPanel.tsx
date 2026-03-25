import { Bot } from "lucide-react";

interface AgentsPanelProps {
  onOpenChange: (open: boolean) => void;
}

export function AgentsPanel({}: AgentsPanelProps) {
  return (
    <div className="px-6 flex flex-col flex-1 min-h-0">
      <div className="flex-1 glass-card rounded-xl p-8 flex flex-col items-center justify-center text-center space-y-4">
        <div className="mb-4 flex h-20 w-20 items-center justify-center rounded-full bg-background/65 ring-1 ring-border animate-pulse-slow">
          <Bot className="w-10 h-10 text-muted-foreground" />
        </div>
        <h3 className="text-xl font-semibold">Coming Soon</h3>
        <p className="text-muted-foreground max-w-md">
          The Agents management feature is currently under development. Stay
          tuned for powerful autonomous capabilities.
        </p>
      </div>
    </div>
  );
}
