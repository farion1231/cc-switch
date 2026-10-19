import { useState } from "react";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { beforeAll, describe, expect, it, vi } from "vitest";

beforeAll(() => {
  Element.prototype.scrollIntoView = vi.fn();
});
import {
  buildModelApiFormats,
  modelApiFormatRows,
  ModelApiFormatsField,
  type ModelApiFormatRow,
} from "@/components/providers/forms/ModelApiFormatsField";

function Editor({
  fullUrl = false,
  client = "codex",
}: {
  fullUrl?: boolean;
  client?: "claude" | "codex";
}) {
  const [rows, setRows] = useState(
    modelApiFormatRows({ "chat-model": "openai_chat" }),
  );
  return (
    <ModelApiFormatsField
      client={client}
      rows={rows}
      onChange={setRows}
      isFullUrl={fullUrl}
      models={[{ id: "catalog-model", ownedBy: null }]}
    />
  );
}

describe("model protocol overrides", () => {
  for (const client of ["claude", "codex"] as const) {
    it(`${client}: labels the native protocol relative to the consumer`, () => {
      render(<Editor client={client} />);
      const messages = screen.getByRole("tab", { name: /Anthropic Messages/ });
      const responses = screen.getByRole("tab", { name: /Responses/ });
      const chat = screen.getByRole("tab", { name: /Chat Completions/ });
      const native = "providerForm.modelApiFormats.native";
      const converted = "providerForm.modelApiFormats.conversionRequired";
      expect(
        within(messages).getByText(client === "claude" ? native : converted),
      ).toBeInTheDocument();
      expect(
        within(responses).getByText(client === "codex" ? native : converted),
      ).toBeInTheDocument();
      expect(within(chat).getByText(converted)).toBeInTheDocument();
      if (client === "claude") {
        expect(chat).toHaveTextContent("OpenAI Chat Completions");
        expect(responses).toHaveTextContent("OpenAI Responses API");
      }
    });
  }
  it.each(["claude", "codex"] as const)(
    "%s shows protocol hints for the downstream client",
    (client) => {
      render(<Editor client={client} />);
      for (const [label, hint] of [
        [/Chat Completions/, "chat"],
        [/Responses/, "responses"],
        [/Anthropic Messages/, "messages"],
      ] as const) {
        fireEvent.mouseDown(screen.getByRole("tab", { name: label }), {
          button: 0,
          ctrlKey: false,
        });
        const key =
          client === "claude"
            ? `claude${hint[0].toUpperCase()}${hint.slice(1)}`
            : hint;
        expect(screen.getByRole("group")).toHaveTextContent(
          `providerForm.protocolSettings.${key}Hint`,
        );
      }
    },
  );

  it("normalizes IDs, ignores empty rows, and detects incomplete/duplicate entries", () => {
    const row = (
      model: string,
      format: ModelApiFormatRow["format"],
    ): ModelApiFormatRow => ({ rowId: 1, model, format });
    expect(buildModelApiFormats([row("", "")])).toEqual({ formats: undefined });
    expect(buildModelApiFormats([row(" model[1M] ", "anthropic")])).toEqual({
      formats: { model: "anthropic" },
    });
    expect(buildModelApiFormats([row("", "anthropic")]).error).toBe(
      "missingModel",
    );
    expect(buildModelApiFormats([row("model", "")]).error).toBe(
      "missingProtocol",
    );
    expect(
      buildModelApiFormats([
        row("model", "anthropic"),
        row("model[1m]", "openai_chat"),
      ]).error,
    ).toBe("duplicate");
    expect(
      buildModelApiFormats(
        modelApiFormatRows({
          __proto__: "anthropic",
          model: "openai_responses",
        }),
      ).formats?.model,
    ).toBe("openai_responses");
  });

  it("loads existing rows, switches protocol tabs without losing edits, adds and removes models", () => {
    render(<Editor />);
    const model = screen.getByRole("textbox", {
      name: "providerForm.modelApiFormats.model 1",
    });
    expect(model).toHaveValue("chat-model");
    fireEvent.change(model, { target: { value: "messages-model" } });
    fireEvent.mouseDown(
      screen.getByRole("tab", { name: /Anthropic Messages · 0/ }),
      {
        button: 0,
        ctrlKey: false,
      },
    );
    expect(screen.queryByRole("textbox")).toBeNull();
    fireEvent.mouseDown(
      screen.getByRole("tab", { name: /Chat Completions · 1/ }),
      {
        button: 0,
        ctrlKey: false,
      },
    );
    expect(screen.getByRole("textbox")).toHaveValue("messages-model");
    fireEvent.click(
      screen.getByRole("button", { name: "providerForm.modelApiFormats.add" }),
    );
    expect(screen.getAllByRole("textbox")).toHaveLength(2);
    fireEvent.click(
      screen.getByRole("button", {
        name: "providerForm.modelApiFormats.remove 1",
      }),
    );
    expect(screen.getAllByRole("textbox")).toHaveLength(1);
  });

  it("selects an existing catalog model", () => {
    render(<Editor />);
    fireEvent.click(screen.getByRole("button", { name: "Select model" }));
    fireEvent.click(screen.getByText("catalog-model"));
    expect(screen.getByRole("textbox")).toHaveValue("catalog-model");
  });

  it("adds a model to the selected protocol and validates duplicates across tabs", () => {
    render(<Editor />);
    fireEvent.mouseDown(
      screen.getByRole("tab", { name: /Anthropic Messages · 0/ }),
      {
        button: 0,
        ctrlKey: false,
      },
    );
    fireEvent.click(
      screen.getByRole("button", { name: "providerForm.modelApiFormats.add" }),
    );
    // An untouched row is UI-only and may be left blank on save.
    expect(screen.queryByRole("alert")).toBeNull();
    fireEvent.change(screen.getByRole("textbox"), {
      target: { value: "chat-model[1M]" },
    });
    expect(
      screen.getByRole("tab", { name: /Anthropic Messages · 1/ }),
    ).toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent(
      "providerForm.modelApiFormats.duplicate",
    );
    fireEvent.change(screen.getByRole("textbox"), {
      target: { value: "messages-model" },
    });
    expect(screen.queryByRole("alert")).toBeNull();
    fireEvent.mouseDown(
      screen.getByRole("tab", { name: /Chat Completions · 1/ }),
      {
        button: 0,
        ctrlKey: false,
      },
    );
    expect(screen.getByRole("textbox")).toHaveValue("chat-model");
    fireEvent.mouseDown(
      screen.getByRole("tab", { name: /Anthropic Messages · 1/ }),
      {
        button: 0,
        ctrlKey: false,
      },
    );
    expect(screen.getByRole("textbox")).toHaveValue("messages-model");
  });

  it("explains the full URL conflict and disables adding overrides", () => {
    render(<Editor fullUrl />);
    expect(screen.getByRole("alert")).toHaveTextContent(
      "providerForm.modelApiFormats.fullUrl",
    );
    expect(
      screen.getByRole("button", { name: "providerForm.modelApiFormats.add" }),
    ).toBeDisabled();
  });
});
