import { mount } from "svelte";

import App from "./App.svelte";
import { applyApplicationAppearance } from "./lib/application-appearance";
import { DEFAULT_BROWSING_PREFERENCES } from "./lib/types";
import "virtual:design-tokens.css";

const target = document.getElementById("app");

if (!target) {
  throw new Error("App mount point was not found");
}

applyApplicationAppearance(DEFAULT_BROWSING_PREFERENCES);
const app = mount(App, { target });

export default app;
