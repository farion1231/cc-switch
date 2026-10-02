import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  CalendarDays,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { HoverTip } from "@/components/ui/hover-tip";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { cn } from "@/lib/utils";
import { getUsageRangePresetLabel, resolveUsageRange } from "@/lib/usageRange";
import { getLocaleFromLanguage } from "./format";
import type { UsageRangePreset, UsageRangeSelection } from "@/types/usage";

type DraftField = "start" | "end";

const PRESETS: UsageRangePreset[] = ["today", "1d", "7d", "14d", "30d", "all"];

interface UsageDateRangePickerProps {
  selection: UsageRangeSelection;
  onApply: (selection: UsageRangeSelection) => void;
  triggerLabel: string;
}

/* ── helpers ── */

function startOfDay(d: Date): Date {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate());
}

function isSameDay(a: Date, b: Date): boolean {
  return (
    a.getFullYear() === b.getFullYear() &&
    a.getMonth() === b.getMonth() &&
    a.getDate() === b.getDate()
  );
}

function toTs(d: Date): number {
  return Math.floor(d.getTime() / 1000);
}

function fromTs(ts: number): Date {
  return new Date(ts * 1000);
}

function fmtDate(ts: number): string {
  const d = fromTs(ts);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

function fmtTime(ts: number): string {
  const d = fromTs(ts);
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

function parseDateInput(ts: number, value: string): number {
  const [y, m, d] = value.split("-").map(Number);
  if (!Number.isFinite(y) || !Number.isFinite(m) || !Number.isFinite(d))
    return ts;
  const base = fromTs(ts);
  return toTs(new Date(y, m - 1, d, base.getHours(), base.getMinutes()));
}

function parseTimeInput(ts: number, value: string): number {
  const [h, min] = value.split(":").map(Number);
  if (!Number.isFinite(h) || !Number.isFinite(min)) return ts;
  const base = fromTs(ts);
  return toTs(
    new Date(base.getFullYear(), base.getMonth(), base.getDate(), h, min),
  );
}

function setDateKeepTime(ts: number, day: Date): number {
  const base = fromTs(ts);
  return toTs(
    new Date(
      day.getFullYear(),
      day.getMonth(),
      day.getDate(),
      base.getHours(),
      base.getMinutes(),
    ),
  );
}

function getCalendarDays(month: Date): Date[] {
  const first = new Date(month.getFullYear(), month.getMonth(), 1);
  const gridStart = new Date(first);
  gridStart.setDate(first.getDate() - first.getDay());
  return Array.from({ length: 42 }, (_, i) => {
    const d = new Date(gridStart);
    d.setDate(gridStart.getDate() + i);
    return d;
  });
}

/* ── component ── */

function resolveCalendarRange(selection: UsageRangeSelection) {
  const r = resolveUsageRange(selection);
  if (selection.preset !== "all") return r;
  return { startDate: r.endDate - 30 * 24 * 60 * 60, endDate: r.endDate };
}

export function UsageDateRangePicker({
  selection,
  onApply,
  triggerLabel,
}: UsageDateRangePickerProps) {
  const { t, i18n } = useTranslation();
  const [open, setOpen] = useState(false);
  const prevMonthLabel = t("usage.prevMonth", { defaultValue: "上个月" });
  const nextMonthLabel = t("usage.nextMonth", { defaultValue: "下个月" });
  const [activeField, setActiveField] = useState<DraftField>("start");
  // 「全部」的起点是 0，日历草稿改从结束日往前 30 天开始，避免跳到 1970 年
  const resolvedRange = useMemo(
    () => resolveCalendarRange(selection),
    [selection],
  );
  const [draftStart, setDraftStart] = useState(resolvedRange.startDate);
  const [draftEnd, setDraftEnd] = useState(resolvedRange.endDate);
  const [draftLiveEnd, setDraftLiveEnd] = useState(
    selection.preset === "custom" ? (selection.liveEndTime ?? false) : false,
  );
  const [displayMonth, setDisplayMonth] = useState(
    () =>
      new Date(
        fromTs(resolvedRange.startDate).getFullYear(),
        fromTs(resolvedRange.startDate).getMonth(),
        1,
      ),
  );
  const [error, setError] = useState<string | null>(null);

  const language = i18n.resolvedLanguage || i18n.language || "en";
  const locale = getLocaleFromLanguage(language);

  // Reset draft when popover opens
  useEffect(() => {
    if (!open) return;
    const r = resolveCalendarRange(selection);
    setDraftStart(r.startDate);
    setDraftEnd(r.endDate);
    setDraftLiveEnd(
      selection.preset === "custom" ? (selection.liveEndTime ?? false) : false,
    );
    setDisplayMonth(
      new Date(
        fromTs(r.startDate).getFullYear(),
        fromTs(r.startDate).getMonth(),
        1,
      ),
    );
    setActiveField("start");
    setError(null);
  }, [open, selection]);

  // Keep draftEnd ticking when live mode is active and popover is open
  useEffect(() => {
    if (!open || !draftLiveEnd) return;
    const tick = () => setDraftEnd(Math.floor(Date.now() / 1000));
    tick();
    const id = setInterval(tick, 1000);
    return () => clearInterval(id);
  }, [open, draftLiveEnd]);

  const calendarDays = useMemo(
    () => getCalendarDays(displayMonth),
    [displayMonth],
  );

  const weekdayLabels = useMemo(
    () =>
      Array.from({ length: 7 }, (_, i) =>
        new Intl.DateTimeFormat(locale, { weekday: "narrow" }).format(
          new Date(2024, 0, 7 + i),
        ),
      ),
    [locale],
  );

  const startDay = fromTs(draftStart);
  const endDay = fromTs(draftEnd);
  const today = new Date();

  /* Pick a date from the calendar */
  const handleDatePick = (day: Date) => {
    setError(null);

    // When live end time is active, calendar only controls start date
    if (draftLiveEnd) {
      const nextTs = setDateKeepTime(draftStart, day);
      setDraftStart(nextTs);
      return;
    }

    const nextTs = setDateKeepTime(
      activeField === "start" ? draftStart : draftEnd,
      day,
    );

    if (activeField === "start") {
      setDraftStart(nextTs);
      // Auto-swap if start > end
      if (nextTs > draftEnd) {
        setDraftEnd(nextTs);
      }
      // Auto-advance to end field
      setActiveField("end");
    } else {
      // If picked end < start, treat as new start and auto-advance
      if (nextTs < draftStart) {
        setDraftStart(nextTs);
        setActiveField("end");
      } else {
        setDraftEnd(nextTs);
      }
    }

    // Navigate calendar if the day is outside the displayed month
    if (
      day.getMonth() !== displayMonth.getMonth() ||
      day.getFullYear() !== displayMonth.getFullYear()
    ) {
      setDisplayMonth(new Date(day.getFullYear(), day.getMonth(), 1));
    }
  };

  const handleApply = () => {
    setError(null);
    if (draftStart > draftEnd) {
      setError(t("usage.invalidTimeRangeOrder", "开始时间不能晚于结束时间"));
      return;
    }
    onApply({
      preset: "custom",
      customStartDate: draftStart,
      customEndDate: draftEnd,
      liveEndTime: draftLiveEnd,
    });
    setOpen(false);
  };

  const goToToday = () => {
    setDisplayMonth(new Date(today.getFullYear(), today.getMonth(), 1));
  };

  /* ── Field card (start / end) ── */
  const renderField = (field: DraftField) => {
    const isActive = activeField === field;
    const isEndLive = field === "end" && draftLiveEnd;
    const ts = field === "start" ? draftStart : draftEnd;
    const setTs = field === "start" ? setDraftStart : setDraftEnd;
    const label =
      field === "start"
        ? t("usage.startTime", "开始时间")
        : t("usage.endTime", "结束时间");

    return (
      <div
        className={cn(
          "rounded-panel border px-3 py-2 transition-all",
          isEndLive
            ? "border-border bg-subtle cursor-not-allowed opacity-50"
            : isActive
              ? "border-border-strong ring-1 ring-ring bg-surface cursor-pointer"
              : "border-border hover:border-border cursor-pointer",
        )}
        onClick={() => {
          if (!isEndLive) setActiveField(field);
        }}
      >
        <div className="mb-1.5 text-badge font-medium uppercase tracking-wider text-fg-2">
          {label}
        </div>
        <div className="flex items-center gap-1.5">
          <Input
            type="date"
            className={cn(
              "h-7 flex-1 border-0 bg-transparent p-0 text-body shadow-none focus-visible:ring-0",
              isEndLive && "pointer-events-none",
            )}
            value={fmtDate(ts)}
            onChange={(e) => {
              if (isEndLive) return;
              const next = parseDateInput(ts, e.target.value);
              setTs(next);
              const d = fromTs(next);
              setDisplayMonth(new Date(d.getFullYear(), d.getMonth(), 1));
              setError(null);
            }}
            onFocus={() => {
              if (!isEndLive) setActiveField(field);
            }}
            readOnly={isEndLive}
          />
          <Input
            type="time"
            step={60}
            className={cn(
              "h-7 w-[90px] flex-none border-0 bg-transparent p-0 text-body shadow-none focus-visible:ring-0",
              isEndLive && "pointer-events-none",
            )}
            value={fmtTime(ts)}
            onChange={(e) => {
              if (isEndLive) return;
              setTs(parseTimeInput(ts, e.target.value));
              setError(null);
            }}
            onFocus={() => {
              if (!isEndLive) setActiveField(field);
            }}
            readOnly={isEndLive}
          />
        </div>
      </div>
    );
  };

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button
          type="button"
          variant="neutral"
          size="regular"
          className="max-w-[220px] shrink-0 gap-1 pe-2 ps-3"
          title={`${t("usage.timeRange")}: ${triggerLabel}`}
        >
          {selection.preset === "custom" && (
            <CalendarDays className="h-3.5 w-3.5 shrink-0 text-fg-2" />
          )}
          <span className="min-w-0 truncate">{triggerLabel}</span>
          <ChevronDown className="h-3.5 w-3.5 shrink-0 text-fg-2" />
        </Button>
      </PopoverTrigger>
      <PopoverContent
        className="usage-range-popover w-[620px] max-w-[calc(100vw-2rem)] rounded-panel border-border bg-surface p-3 shadow-v7-md"
        align="end"
      >
        {/* Preset shortcuts */}
        <div className="flex flex-wrap gap-1.5 pb-2 border-b border-border">
          {PRESETS.map((preset) => (
            <Button
              key={preset}
              type="button"
              size="compact"
              variant="neutral"
              aria-pressed={selection.preset === preset}
              className={cn(
                "px-2.5",
                selection.preset === preset &&
                  "border-fg-1 bg-selected font-semibold",
              )}
              onClick={() => {
                onApply({ preset });
                setOpen(false);
              }}
            >
              {getUsageRangePresetLabel(preset, t)}
            </Button>
          ))}
        </div>

        <div className="usage-range-layout flex flex-col gap-3">
          {/* Left: date fields */}
          <div className="usage-range-fields space-y-2">
            <p className="text-caption text-fg-2">
              {t("usage.customRangeHint", "支持日期与时间，最长 30 天")}
            </p>
            {renderField("start")}
            {renderField("end")}

            <label className="flex items-center gap-2 cursor-pointer select-none">
              <Checkbox
                checked={draftLiveEnd}
                onCheckedChange={(checked) => {
                  const live = checked === true;
                  setDraftLiveEnd(live);
                  if (live) {
                    setDraftEnd(Math.floor(Date.now() / 1000));
                    setActiveField("start");
                  }
                }}
              />
              <span className="text-caption text-fg-2">
                {t("usage.liveEndTime", "结束时间跟随当前时刻")}
              </span>
            </label>

            {error && <p className="text-caption text-danger-text">{error}</p>}

            <div className="flex gap-2 pt-1">
              <Button
                type="button"
                variant="quiet"
                size="compact"
                className="flex-1"
                onClick={() => setOpen(false)}
              >
                {t("common.cancel")}
              </Button>
              <Button
                type="button"
                variant="solid"
                size="compact"
                className="flex-1"
                onClick={handleApply}
              >
                {t("common.confirm")}
              </Button>
            </div>
          </div>

          {/* Right: calendar */}
          <div className="usage-range-calendar rounded-panel border border-border bg-subtle p-2.5">
            {/* Month navigation */}
            <div className="flex items-center justify-between mb-1.5">
              <HoverTip content={prevMonthLabel}>
                <Button
                  type="button"
                  size="icon-compact"
                  variant="quiet"
                  aria-label={prevMonthLabel}
                  onClick={() =>
                    setDisplayMonth(
                      new Date(
                        displayMonth.getFullYear(),
                        displayMonth.getMonth() - 1,
                        1,
                      ),
                    )
                  }
                >
                  <ChevronLeft className="h-3.5 w-3.5" />
                </Button>
              </HoverTip>
              {/* 点月份标题跳回本月：动作不在字面上，用 HoverTip 说明 */}
              <HoverTip
                content={t("usage.presetToday", { defaultValue: "当天" })}
              >
                <button
                  type="button"
                  className="text-body font-medium text-fg-1 transition-colors hover:text-fg-2"
                  onClick={goToToday}
                >
                  {displayMonth.toLocaleDateString(locale, {
                    year: "numeric",
                    month: "long",
                  })}
                </button>
              </HoverTip>
              <HoverTip content={nextMonthLabel}>
                <Button
                  type="button"
                  size="icon-compact"
                  variant="quiet"
                  aria-label={nextMonthLabel}
                  onClick={() =>
                    setDisplayMonth(
                      new Date(
                        displayMonth.getFullYear(),
                        displayMonth.getMonth() + 1,
                        1,
                      ),
                    )
                  }
                >
                  <ChevronRight className="h-3.5 w-3.5" />
                </Button>
              </HoverTip>
            </div>

            {/* Weekday headers */}
            <div className="grid grid-cols-7 text-center text-badge text-fg-2 mb-0.5">
              {weekdayLabels.map((label, i) => (
                <div key={i} className="py-0.5">
                  {label}
                </div>
              ))}
            </div>

            {/* Day grid */}
            <div className="grid grid-cols-7 gap-px">
              {calendarDays.map((day) => {
                const isCurrentMonth =
                  day.getMonth() === displayMonth.getMonth();
                const isToday = isSameDay(day, today);
                const isStart = isSameDay(day, startDay);
                const isEnd = isSameDay(day, endDay);
                const dayStart = startOfDay(day);
                const inRange =
                  dayStart >= startOfDay(startDay) &&
                  dayStart <= startOfDay(endDay);
                const isEndpoint = isStart || isEnd;

                return (
                  <button
                    key={day.toISOString()}
                    type="button"
                    aria-label={day.toLocaleDateString(locale)}
                    aria-current={isToday ? "date" : undefined}
                    aria-pressed={isEndpoint}
                    className={cn(
                      "relative h-7 rounded text-caption transition-colors",
                      !isCurrentMonth && "text-fg-3",
                      isCurrentMonth && !inRange && "hover:bg-subtle",
                      inRange && !isEndpoint && "bg-selected text-fg-1",
                      isEndpoint && "bg-action text-action-fg font-medium",
                      isToday && !isEndpoint && "ring-1 ring-border-strong",
                    )}
                    onClick={() => handleDatePick(day)}
                  >
                    {day.getDate()}
                  </button>
                );
              })}
            </div>
          </div>
        </div>
      </PopoverContent>
    </Popover>
  );
}
