import { memo, useEffect, useRef, useState } from "react";
import { ImageOff } from "lucide-react";

import { useSessionImage } from "@/lib/query/sessions";
import { cn } from "@/lib/utils";
import type { ImageRef } from "@/types";
import { useReaderContext } from "./context";
import { useReaderT } from "./i18n";
import { PathChip } from "./PathChip";
import { SessionImageLightbox } from "./SessionImageLightbox";

/** 离视口多近开始取图（规则 13：进入视口附近才请求） */
const PRELOAD_MARGIN = "200px";

/**
 * 元素是否进入视口附近；进入后保持 true（图片取过就留着）。
 * 没有 IntersectionObserver 的环境直接视为可见。
 */
const useNearViewport = () => {
  const ref = useRef<HTMLSpanElement | null>(null);
  const [near, setNear] = useState(
    () => typeof IntersectionObserver === "undefined",
  );
  useEffect(() => {
    if (near) return;
    const element = ref.current;
    if (!element) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) {
          setNear(true);
          observer.disconnect();
        }
      },
      { rootMargin: PRELOAD_MARGIN },
    );
    observer.observe(element);
    return () => observer.disconnect();
  }, [near]);
  return [ref, near] as const;
};

export interface SessionImageProps {
  image: ImageRef;
  alt: string;
  /** thumb：提问贴图缩略图（高 96）；result：工具结果截图（宽随容器、最高 320） */
  variant?: "thumb" | "result";
  className?: string;
}

/** 会话图片：懒加载缩略图，点击开灯箱；失败时本地文件显示路径 chip */
export const SessionImage = memo(function SessionImage({
  image,
  alt,
  variant = "thumb",
  className,
}: SessionImageProps) {
  const rt = useReaderT();
  const { providerId, sourcePath, projectDir } = useReaderContext();
  const [ref, near] = useNearViewport();
  const [open, setOpen] = useState(false);
  const { url, error } = useSessionImage(providerId, sourcePath, image, {
    enabled: near,
  });

  const sizeClass =
    variant === "thumb" ? "h-24 max-w-[240px]" : "max-h-[320px] max-w-full";

  if (error) {
    if (image.source.kind === "local_file") {
      return (
        <PathChip
          path={image.source.path}
          projectDir={projectDir}
          className={className}
        />
      );
    }
    return (
      <span
        role="img"
        aria-label={`${alt} · ${rt("image.failed")}`}
        className={cn(
          "inline-flex h-24 w-32 flex-col items-center justify-center gap-1 rounded-[8px] border border-border bg-subtle text-caption text-fg-3",
          className,
        )}
      >
        <ImageOff aria-hidden className="h-4 w-4" />
        {rt("image.failed")}
      </span>
    );
  }

  if (!url) {
    return (
      <span
        ref={ref}
        role="img"
        aria-label={alt}
        aria-busy
        className={cn(
          "inline-block rounded-[8px] border border-border bg-subtle motion-safe:animate-pulse",
          variant === "thumb" ? "h-24 w-32" : "h-40 w-full max-w-[420px]",
          className,
        )}
      />
    );
  }

  return (
    <>
      <button
        type="button"
        aria-label={`${rt("image.open")}：${alt}`}
        onClick={() => setOpen(true)}
        className={cn(
          "inline-flex shrink-0 overflow-hidden rounded-[8px] border border-border bg-subtle transition-[border-color] hover:border-border-strong focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
          className,
        )}
      >
        <img
          src={url}
          alt={alt}
          loading="lazy"
          decoding="async"
          className={cn("block w-auto object-cover", sizeClass)}
        />
      </button>
      {open && (
        <SessionImageLightbox
          open={open}
          onOpenChange={setOpen}
          url={url}
          alt={alt}
          mediaType={image.mediaType}
        />
      )}
    </>
  );
});

/** 一组缩略图（提问贴图、结果截图共用） */
export const SessionImageGrid = memo(function SessionImageGrid({
  images,
  variant = "thumb",
  className,
}: {
  images: ImageRef[];
  variant?: "thumb" | "result";
  className?: string;
}) {
  const rt = useReaderT();
  if (images.length === 0) return null;
  return (
    <div className={cn("flex flex-wrap gap-2", className)}>
      {images.map((image, index) => (
        <SessionImage
          key={index}
          image={image}
          variant={variant}
          alt={image.alt || rt("image.alt", { index: index + 1 })}
        />
      ))}
    </div>
  );
});
