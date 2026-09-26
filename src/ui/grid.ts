/** Card grid: card rendering, search filtering, playback state, add/empty tiles. */
import * as api from "../api";
import type { Sound } from "../api";
import { ic } from "../icons";
import { set, soundById, state, subscribe, type State } from "../store";
import { assignKey, openMenu } from "./menu";
import { pickSounds } from "./topbar";
import { toast } from "./toast";

function esc(value: string): string {
  const entities: Record<string, string> = {
    "&": "&amp;",
    "<": "&lt;",
    ">": "&gt;",
    '"': "&quot;",
    "'": "&#39;",
  };
  return value.replace(/[&<>"']/g, (ch) => entities[ch] ?? ch);
}

/** "2.4s" or "1:05"; empty when the backend could not probe the length. */
function fmtDuration(durationMs: number | null): string {
  if (durationMs == null) return "";
  const seconds = durationMs / 1000;
  if (seconds < 60) return `${seconds.toFixed(1)}s`;
  return `${Math.floor(seconds / 60)}:${String(Math.round(seconds % 60)).padStart(2, "0")}`;
}

/** One card, markup-for-markup the prototype's `cardHTML`. */
function cardHtml(sound: Sound, playing: boolean): string {
  const key = sound.hotkey
    ? `<span class="kbd" data-action="assign" title="Change key">${esc(sound.hotkey.label)}</span>`
    : `<span class="kbd empty" data-action="assign">Set key</span>`;
  return `<div class="card${playing ? " playing" : ""}" data-id="${sound.id}" draggable="true" tabindex="0" style="--c:${sound.color}" title="Click to play">
    <div class="glow"></div><div class="stripe"></div>
    <button class="more" data-action="menu" title="More">${ic("more")}</button>
    <div class="emoji">${esc(sound.emoji)}</div>
    <div class="name">${esc(sound.name)}</div>
    <div class="meta">
      ${key}
      <span style="display:inline-flex;gap:6px;align-items:center"><span class="eq"><i></i><i></i><i></i></span>${fmtDuration(sound.durationMs)}</span>
    </div>
    <div class="progress"></div>
  </div>`;
}

/**
 * Wires the grid: renders cards plus the add/empty state, keeps the playing
 * classes and progress bars in sync without rebuilding the DOM per frame.
 */
export function mountGrid(): void {
  const grid = document.getElementById("grid") as HTMLElement;
  const empty = document.getElementById("empty") as HTMLElement;
  const emptyBox = document.getElementById("emptyBox") as HTMLElement;

  let renderedSounds: Sound[] | null = null;
  let renderedQuery = "";
  let playingBefore = new Set<string>();
  let rafId = 0;

  function renderCards(st: State): void {
    const query = st.query.trim().toLowerCase();
    const matches = query ? st.sounds.filter((s) => s.name.toLowerCase().includes(query)) : st.sounds;

    empty.classList.toggle("hidden", st.sounds.length > 0);
    grid.classList.toggle("hidden", st.sounds.length === 0);

    let html = matches.map((s) => cardHtml(s, st.playing.has(s.id))).join("");
    if (query && matches.length === 0) html = `<div class="no-results">No sounds match “${esc(query)}”</div>`;
    if (!query) {
      html += `<button class="card add-card" id="addCard">${ic("plus", "lg")}<span>Add sound</span><small>or drop files anywhere</small></button>`;
    }
    grid.innerHTML = html;
  }

  function syncPlaying(st: State): void {
    for (const card of grid.querySelectorAll<HTMLElement>(".card[data-id]")) {
      const id = card.dataset.id ?? "";
      const playing = st.playing.has(id);
      card.classList.toggle("playing", playing);
      card.classList.toggle("indeterminate", playing && soundById(id)?.durationMs == null);

      if (!playing) {
        const bar = card.querySelector<HTMLElement>(".progress");
        if (bar) bar.style.width = "0";
      } else if (!playingBefore.has(id)) {
        card.classList.remove("pulse");
        void card.offsetWidth; // restart the pop animation
        card.classList.add("pulse");
      }
    }
    playingBefore = new Set(st.playing.keys());
    if (st.playing.size > 0 && rafId === 0) rafId = requestAnimationFrame(tick);
  }

  /** Advances every known-length progress bar. Stops once they all ran out. */
  function tick(): void {
    rafId = 0;
    const now = performance.now();
    let running = false;

    for (const [id, startedAt] of [...state.playing]) {
      const durationMs = soundById(id)?.durationMs;
      if (durationMs == null) continue; // no known length: the CSS pulse runs instead
      const elapsed = now - startedAt;
      const card = grid.querySelector<HTMLElement>(`.card[data-id="${CSS.escape(id)}"]`);
      const bar = card?.querySelector<HTMLElement>(".progress");
      if (bar) bar.style.width = `${Math.min(100, (elapsed / durationMs) * 100)}%`;
      if (elapsed < durationMs) running = true;
    }

    if (running) rafId = requestAnimationFrame(tick);
  }

  grid.addEventListener("click", (event) => {
    const target = event.target as HTMLElement;

    const addCard = target.closest("#addCard");
    if (addCard) {
      void pickSounds();
      return;
    }

    const card = target.closest<HTMLElement>(".card[data-id]");
    if (!card) return;
    const id = card.dataset.id ?? "";
    const action = target.closest<HTMLElement>("[data-action]")?.dataset.action;

    if (action === "menu") {
      const button = target.closest("button");
      if (button) {
        const rect = button.getBoundingClientRect();
        openMenu(id, rect.right - 180, rect.bottom + 4);
      }
      return;
    }
    if (action === "assign") {
      // "Set key" / "Change key" chip: capture a key and save it right away.
      void assignKey(id);
      return;
    }
    void api.playSound(id).catch((err: unknown) => toast(String(err), "error"));
  });

  grid.addEventListener("contextmenu", (event) => {
    const card = (event.target as HTMLElement).closest<HTMLElement>(".card[data-id]");
    if (!card) return;
    event.preventDefault();
    openMenu(card.dataset.id ?? "", event.clientX, event.clientY);
  });

  grid.addEventListener("keydown", (event) => {
    const card = (event.target as HTMLElement).closest<HTMLElement>(".card[data-id]");
    if (!card || (event.key !== "Enter" && event.key !== " ")) return;
    event.preventDefault();
    void api.playSound(card.dataset.id ?? "").catch((err: unknown) => toast(String(err), "error"));
  });

  // ---- drag to reorder ----------------------------------------------------
  // `dragId` is only ever set by our own dragstart, so file drags (handled by
  // the window's drag-drop event) can never reorder anything.
  let dragId: string | null = null;

  grid.addEventListener("dragstart", (event) => {
    const card = (event.target as HTMLElement).closest<HTMLElement>(".card[data-id]");
    if (!card || !event.dataTransfer || !card.dataset.id) return;
    dragId = card.dataset.id;
    card.classList.add("dragging");
    event.dataTransfer.effectAllowed = "move";
    event.dataTransfer.setData("text/plain", dragId);
  });

  grid.addEventListener("dragend", () => {
    dragId = null;
    for (const el of grid.querySelectorAll(".dragging,.drop-target")) el.classList.remove("dragging", "drop-target");
  });

  grid.addEventListener("dragover", (event) => {
    if (!dragId || !event.dataTransfer || event.dataTransfer.types.includes("Files")) return;
    event.preventDefault(); // required for the drop to fire
    event.dataTransfer.dropEffect = "move";
    const card = (event.target as HTMLElement).closest<HTMLElement>(".card[data-id]");
    for (const el of grid.querySelectorAll(".drop-target")) {
      if (el !== card) el.classList.remove("drop-target");
    }
    if (card && card.dataset.id !== dragId) card.classList.add("drop-target");
  });

  grid.addEventListener("drop", (event) => {
    const from = dragId;
    dragId = null;
    const card = (event.target as HTMLElement).closest<HTMLElement>(".card[data-id]");
    if (!from || !card) return;
    event.preventDefault();
    const targetId = card.dataset.id ?? "";
    if (!targetId || targetId === from) return;

    // Optimistic: the card lands before the drop target, then the backend confirms.
    const sounds = [...state.sounds];
    const fromIndex = sounds.findIndex((sound) => sound.id === from);
    if (fromIndex < 0) return;
    const [moved] = sounds.splice(fromIndex, 1);
    const toIndex = sounds.findIndex((sound) => sound.id === targetId);
    if (toIndex < 0) return;
    sounds.splice(toIndex, 0, moved);
    set({ sounds });

    void api.reorderSounds(sounds.map((sound) => sound.id)).catch(async (err: unknown) => {
      toast(String(err), "error");
      try {
        const fresh = await api.getState();
        set({ sounds: fresh.sounds, settings: fresh.settings, router: fresh.router, input: fresh.input });
      } catch {
        /* keep the optimistic order if the reload fails too */
      }
    });
  });

  emptyBox.addEventListener("click", () => void pickSounds());

  subscribe((st) => {
    if (st.sounds !== renderedSounds || st.query !== renderedQuery) {
      renderedSounds = st.sounds;
      renderedQuery = st.query;
      renderCards(st);
    }
    syncPlaying(st);
  });
}
