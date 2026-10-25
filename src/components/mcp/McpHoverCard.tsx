import React, { useState, useRef, useEffect, useId } from "react";
import { useTranslation } from "react-i18next";
import * as PopoverPrimitive from "@radix-ui/react-popover";
import { Globe, BookOpen, ExternalLink } from "lucide-react";
import type { McpServer } from "@/types";
import { type McpAppId, MCP_APP_IDS } from "@/config/appConfig";
import { APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import { mcpPresets } from "@/config/mcpPresets";
import { NeutralBadge } from "./AppMatrix";
import { transportOf } from "./mcpDraft";
import { cn } from "@/lib/utils";

export interface McpHoverCardProps {
  id: string;
  server: McpServer;
  appIds: readonly McpAppId[];
  onOpenDocs?: () => void;
  onOpenHomepage?: (url: string) => void;
  children: React.ReactNode;
}

const OPEN_DELAY = 300;
const CLOSE_DELAY = 200;

export function McpHoverCard({
  id,
  server,
  appIds,
  onOpenDocs,
  onOpenHomepage,
  children,
}: McpHoverCardProps) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const timer = useRef<number | undefined>(undefined);
  const cardId = useId();

  const clear = () => {
    if (timer.current !== undefined) {
      window.clearTimeout(timer.current);
      timer.current = undefined;
    }
  };

  useEffect(() => clear, []);

  const schedule = (next: boolean, delay: number) => {
    clear();
    timer.current = window.setTimeout(() => setOpen(next), delay);
  };

  const handlePointerEnter = () => schedule(true, OPEN_DELAY);
  const handlePointerLeave = () => schedule(false, CLOSE_DELAY);

  const displayName = server.name?.trim() || id;
  const hasCustomName = Boolean(
    server.name?.trim() && server.name.trim() !== id,
  );
  const transport = transportOf(server.server);
  const enabled = MCP_APP_IDS.some((app) => server.apps[app]);
  const enabledCount = appIds.filter((app) => server.apps[app]).length;

  const preset = mcpPresets.find((item) => item.id === id);
  const docsUrl = server.docs || preset?.docs;
  const homepageUrl = server.homepage || preset?.homepage;
  const tags = Array.isArray(server.tags) ? server.tags : [];

  const envKeys =
    server.server?.env && typeof server.server.env === "object"
      ? Object.keys(server.server.env)
      : [];
  const headerKeys =
    server.server?.headers && typeof server.server.headers === "object"
      ? Object.keys(server.server.headers)
      : [];

  const command = server.server?.command || "";
  const args = Array.isArray(server.server?.args) ? server.server.args : [];
  const cmdStr = [command, ...args].filter(Boolean).join(" ");
  const url = server.server?.url || "";

  return (
    <PopoverPrimitive.Root
      open={open}
      onOpenChange={(next) => {
        if (!next) {
          clear();
          setOpen(false);
        }
      }}
    >
      <PopoverPrimitive.Anchor asChild>
        <div
          data-testid={`mcp-hover-trigger-${id}`}
          className="flex min-w-0 flex-1 flex-col pe-3"
          onPointerEnter={handlePointerEnter}
          onPointerLeave={handlePointerLeave}
          onClick={() => {
            clear();
            setOpen(false);
          }}
        >
          {children}
        </div>
      </PopoverPrimitive.Anchor>

      <PopoverPrimitive.Portal>
        <PopoverPrimitive.Content
          id={cardId}
          data-testid={`mcp-hover-card-${id}`}
          side="bottom"
          align="start"
          sideOffset={6}
          collisionPadding={8}
          onOpenAutoFocus={(event) => event.preventDefault()}
          onCloseAutoFocus={(event) => event.preventDefault()}
          onPointerEnter={() => clear()}
          onPointerLeave={handlePointerLeave}
          onClick={(event) => event.stopPropagation()}
          className="z-[110] flex w-[360px] max-w-[calc(100vw-32px)] flex-col gap-3 rounded-[10px] border border-border bg-surface p-3.5 text-fg-1 shadow-v7-md outline-none data-[state=open]:animate-in data-[state=open]:fade-in-0 data-[state=closed]:animate-out data-[state=closed]:fade-out-0"
        >
          {/* Header */}
          <div className="flex flex-col gap-1">
            <div className="flex flex-wrap items-center gap-1.5">
              <span
                className="max-w-[200px] truncate text-body font-semibold text-fg-1"
                title={displayName}
              >
                {displayName}
              </span>
              <NeutralBadge mono>{transport}</NeutralBadge>
              {enabled ? (
                <span className="inline-flex h-[18px] items-center gap-1 rounded-full border border-success/30 bg-success/10 px-1.5 text-badge font-medium text-success-text">
                  <span className="h-1.5 w-1.5 rounded-full bg-success" />
                  {t("mcpPage.enabled", { defaultValue: "On" })}
                </span>
              ) : (
                <NeutralBadge>{t("mcpPage.notEnabled")}</NeutralBadge>
              )}
            </div>
            {hasCustomName && (
              <span className="truncate font-mono text-caption text-fg-3">
                id: {id}
              </span>
            )}
          </div>

          {/* Description */}
          {server.description?.trim() && (
            <p className="text-caption leading-relaxed text-fg-2 break-words">
              {server.description.trim()}
            </p>
          )}

          {/* Tags */}
          {tags.length > 0 && (
            <div className="flex flex-wrap items-center gap-1">
              {tags.map((tag) => (
                <span
                  key={tag}
                  className="inline-flex items-center rounded border border-border bg-subtle px-1.5 py-0.5 text-[11px] font-medium text-fg-2"
                >
                  #{tag}
                </span>
              ))}
            </div>
          )}

          {/* Command or URL Summary */}
          {transport === "stdio" ? (
            <div className="flex flex-col gap-1.5 rounded-[6px] border border-border bg-subtle p-2 text-[11px]">
              <div className="flex items-start gap-1 font-mono text-fg-1 break-all select-text">
                <span className="shrink-0 select-none text-fg-3">$</span>
                <span>{cmdStr || "—"}</span>
              </div>
              {server.server?.cwd && (
                <div className="font-mono text-[10px] text-fg-3 break-all">
                  cwd: {server.server.cwd}
                </div>
              )}
              {envKeys.length > 0 && (
                <div className="mt-1 flex flex-col gap-1 border-t border-border/50 pt-1.5">
                  <span className="text-[10px] font-medium uppercase tracking-wider text-fg-3">
                    {t("mcpPage.envVars", {
                      defaultValue: "Environment Variables",
                    })}{" "}
                    ({envKeys.length})
                  </span>
                  <div className="flex flex-wrap gap-1">
                    {envKeys.map((key) => (
                      <span
                        key={key}
                        className="rounded border border-border bg-surface px-1 py-0.5 font-mono text-[10px] text-fg-2"
                      >
                        {key}=••••••••
                      </span>
                    ))}
                  </div>
                </div>
              )}
            </div>
          ) : (
            <div className="flex flex-col gap-1.5 rounded-[6px] border border-border bg-subtle p-2 text-[11px]">
              <div className="flex items-start gap-1.5 font-mono text-fg-1 break-all select-text">
                <span className="select-none font-sans text-[10px] font-medium uppercase text-fg-3">
                  URL
                </span>
                <span>{url || "—"}</span>
              </div>
              {headerKeys.length > 0 && (
                <div className="mt-1 flex flex-col gap-1 border-t border-border/50 pt-1.5">
                  <span className="text-[10px] font-medium uppercase tracking-wider text-fg-3">
                    {t("mcpPage.headers", { defaultValue: "Headers" })} (
                    {headerKeys.length})
                  </span>
                  <div className="flex flex-wrap gap-1">
                    {headerKeys.map((key) => (
                      <span
                        key={key}
                        className="rounded border border-border bg-surface px-1 py-0.5 font-mono text-[10px] text-fg-2"
                      >
                        {key}: ••••••••
                      </span>
                    ))}
                  </div>
                </div>
              )}
            </div>
          )}

          {/* Enabled Clients */}
          <div className="flex flex-col gap-1.5">
            <div className="flex items-center justify-between text-[11px] text-fg-3">
              <span className="font-medium">
                {t("mcpPage.enabledClients", {
                  defaultValue: "Enabled Clients",
                })}
              </span>
              <span className="font-mono">
                {enabledCount}/{appIds.length}
              </span>
            </div>
            <div className="flex flex-wrap gap-1">
              {appIds.map((app) => {
                const on = Boolean(server.apps[app]);
                return (
                  <span
                    key={app}
                    className={cn(
                      "inline-flex items-center gap-1 rounded-full border px-2 py-0.5 text-[11px] font-medium transition-colors",
                      on
                        ? "border-primary/30 bg-primary/10 text-primary-text"
                        : "border-border/60 bg-subtle/50 text-fg-3 opacity-60",
                    )}
                  >
                    <span
                      className={cn(
                        "h-1.5 w-1.5 rounded-full",
                        on ? "bg-success" : "bg-fg-3/40",
                      )}
                    />
                    {APP_DISPLAY_NAME[app]}
                  </span>
                );
              })}
            </div>
          </div>

          {/* Links Footer */}
          {(docsUrl || homepageUrl) && (
            <div className="flex items-center justify-between border-t border-border pt-2 text-caption">
              <div className="flex items-center gap-3">
                {homepageUrl && (
                  <button
                    type="button"
                    onClick={(event) => {
                      event.stopPropagation();
                      onOpenHomepage?.(homepageUrl);
                    }}
                    className="inline-flex items-center gap-1 text-[12px] font-medium text-primary-text hover:underline focus:outline-none"
                  >
                    <Globe className="h-3.5 w-3.5" />
                    <span>{t("mcpPage.drawer.homepage")}</span>
                    <ExternalLink className="h-2.5 w-2.5 opacity-60" />
                  </button>
                )}
                {docsUrl && docsUrl !== homepageUrl && (
                  <button
                    type="button"
                    onClick={(event) => {
                      event.stopPropagation();
                      onOpenDocs?.();
                    }}
                    className="inline-flex items-center gap-1 text-[12px] font-medium text-primary-text hover:underline focus:outline-none"
                  >
                    <BookOpen className="h-3.5 w-3.5" />
                    <span>{t("mcpPage.openDocs")}</span>
                    <ExternalLink className="h-2.5 w-2.5 opacity-60" />
                  </button>
                )}
              </div>
            </div>
          )}
        </PopoverPrimitive.Content>
      </PopoverPrimitive.Portal>
    </PopoverPrimitive.Root>
  );
}
