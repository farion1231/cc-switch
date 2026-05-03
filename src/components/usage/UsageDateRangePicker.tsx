import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { CalendarDays, ChevronLeft, ChevronRight } from "lucide-react";
import { Button } from "@/components/ui/button";
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

const PRESETS: UsageRangePreset[] = ["today", "1d", "7d", "14d", "30d"];

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

export function UsageDateRangePicker({
  selection,
  onApply,
  triggerLabel,
}: UsageDateRangePickerProps) {
  const { t, i18n } = useTranslation();
  const [open, setOpen] = useState(false);
  const [activeField, setActiveField] = useState<DraftField>("start");
  const resolvedRange = useMemo(
    () => resolveUsageRange(selection),
    [selection],
  );
  const [draftStart, setDraftStart] = useState(resolvedRange.startDate);
  const [draftEnd, setDraftEnd] = useState(resolvedRange.endDate);
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
    const r = resolveUsageRange(selection);
    setDraftStart(r.startDate);
    setDraftEnd(r.endDate);
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
    });
    setOpen(false);
  };

  const goToToday = () => {
    setDisplayMonth(new Date(today.getFullYear(), today.getMonth(), 1));
  };

  /* ── Field card (start / end) ── */
  const renderField = (field: DraftField) => {
    const isActive = activeField === field;
    const ts = field === "start" ? draftStart : draftEnd;
    const setTs = field === "start" ? setDraftStart : setDraftEnd;
    const label =
      field === "start"
        ? t("usage.startTime", "开始时间")
        : t("usage.endTime", "结束时间");

    return (
      <div
        className={cn(
          "rounded-lg border px-3 py-2 cursor-pointer transition-all",
          isActive
            ? "border-primary ring-1 ring-primary/30 bg-primary/5"
            : "border-border/50 hover:border-border",
        )}
        onClick={() => setActiveField(field)}
      >
        <div className="mb-1.5 text-[11px] font-medium uppercase tracking-wider text-muted-foreground">
          {label}
        </div>
        <div className="flex items-center gap-1.5">
          <Input
            type="date"
            className="h-7 flex-1 border-0 bg-transparent p-0 text-sm shadow-none focus-visible:ring-0"
            value={fmtDate(ts)}
            onChange={(e) => {
              const next = parseDateInput(ts, e.target.value);
              setTs(next);
              const d = fromTs(next);
              setDisplayMonth(new Date(d.getFullYear(), d.getMonth(), 1));
              setError(null);
            }}
            onFocus={() => setActiveField(field)}
          />
          <Input
            type="time"
            step={60}
            className="h-7 w-[90px] flex-none border-0 bg-transparent p-0 text-sm shadow-none focus-visible:ring-0"
            value={fmtTime(ts)}
            onChange={(e) => {
              setTs(parseTimeInput(ts, e.target.value));
              setError(null);
            }}
            onFocus={() => setActiveField(field)}
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
          variant={selection.preset === "custom" ? "default" : "outline"}
          className="justify-start gap-2"
        >
          <CalendarDays className="h-4 w-4" />
          <span className="truncate">{triggerLabel}</span>
        </Button>
      </PopoverTrigger>
      <PopoverContent
        className="w-[340px] max-w-[calc(100vw-2rem)] p-3 sm:w-[620px]"
        align="end"
      >
        {/* Preset shortcuts */}
        <div className="flex flex-wrap gap-1.5 pb-2 border-b border-border/40">
          {PRESETS.map((preset) => (
            <Button
              key={preset}
              type="button"
              size="sm"
              variant={selection.preset === preset ? "default" : "outline"}
              className="h-7 px-2.5 text-xs"
              onClick={() => {
                onApply({ preset });
                setOpen(false);
              }}
            >
              {getUsageRangePresetLabel(preset, t)}
            </Button>
          ))}
        </div>

        <div className="flex flex-col gap-3 sm:flex-row">
          {/* Left: date fields */}
          <div className="space-y-2 sm:w-[250px] sm:flex-none">
            <p className="text-xs text-muted-foreground">
              {t("usage.customRangeHint", "支持日期与时间，最长 30 天")}
            </p>
            {renderField("start")}
            {renderField("end")}

            {error && <p className="text-xs text-destructive">{error}</p>}

            <div className="flex gap-2 pt-1">
              <Button
                type="button"
                variant="ghost"
                size="sm"
                className="flex-1"
                onClick={() => setOpen(false)}
              >
                {t("common.cancel")}
              </Button>
              <Button
                type="button"
                size="sm"
                className="flex-1"
                onClick={handleApply}
              >
                {t("common.confirm")}
              </Button>
            </div>
          </div>

          {/* Right: calendar */}
          <div className="rounded-lg border border-border/50 bg-muted/30 p-2.5 sm:min-w-0 sm:flex-1">
            {/* Month navigation */}
            <div className="flex items-center justify-between mb-1.5">
              <Button
                type="button"
                size="icon"
                variant="ghost"
                className="h-7 w-7"
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
              <button
                type="button"
                className="text-sm font-medium hover:text-primary transition-colors"
                onClick={goToToday}
                title={t("usage.presetToday", { defaultValue: "当天" })}
              >
                {displayMonth.toLocaleDateString(locale, {
                  year: "numeric",
                  month: "long",
                })}
              </button>
              <Button
                type="button"
                size="icon"
                variant="ghost"
                className="h-7 w-7"
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
            </div>

            {/* Weekday headers */}
            <div className="grid grid-cols-7 text-center text-[11px] text-muted-foreground mb-0.5">
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
                      "relative h-7 rounded text-xs transition-colors",
                      !isCurrentMonth && "text-muted-foreground/30",
                      isCurrentMonth && !inRange && "hover:bg-muted",
                      inRange && !isEndpoint && "bg-primary/10 text-primary",
                      isEndpoint &&
                        "bg-primary text-primary-foreground font-medium",
                      isToday && !isEndpoint && "ring-1 ring-primary/40",
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
