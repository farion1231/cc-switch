import { createContext, useContext, type ReactNode } from "react";
import type { TaskGateway } from "../types";

const TaskGatewayContext = createContext<TaskGateway | null>(null);

export function TaskGatewayProvider({
  gateway,
  children,
}: {
  gateway: TaskGateway;
  children: ReactNode;
}) {
  return (
    <TaskGatewayContext.Provider value={gateway}>
      {children}
    </TaskGatewayContext.Provider>
  );
}

export function useTaskGateway(): TaskGateway {
  const gateway = useContext(TaskGatewayContext);
  if (!gateway) {
    throw new Error("TaskGatewayProvider is required");
  }
  return gateway;
}
