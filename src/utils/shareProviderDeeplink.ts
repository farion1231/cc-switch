import { deeplinkApi } from "@/lib/api/deeplink";
import { copyText } from "@/lib/clipboard";
import type { AppId } from "@/lib/api";

export async function shareProviderDeeplink(
  appId: AppId,
  providerId: string,
): Promise<string> {
  const url = await deeplinkApi.generateProviderDeeplink(appId, providerId);
  await copyText(url);
  return url;
}
