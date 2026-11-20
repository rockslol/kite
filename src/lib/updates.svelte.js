// App updates: the quiet startup check (from the backend) and the Settings buttons.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { toasts } from "./ui.svelte.js";

class Updates {
  available = $state(null);
  checked = $state(false);
  checking = $state(false);
  installing = $state(false);

  constructor() {
    listen("update-available", ({ payload }) => {
      this.available = payload;
      toasts.show(`Kite ${payload} is available. Install it from Settings.`);
    });
  }

  async check(manual = false) {
    this.checking = true;
    try {
      this.available = await invoke("update_check");
      this.checked = true;
    } catch (e) {
      if (manual) toasts.show("Couldn't reach the update server. Try again later.");
      console.warn("update check failed:", e);
    }
    this.checking = false;
  }

  async install() {
    this.installing = true;
    try {
      await invoke("update_install");
    } catch (e) {
      toasts.show(String(e));
      this.installing = false;
    }
  }
}

export const updates = new Updates();
