import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { SuccessSpeedCells } from "@/components/usage/statsColumns";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

afterEach(cleanup);

describe("Hermes compatibility with v4 usage statistics", () => {
  it("does not present an unavailable aggregate success rate as zero percent", () => {
    const stat = { successRate: 0, statusAvailable: false };
    render(
      <table>
        <tbody>
          <tr>
            <SuccessSpeedCells stat={stat} />
          </tr>
        </tbody>
      </table>,
    );
    expect(screen.queryByText("0.0%")).not.toBeInTheDocument();
    expect(screen.getAllByRole("cell").map((cell) => cell.textContent)).toEqual(
      ["—", "—"],
    );
  });

  it("keeps the upstream exact speed and success rate for measured requests", () => {
    render(
      <table>
        <tbody>
          <tr>
            <SuccessSpeedCells
              stat={{
                successRate: 100,
                speedOutputTokens: 200,
                speedGenerationMs: 2000,
              }}
            />
          </tr>
        </tbody>
      </table>,
    );
    expect(screen.getByText("100%")).toBeInTheDocument();
    expect(screen.getAllByRole("cell")[1]).toHaveTextContent("100tok/s");
  });
});
