/** Top bar: brand, virtual-mic pill, search, "Add sound". */
import { open } from "@tauri-apps/plugin-dialog";
import * as api from "../api";
import type { RouterStatus } from "../api";
import { set, state, subscribe } from "../store";
import { toast } from "./toast";

/** Extensions the backend can import (PLAN.md §5). */
const AUDIO_EXTENSIONS = ["wav", "ogg", "oga", "opus", "flac", "mp3"];

const ROUTER_LABEL: Record<RouterStatus["state"], string> = {
  starting: "Starting virtual mic…",
  ok: "Virtual mic ready",
  error: "Virtual mic offline",
};

/** Asks for files, then imports whatever was picked. */
export async function pickSounds(): Promise<void> {
  let picked: string | string[] | null;
  try {
    picked = await open({
      multiple: true,
      filters: [{ name: "Audio", extensions: AUDIO_EXTENSIONS }],
    });
  } catch (err) {
    toast(String(err), "error");
    return;
  }
  const paths = Array.isArray(picked) ? picked : picked ? [picked] : [];
  await importPaths(paths);
}

/** Copies `paths` into the sound library, appends the new cards, reports the rest. */
export async function importPaths(paths: string[]): Promise<void> {
  if (paths.length === 0) return;

  let result: api.ImportResult;
  try {
    result = await api.importSounds(paths);
  } catch (err) {
    toast(String(err), "error");
    return;
  }

  if (result.added.length > 0) {
    set({ sounds: [...state.sounds, ...result.added] });
    toast(`Added ${result.added.length} sound${result.added.length === 1 ? "" : "s"}`);
  }
  if (result.rejected.length > 0) toast(`Skipped: ${result.rejected.join(", ")}`, "error");
}

/**
 * Wires the top bar. The pill is display-only: its click opens the settings
 * drawer in T12, so no handler is attached yet.
 */
export function mountTopbar(): void {
  const pill = document.getElementById("routerPill") as HTMLButtonElement;
  const routerText = document.getElementById("routerText") as HTMLElement;
  const search = document.getElementById("search") as HTMLInputElement;
  const addBtn = document.getElementById("addBtn") as HTMLButtonElement;

  search.addEventListener("input", () => set({ query: search.value }));
  addBtn.addEventListener("click", () => void pickSounds());

  subscribe((st) => {
    pill.classList.toggle("error", st.router.state === "error");
    routerText.textContent = ROUTER_LABEL[st.router.state];
    if (search.value !== st.query) search.value = st.query;
  });
}
