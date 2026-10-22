import React, { useState, useRef, useId, useCallback, useEffect } from "react";
import { X, Tag as TagIcon } from "lucide-react";
import {
  DndContext,
  closestCenter,
  KeyboardSensor,
  PointerSensor,
  useSensor,
  useSensors,
  type DragEndEvent,
} from "@dnd-kit/core";
import {
  SortableContext,
  horizontalListSortingStrategy,
  useSortable,
  arrayMove,
  sortableKeyboardCoordinates,
} from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { cn } from "@/lib/utils";

export interface TagInputProps {
  id?: string;
  value?: string[];
  onChange?: (tags: string[]) => void;
  suggestions?: string[];
  placeholder?: string;
  disabled?: boolean;
  className?: string;
  maxTags?: number;
}

interface SortableTagChipProps {
  tag: string;
  disabled?: boolean;
  onRemove: (tag: string) => void;
}

function SortableTagChip({ tag, disabled, onRemove }: SortableTagChipProps) {
  const {
    attributes,
    listeners,
    setNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({
    id: tag,
    disabled,
  });

  const style: React.CSSProperties = {
    transform: CSS.Translate.toString(transform),
    transition,
  };

  return (
    <span
      ref={setNodeRef}
      style={style}
      {...attributes}
      {...listeners}
      className={cn(
        "inline-flex items-center gap-1 rounded-[5px] border border-border bg-subtle px-2 py-0.5 text-caption font-medium text-fg-1 select-none transition-colors",
        !disabled && "cursor-grab active:cursor-grabbing hover:bg-selected",
        isDragging && "z-10 opacity-60 ring-2 ring-ring shadow-sm",
      )}
    >
      <TagIcon className="h-2.5 w-2.5 text-fg-3 shrink-0" aria-hidden="true" />
      <span className="max-w-[150px] truncate">{tag}</span>
      {!disabled && (
        <button
          type="button"
          tabIndex={-1}
          onClick={(event) => {
            event.stopPropagation();
            onRemove(tag);
          }}
          className="-me-1 ms-0.5 rounded-sm p-0.5 text-fg-3 transition-colors hover:bg-border/60 hover:text-fg-1 focus:outline-none"
          aria-label={`Remove tag ${tag}`}
        >
          <X className="h-3 w-3" strokeWidth={2} />
        </button>
      )}
    </span>
  );
}

export function TagInput({
  id,
  value = [],
  onChange,
  suggestions = [],
  placeholder = "Add a tag...",
  disabled = false,
  className,
  maxTags,
}: TagInputProps) {
  const [inputValue, setInputValue] = useState("");
  const [isOpen, setIsOpen] = useState(false);
  const [highlightedIndex, setHighlightedIndex] = useState(-1);
  const containerRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const dropdownId = useId();

  const sensors = useSensors(
    useSensor(PointerSensor, {
      activationConstraint: { distance: 5 },
    }),
    useSensor(KeyboardSensor, {
      coordinateGetter: sortableKeyboardCoordinates,
    }),
  );

  const trimmedInput = inputValue.trim();

  // 过滤建议列表：匹配当前输入（若输入为空且聚焦时展示全部候选项），且排除已选择项
  const matchingSuggestions = suggestions.filter((s) => {
    if (value.includes(s)) return false;
    if (!trimmedInput) return true;
    return s.toLowerCase().includes(trimmedInput.toLowerCase());
  });

  const addTag = useCallback(
    (tagToAdd: string) => {
      const clean = tagToAdd.trim();
      if (!clean) return;
      if (maxTags !== undefined && value.length >= maxTags) return;
      if (value.some((t) => t.toLowerCase() === clean.toLowerCase())) {
        setInputValue("");
        return;
      }
      onChange?.([...value, clean]);
      setInputValue("");
      setIsOpen(false);
      setHighlightedIndex(-1);
    },
    [maxTags, onChange, value],
  );

  const removeTag = useCallback(
    (tagToRemove: string) => {
      onChange?.(value.filter((t) => t !== tagToRemove));
    },
    [onChange, value],
  );

  const handleDragEnd = useCallback(
    (event: DragEndEvent) => {
      const { active, over } = event;
      if (over && active.id !== over.id) {
        const oldIndex = value.indexOf(String(active.id));
        const newIndex = value.indexOf(String(over.id));
        if (oldIndex !== -1 && newIndex !== -1) {
          onChange?.(arrayMove(value, oldIndex, newIndex));
        }
      }
    },
    [onChange, value],
  );

  const handleKeyDown = (event: React.KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Enter") {
      event.preventDefault();
      if (
        isOpen &&
        highlightedIndex >= 0 &&
        highlightedIndex < matchingSuggestions.length
      ) {
        addTag(matchingSuggestions[highlightedIndex]);
      } else if (trimmedInput) {
        addTag(trimmedInput);
      }
      return;
    }

    if (event.key === "," || event.key === "，") {
      event.preventDefault();
      if (trimmedInput) {
        addTag(trimmedInput);
      }
      return;
    }

    if (event.key === "Backspace" && !inputValue && value.length > 0) {
      event.preventDefault();
      removeTag(value[value.length - 1]);
      return;
    }

    if (event.key === "ArrowDown") {
      if (!isOpen && matchingSuggestions.length > 0) {
        setIsOpen(true);
        setHighlightedIndex(0);
        event.preventDefault();
        return;
      }
      if (matchingSuggestions.length > 0) {
        event.preventDefault();
        setHighlightedIndex((prev) =>
          prev + 1 >= matchingSuggestions.length ? 0 : prev + 1,
        );
      }
      return;
    }

    if (event.key === "ArrowUp") {
      if (isOpen && matchingSuggestions.length > 0) {
        event.preventDefault();
        setHighlightedIndex((prev) =>
          prev <= 0 ? matchingSuggestions.length - 1 : prev - 1,
        );
      }
      return;
    }

    if (event.key === "Escape") {
      setIsOpen(false);
      setHighlightedIndex(-1);
      return;
    }

    if (event.key === "Tab" && isOpen && highlightedIndex >= 0) {
      event.preventDefault();
      addTag(matchingSuggestions[highlightedIndex]);
    }
  };

  // 点击外部关闭下拉建议
  useEffect(() => {
    const handleOutsideClick = (event: MouseEvent) => {
      if (
        containerRef.current &&
        !containerRef.current.contains(event.target as Node)
      ) {
        setIsOpen(false);
      }
    };
    document.addEventListener("mousedown", handleOutsideClick);
    return () => document.removeEventListener("mousedown", handleOutsideClick);
  }, []);

  return (
    <div ref={containerRef} className="relative w-full">
      <div
        className={cn(
          "flex min-h-8 w-full flex-wrap items-center gap-1.5 rounded-control border border-border-strong bg-surface px-2 py-1 text-body text-fg-1 transition-[border-color,box-shadow]",
          "focus-within:border-ring focus-within:ring-[3px] focus-within:ring-ring/20 dark:focus-within:ring-ring/15",
          disabled && "cursor-not-allowed opacity-50",
          className,
        )}
        onClick={() => inputRef.current?.focus()}
      >
        <DndContext
          sensors={sensors}
          collisionDetection={closestCenter}
          onDragEnd={handleDragEnd}
        >
          <SortableContext
            items={value}
            strategy={horizontalListSortingStrategy}
          >
            {value.map((tag) => (
              <SortableTagChip
                key={tag}
                tag={tag}
                disabled={disabled}
                onRemove={removeTag}
              />
            ))}
          </SortableContext>
        </DndContext>

        <input
          id={id}
          ref={inputRef}
          type="text"
          value={inputValue}
          disabled={
            disabled || (maxTags !== undefined && value.length >= maxTags)
          }
          placeholder={value.length === 0 ? placeholder : ""}
          autoComplete="off"
          spellCheck={false}
          aria-autocomplete="list"
          aria-expanded={isOpen && matchingSuggestions.length > 0}
          aria-controls={dropdownId}
          onFocus={() => {
            if (matchingSuggestions.length > 0) setIsOpen(true);
          }}
          onChange={(event) => {
            setInputValue(event.target.value);
            setIsOpen(true);
            setHighlightedIndex(-1);
          }}
          onKeyDown={handleKeyDown}
          className="min-w-[80px] flex-1 bg-transparent p-0 text-caption text-fg-1 placeholder:text-fg-3 focus:outline-none disabled:cursor-not-allowed"
        />
      </div>

      {isOpen && matchingSuggestions.length > 0 && !disabled && (
        <ul
          id={dropdownId}
          role="listbox"
          className="absolute left-0 top-[calc(100%+4px)] z-50 max-h-44 w-full overflow-y-auto rounded-control border border-border bg-surface p-1 shadow-v7-md animate-in fade-in-0 zoom-in-95"
        >
          {matchingSuggestions.map((suggestion, index) => {
            const isSelected = index === highlightedIndex;
            return (
              <li
                key={suggestion}
                role="option"
                aria-selected={isSelected}
                onMouseDown={(event) => {
                  event.preventDefault(); // 阻止失焦
                  addTag(suggestion);
                }}
                onMouseEnter={() => setHighlightedIndex(index)}
                className={cn(
                  "flex cursor-pointer items-center justify-between rounded-sm px-2.5 py-1 text-caption text-fg-1 transition-colors select-none",
                  isSelected ? "bg-selected font-medium" : "hover:bg-subtle",
                )}
              >
                <span>{suggestion}</span>
                <span className="text-[11px] text-fg-3">#tag</span>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
