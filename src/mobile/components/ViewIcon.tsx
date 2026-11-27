import {
  Activity,
  BarChart3,
  Cpu,
  Layers,
  ScrollText,
  SlidersHorizontal,
  Terminal,
} from "lucide-react";
import type { MobileViewId } from "../shortcuts";

export function ViewIcon({
  id,
  className = "h-5 w-5",
}: {
  id: MobileViewId;
  className?: string;
}) {
  switch (id) {
    case "overview":
      return <BarChart3 className={className} />;
    case "providers":
      return <Layers className={className} />;
    case "logs":
      return <ScrollText className={className} />;
    case "proxy":
      return <Activity className={className} />;
    case "remote":
      return <Terminal className={className} />;
    case "models":
      return <Cpu className={className} />;
    case "preferences":
      return <SlidersHorizontal className={className} />;
  }
}
