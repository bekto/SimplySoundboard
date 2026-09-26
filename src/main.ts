/**
 * Frontend entry point: hydrate the static markup, load the backend state into
 * the store, mount the UI modules, then follow what the backend reports.
 *
 * Playback keys are *not* handled here — the Rust evdev listener owns hardware
 * hotkeys, and a DOM keydown handler would fire them a second time.
 */
import { getCurrentWebview } from "@tauri-apps/api/webview";
import * as api from "./api";
import { hydrateIcons } from "./icons";
import { set, state } from "./store";
import { mountBottombar } from "./ui/bottombar";
import { mountGrid } from "./ui/grid";
import { mountTopbar, importPaths } from "./ui/topbar";
import { toast } from "./ui/toast";

async function boot(): Promise<void> {
  hydrateIcons();

  try {
    const initial = await api.getState();
    set({
      sounds: initial.sounds,
      settings: initial.settings,
      router: initial.router,
      input: initial.input,
    });
  } catch (err) {
    toast(String(err), "error");
  }

  mountTopbar();
  mountGrid();
  mountBottombar();

  const dropOverlay = document.getElementById("drop") as HTMLElement;
  await Promise.all([
    api.onPlayback((payload) => {
      if (payload.state === "started") state.playing.set(payload.id, performance.now());
      else state.playing.delete(payload.id);
      // The map is mutated in place; `set` only tells the UI to re-read it.
      set({ playing: state.playing });
    }),
    api.onRouterStatus((status) => set({ router: status })),
    api.onInputStatus((status) => set({ input: status })),
    getCurrentWebview().onDragDropEvent((event) => {
      const payload = event.payload;
      if (payload.type === "drop") {
        dropOverlay.classList.add("hidden");
        void importPaths(payload.paths);
        return;
      }
      dropOverlay.classList.toggle("hidden", payload.type === "leave");
    }),
  ]);
}

boot().catch((err: unknown) => toast(`Frontend failed to start: ${String(err)}`, "error"));
