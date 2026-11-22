// The screen map: the only place targets are built. Names are what a user reads on the control,
// in English (the cases run with LANG=en_US.UTF-8 and a fresh HOME).
import { by } from "../kit/screen-builders.ts";

const welcome = by.role("dialog", "Welcome to CC Switch");
const nav = by.role("navigation", "Main navigation");
const apps = by.role("group", "Apps", { within: nav });
const main = by.role("main", "");

export const S = {
  firstRun: {
    dialog: welcome,
    gotIt: by.role("button", "Got it", { within: welcome }),
  },
  shell: {
    /** An app in the sidebar, by the name the sidebar shows. */
    sidebarApp: (name: string) => by.role("button", name, { within: apps }),
    /** A section below the apps in the sidebar: MCP, Skills, Usage, Settings and so on. */
    section: (name: string) => by.role("button", name, { within: nav }),
  },
  providers: {
    addButton: by.role("button", "Add Provider", { within: main }),
    /** A provider card's title. */
    cardTitle: (name: string) => by.role("heading", name, { within: main }),
    /** A provider card: a group named after its provider. */
    card: (name: string) => by.role("group", name, { within: main }),
    /** A button on one provider card, by the label it shows or its accessible name. */
    cardButton: (provider: string, label: string) => by.role("button", label, { within: by.role("group", provider, { within: main }) }),
    add: {
      customConfig: by.role("button", "Custom Configuration", { within: main }),
      name: by.field("Name", { within: main }),
      apiKey: by.field("API Key", { within: main }),
      /** The endpoint field of Codex-style forms (Codex, Grok Build). */
      requestUrl: by.field("API Request URL", { within: main }),
      /** The endpoint field of Claude-style forms (Claude Code, Claude Desktop, Gemini CLI). */
      apiEndpoint: by.field("API Endpoint", { within: main }),
      submit: by.role("button", "Add", { within: main }),
    },
  },
};
