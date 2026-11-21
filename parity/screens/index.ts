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
  },
  providers: {
    addButton: by.role("button", "Add Provider", { within: main }),
    /** A provider card's title. Cards have no named container yet, so actions inside a card need seam S4. */
    cardTitle: (name: string) => by.role("heading", name, { within: main }),
    add: {
      customConfig: by.role("button", "Custom Configuration", { within: main }),
      name: by.field("Name", { within: main }),
      apiKey: by.field("API Key", { within: main }),
      endpoint: by.field("API Request URL", { within: main }),
      submit: by.role("button", "Add", { within: main }),
    },
  },
};
