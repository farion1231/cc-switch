import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { useState } from "react";
import type { FailoverRetryPolicy } from "@/types";
import { ProviderFailoverRetrySection } from "@/components/providers/forms/ProviderFailoverRetrySection";

function TestHarness() {
  const [value, setValue] = useState<FailoverRetryPolicy>({
    mode: "finite",
    maxRetries: 0,
    baseDelaySeconds: 3,
    maxDelaySeconds: 30,
    backoffMultiplier: 2,
  });

  return (
    <div>
      <ProviderFailoverRetrySection value={value} onChange={setValue} />
      <pre data-testid="policy">{JSON.stringify(value)}</pre>
    </div>
  );
}

describe("ProviderFailoverRetrySection", () => {
  it("supports add, edit, and remove for non-retryable keywords", () => {
    render(<TestHarness />);

    expect(
      screen.getByText(
        /Matches error message, type, code, and status after case-insensitive normalization/,
      ),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Add keyword" }));

    const keyword4 = screen.getByLabelText("Non-retryable keyword 4");
    fireEvent.change(keyword4, { target: { value: "rate_limit_exceeded" } });

    expect(screen.getByTestId("policy")).toHaveTextContent(
      "rate_limit_exceeded",
    );

    fireEvent.click(screen.getByRole("button", { name: "Remove keyword 2" }));

    expect(screen.getByTestId("policy")).not.toHaveTextContent(
      "invalid_request",
    );
  });
});
