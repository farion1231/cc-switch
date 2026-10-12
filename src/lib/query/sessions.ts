import { useEffect, useMemo, useState } from "react";
import {
  useInfiniteQuery,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import { sessionsApi } from "@/lib/api/sessions";
import type {
  ContentRef,
  ImageRef,
  SessionMessage,
  TranscriptChunk,
} from "@/types";
import { buildTurnIndex, buildTurns } from "@/components/sessions/reader/turns";

/**
 * 会话阅读页的数据层：
 * 分块流式读取会话、按需取块全文、图片转 Blob URL。
 */

export const sessionKeys = {
  messages: (providerId?: string, sourcePath?: string) =>
    ["sessionMessages", providerId, sourcePath] as const,
  transcript: (providerId?: string, sourcePath?: string) =>
    ["sessionTranscript", providerId, sourcePath] as const,
  blockContent: (providerId?: string, sourcePath?: string, ref?: ContentRef) =>
    [
      "sessionBlockContent",
      providerId,
      sourcePath,
      ref ? stableKey(ref) : null,
    ] as const,
};

/** 键顺序无关的序列化：ContentRef / ImageRef 做 query key、缓存 key 用 */
export const stableKey = (value: unknown): string => {
  if (Array.isArray(value)) return `[${value.map(stableKey).join(",")}]`;
  if (value && typeof value === "object") {
    const entries = Object.entries(value as Record<string, unknown>)
      .filter(([, item]) => item !== undefined)
      .sort(([a], [b]) => (a < b ? -1 : 1))
      .map(([key, item]) => `${JSON.stringify(key)}:${stableKey(item)}`);
    return `{${entries.join(",")}}`;
  }
  return JSON.stringify(value);
};

// ─── 一次性读取（旧接口，复制整段等场景） ─────────────────────────────────

export const useSessionMessagesQuery = (
  providerId?: string,
  sourcePath?: string,
) => {
  return useQuery<SessionMessage[]>({
    queryKey: sessionKeys.messages(providerId, sourcePath),
    queryFn: async () => sessionsApi.getMessages(providerId!, sourcePath!),
    enabled: Boolean(providerId && sourcePath),
    staleTime: 30 * 1000,
  });
};

// ─── 分块流式读取 ─────────────────────────────────────────────────────────

export type TranscriptHeader = Extract<TranscriptChunk, { type: "header" }>;

export interface TranscriptState {
  header: TranscriptHeader | null;
  messages: SessionMessage[];
  /** 已收到 Done（或回退到一次性读取） */
  done: boolean;
  payloadBytes?: number;
}

export const EMPTY_TRANSCRIPT: TranscriptState = {
  header: null,
  messages: [],
  done: false,
};

/** 把一个 chunk 合进累积状态（不可变更新）；Error chunk 抛错 */
export const applyTranscriptChunk = (
  state: TranscriptState,
  chunk: TranscriptChunk,
): TranscriptState => {
  switch (chunk.type) {
    case "header":
      return { ...state, header: chunk };
    case "messages": {
      const messages = state.messages.slice(0, chunk.start);
      messages.push(...chunk.messages);
      return { ...state, messages };
    }
    case "done":
      return { ...state, done: true, payloadBytes: chunk.payloadBytes };
    case "error":
      throw new Error(chunk.message);
  }
};

/** 命令返回后再等 Done 的宽限时间：Channel 消息与命令返回值不保证先后 */
export const TRANSCRIPT_DONE_GRACE_MS = 3000;

/** 旧后端（没有 stream_session_messages）回退：一次性读取 + 前端自算轮次索引 */
const loadTranscriptOnce = async (
  providerId: string,
  sourcePath: string,
): Promise<TranscriptState> => {
  const messages = await sessionsApi.getMessages(providerId, sourcePath);
  return {
    header: {
      type: "header",
      total: messages.length,
      turns: buildTurnIndex(buildTurns(messages)),
      cached: false,
      parseMs: 0,
    },
    messages,
    done: true,
  };
};

/**
 * 读取整份会话：每收到一个 chunk 回调一次累积状态，Done 后 resolve。
 * 一个 chunk 都没收到就失败时回退到 get_session_messages。
 */
export const loadTranscript = async (
  providerId: string,
  sourcePath: string,
  onProgress?: (state: TranscriptState) => void,
): Promise<TranscriptState> => {
  let state = EMPTY_TRANSCRIPT;
  let received = false;
  let failure: unknown;
  let settle: () => void = () => {};
  const finished = new Promise<void>((resolve) => {
    settle = resolve;
  });

  try {
    await sessionsApi.streamMessages(providerId, sourcePath, (chunk) => {
      received = true;
      try {
        state = applyTranscriptChunk(state, chunk);
      } catch (error) {
        failure = error;
        settle();
        return;
      }
      onProgress?.(state);
      if (state.done) settle();
    });
  } catch (error) {
    if (!received) return loadTranscriptOnce(providerId, sourcePath);
    throw error;
  }

  let timer: ReturnType<typeof setTimeout> | undefined;
  await Promise.race([
    finished,
    new Promise<void>((resolve) => {
      timer = setTimeout(resolve, TRANSCRIPT_DONE_GRACE_MS);
    }),
  ]);
  clearTimeout(timer);
  if (failure) throw failure;
  return { ...state, done: true };
};

export interface SessionTranscriptResult {
  header: TranscriptHeader | null;
  messages: SessionMessage[];
  /** 正在接收首份数据（后台刷新不算） */
  isStreaming: boolean;
  progress: { loaded: number; total: number | null };
  isLoading: boolean;
  isError: boolean;
  error: unknown;
  refetch: () => Promise<unknown>;
}

/**
 * stream_session_messages 的 TanStack Query 封装：queryFn 里每收到一个 chunk 就
 * setQueryData，首包到达即可渲染。后台刷新时不推中间态（避免已显示的内容闪回半截）。
 */
export const useSessionTranscript = (
  providerId?: string,
  sourcePath?: string,
): SessionTranscriptResult => {
  const queryClient = useQueryClient();
  const queryKey = sessionKeys.transcript(providerId, sourcePath);
  const query = useQuery<TranscriptState>({
    queryKey,
    queryFn: async () => {
      const previous = queryClient.getQueryData<TranscriptState>(queryKey);
      const live = !previous?.done;
      return loadTranscript(providerId!, sourcePath!, (state) => {
        if (live) queryClient.setQueryData(queryKey, state);
      });
    },
    enabled: Boolean(providerId && sourcePath),
    staleTime: 30 * 1000,
  });
  const data = query.data ?? EMPTY_TRANSCRIPT;
  return {
    header: data.header,
    messages: data.messages,
    isStreaming: query.isFetching && !data.done,
    progress: {
      loaded: data.messages.length,
      total: data.header?.total ?? null,
    },
    isLoading: query.isLoading,
    isError: query.isError,
    error: query.error,
    refetch: query.refetch,
  };
};

// ─── 块全文 ───────────────────────────────────────────────────────────────

/** 单次取的字符数（§5.1 默认 512KB） */
export const BLOCK_PAGE_CHARS = 512 * 1024;
/** 前端累计超过 2MB 停止加载，提示复制源文件路径（规则 12） */
export const BLOCK_MAX_CHARS = 2 * 1024 * 1024;

export interface BlockContentResult {
  text: string;
  totalLen: number | null;
  /** 还能「加载更多」 */
  hasMore: boolean;
  /** 已到 2MB 上限但后面还有内容 */
  tooLarge: boolean;
  loadMore: () => void;
  isLoading: boolean;
  isFetchingMore: boolean;
  error: unknown;
}

/** get_session_block_content：按需取全文，分页「加载更多」，结果永不过期 */
export const useBlockContent = (
  providerId: string | undefined,
  sourcePath: string | undefined,
  ref: ContentRef | undefined,
  options: { enabled?: boolean } = {},
): BlockContentResult => {
  const query = useInfiniteQuery({
    queryKey: sessionKeys.blockContent(providerId, sourcePath, ref),
    queryFn: ({ pageParam }) =>
      sessionsApi.getBlockContent(providerId!, sourcePath!, ref!, {
        offset: pageParam,
        limit: BLOCK_PAGE_CHARS,
      }),
    initialPageParam: 0,
    getNextPageParam: (last, pages) => {
      const loaded = pages.reduce((sum, page) => sum + page.text.length, 0);
      return last.truncated &&
        last.nextOffset !== undefined &&
        loaded < BLOCK_MAX_CHARS
        ? last.nextOffset
        : undefined;
    },
    enabled:
      Boolean(providerId && sourcePath && ref) && options.enabled !== false,
    staleTime: Infinity,
  });
  const pages = query.data?.pages ?? [];
  const text = useMemo(() => pages.map((page) => page.text).join(""), [pages]);
  const last = pages[pages.length - 1];
  return {
    text,
    totalLen: last?.totalLen ?? null,
    hasMore: Boolean(query.hasNextPage),
    tooLarge: Boolean(last?.truncated) && !query.hasNextPage,
    loadMore: () => {
      void query.fetchNextPage();
    },
    isLoading: query.isLoading,
    isFetchingMore: query.isFetchingNextPage,
    error: query.error,
  };
};

// ─── 图片 ─────────────────────────────────────────────────────────────────

/** Blob URL 缓存上限（§6.3：LRU 32 张） */
export const SESSION_IMAGE_CACHE_SIZE = 32;

interface CachedImage {
  url: string;
  refs: number;
}

const imageCache = new Map<string, CachedImage>();
const imageLoads = new Map<string, Promise<string>>();
/** 每张图还在等加载结果、尚未占用的等待方个数；有等待方的图不淘汰 */
const imageWaiters = new Map<string, number>();

const startWaiting = (key: string) =>
  imageWaiters.set(key, (imageWaiters.get(key) ?? 0) + 1);

const stopWaiting = (key: string) => {
  const left = (imageWaiters.get(key) ?? 0) - 1;
  if (left > 0) imageWaiters.set(key, left);
  else imageWaiters.delete(key);
};

/**
 * 超出上限时按最久未用淘汰没人在用的图并 revoke。
 * 加载完、等待方还没来得及占用的图也算在用（几张图同时加载完时会互相淘汰）
 */
const evictImages = () => {
  for (const [key, entry] of imageCache) {
    if (imageCache.size <= SESSION_IMAGE_CACHE_SIZE) return;
    if (entry.refs > 0 || imageWaiters.has(key)) continue;
    URL.revokeObjectURL(entry.url);
    imageCache.delete(key);
  }
};

const loadImageUrl = (
  key: string,
  providerId: string,
  sourcePath: string,
  image: ImageRef,
) => {
  let pending = imageLoads.get(key);
  if (!pending) {
    pending = sessionsApi
      .getImage(providerId, sourcePath, image)
      .then((bytes) => {
        const url = URL.createObjectURL(
          new Blob([bytes], { type: image.mediaType }),
        );
        imageCache.set(key, { url, refs: 0 });
        evictImages();
        return url;
      })
      .finally(() => imageLoads.delete(key));
    imageLoads.set(key, pending);
  }
  return pending;
};

/** 取出并占用缓存项（移到 LRU 末尾） */
const acquireImage = (key: string) => {
  const entry = imageCache.get(key);
  if (!entry) return undefined;
  imageCache.delete(key);
  imageCache.set(key, { ...entry, refs: entry.refs + 1 });
  return entry.url;
};

const releaseImage = (key: string) => {
  const entry = imageCache.get(key);
  if (entry) entry.refs = Math.max(0, entry.refs - 1);
  evictImages();
};

/** 清空图片缓存并 revoke 全部 Blob URL（离开阅读页、测试用） */
export const clearSessionImageCache = () => {
  imageCache.forEach((entry) => URL.revokeObjectURL(entry.url));
  imageCache.clear();
  imageLoads.clear();
  imageWaiters.clear();
};

export interface SessionImageResult {
  url: string | null;
  isLoading: boolean;
  error: unknown;
}

/**
 * get_session_image → Blob URL。enabled 交给调用方（进入视口才开，规则 13）；
 * 同一张图多处使用共享一个 URL，卸载后留在 LRU 里，被淘汰时才 revoke。
 */
export const useSessionImage = (
  providerId: string | undefined,
  sourcePath: string | undefined,
  image: ImageRef | undefined,
  options: { enabled?: boolean } = {},
): SessionImageResult => {
  const enabled =
    Boolean(providerId && sourcePath && image) && options.enabled !== false;
  const key = image ? `${providerId}|${sourcePath}|${stableKey(image)}` : "";
  const [state, setState] = useState<SessionImageResult>({
    url: null,
    isLoading: false,
    error: null,
  });

  useEffect(() => {
    if (!enabled) return;
    let cancelled = false;
    let acquired = false;
    const take = () => {
      const url = acquireImage(key);
      acquired = Boolean(url);
      setState({ url: url ?? null, isLoading: false, error: null });
    };
    if (imageCache.has(key)) {
      take();
    } else {
      setState({ url: null, isLoading: true, error: null });
      startWaiting(key);
      loadImageUrl(key, providerId!, sourcePath!, image!).then(
        () => {
          stopWaiting(key);
          // 已卸载就不占用；不再等它了，补一次淘汰以免超出上限
          if (cancelled) evictImages();
          else take();
        },
        (error: unknown) => {
          stopWaiting(key);
          if (!cancelled) setState({ url: null, isLoading: false, error });
        },
      );
    }
    return () => {
      cancelled = true;
      if (acquired) releaseImage(key);
    };
    // image 内容已体现在 key 里
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [enabled, key]);

  return enabled ? state : { url: null, isLoading: false, error: null };
};
