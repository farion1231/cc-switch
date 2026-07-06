import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { AppSwitcher } from "@/components/AppSwitcher";
import { PiProviderDiffPreview } from "@/components/pi/PiProviderDiffPreview";

vi.mock("@/components/ProviderIcon", () => ({
	ProviderIcon: ({ name }: { name: string }) => <span>{name}</span>,
}));

describe("Pi Agent app entry", () => {
	it("renders Pi Agent as an app option", () => {
		render(<AppSwitcher activeApp="claude" onSwitch={vi.fn()} />);

		expect(screen.getAllByText("Pi Agent").length).toBeGreaterThan(0);
	});

	it("renders delete action when reviewing an existing provider", () => {
		const onDelete = vi.fn();
		render(
			<PiProviderDiffPreview
				preview={{
					currentFileHash: "hash-1",
					nextModelsJson: { providers: { keep: {} } },
					summary: ["Upsert Pi provider keep"],
				}}
				onApply={vi.fn()}
				onDelete={onDelete}
				canDelete
			/>,
		);

		fireEvent.click(screen.getByText("pi.review.delete"));

		expect(onDelete).toHaveBeenCalled();
	});
});
