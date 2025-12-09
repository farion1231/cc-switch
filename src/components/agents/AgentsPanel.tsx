import { Bot } from "lucide-react";

interface AgentsPanelProps {
  onOpenChange: (open: boolean) => void;
}

export function AgentsPanel({}: AgentsPanelProps) {
  return (
    <div className="mx-auto max-w-5xl flex flex-col h-[calc(100vh-8rem)]">
      <div className="flex-1 glass-card rounded-xl p-8 flex flex-col items-center justify-center text-center space-y-4">
        <div className="w-20 h-20 rounded-full bg-white/5 flex items-center justify-center mb-4 animate-pulse-slow">
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
