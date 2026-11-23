import { mount } from "svelte";
import "@fontsource/inter/400.css";
import "@fontsource/inter/500.css";
import "@fontsource/inter/600.css";
import "./app.css";
import App from "./App.svelte";

mount(App, { target: document.getElementById("app") });

// Dev builds only: a handle for end-to-end test scripts. Not in release builds.
if (import.meta.env.DEV) {
  Promise.all([import("./lib/player.svelte.js"), import("./lib/sound.svelte.js")]).then(
    ([{ player }, { sound }]) => (window.__kite = { player, sound })
  );
}
