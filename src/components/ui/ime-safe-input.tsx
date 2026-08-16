import * as React from "react";
import { Input, type InputProps } from "@/components/ui/input";

export interface ImeSafeInputProps
  extends Omit<InputProps, "defaultValue" | "onChange" | "value"> {
  value: string;
  onValueChange: (value: string) => void;
  normalize?: (value: string) => string;
}

/**
 * Keeps IME marked text local until composition finishes so parent renders
 * cannot overwrite the browser-managed composition range mid-input.
 */
const ImeSafeInput = React.forwardRef<HTMLInputElement, ImeSafeInputProps>(
  (
    {
      value,
      onValueChange,
      normalize,
      onCompositionStart,
      onCompositionEnd,
      ...props
    },
    ref,
  ) => {
    const composingRef = React.useRef(false);
    const externalValueRef = React.useRef(value);
    const [draft, setDraft] = React.useState(value);

    React.useEffect(() => {
      externalValueRef.current = value;
      if (!composingRef.current) {
        setDraft(value);
      }
    }, [value]);

    const commit = React.useCallback(
      (rawValue: string) => {
        const nextValue = normalize ? normalize(rawValue) : rawValue;
        setDraft(nextValue);

        // Optimistically track the committed value. This also suppresses the
        // duplicate input event that some engines emit after compositionend.
        if (nextValue !== externalValueRef.current) {
          externalValueRef.current = nextValue;
          onValueChange(nextValue);
        }
      },
      [normalize, onValueChange],
    );

    return (
      <Input
        {...props}
        ref={ref}
        value={draft}
        onChange={(event) => {
          const nextValue = event.currentTarget.value;
          if (composingRef.current) {
            setDraft(nextValue);
            return;
          }
          commit(nextValue);
        }}
        onCompositionStart={(event) => {
          composingRef.current = true;
          setDraft(event.currentTarget.value);
          onCompositionStart?.(event);
        }}
        onCompositionEnd={(event) => {
          const finalValue = event.currentTarget.value;
          composingRef.current = false;
          commit(finalValue);
          onCompositionEnd?.(event);
        }}
      />
    );
  },
);

ImeSafeInput.displayName = "ImeSafeInput";

export { ImeSafeInput };
