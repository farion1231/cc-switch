import { invoke } from "@tauri-apps/api/core";
import { useQuery } from "@tanstack/react-query";

export interface RequestLogDetailPayload {
  requestId: string;
  requestHeaders: string | null;
  requestBody: string | null;
  responseHeaders: string | null;
  responseBody: string | null;
}

export function getRequestDetailPayload(
  requestId: string,
): Promise<RequestLogDetailPayload | null> {
  return invoke("get_request_detail_payload", { requestId });
}

export function useRequestDetailPayload(requestId: string) {
  return useQuery({
    queryKey: ["usage", "detail-payload", requestId],
    queryFn: () => getRequestDetailPayload(requestId),
    enabled: !!requestId,
  });
}
