import type { TaskGateway } from "../types";
import { tauriTaskGateway } from "./tauriTaskGateway";

export function createTaskGateway(): TaskGateway {
  return tauriTaskGateway;
}
