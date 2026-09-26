/**
 * Edit-sound modal (T11) plus the small confirm dialog shared with the
 * context menu's Delete item.
 *
 * Edits live in a local draft: nothing reaches the backend until Save, which
 * sends a single `update_sound` with only the fields that changed. A key picked
 * through the capture modal is held in the draft too, so cancelling the edit
 * leaves the other sound's hotkey alone.
 */
import * as api from "../api";
import type { KeyBinding, SoundPatch } from "../api";
import { ic } from "../icons";
import { set, soundById, state } from "../store";
import { clearConflict, conflictFor, openCapture, sameBinding, type Conflict } from "./captureModal";
import { toast } from "./toast";

/** Swatches offered in the edit modal (T03 palette). */
const COLORS = ["#7c5cff", "#ec4899", "#f97316", "#eab308", "#22c55e", "#06b6d4", "#3b82f6", "#ef4444"];
/** Quick-pick emoji row, same set as the prototype. */
const EMOJIS = ["🔊", "📯", "😂", "💀", "🎺", "🥁", "👏", "😱", "🏆", "💥", "🐐", "🤡", "🔥", "🚨"];

type Draft = {
  id: string;
  name: string;
  emoji: string;
  color: string;
  volume: number;
  hotkey: KeyBinding | null;
  conflict: Conflict | null;
};

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

const el = <T extends HTMLElement>(id: string): T => document.getElementById(id) as T;

/**
 * `Intl.Segmenter` (ES2022) read off the runtime `Intl` object, because the
 * tsconfig lib is ES2020 and does not declare it.
 */
const Segmenter = (
  Intl as unknown as {
    Segmenter: new (
      locales?: string,
      options?: { granularity: "grapheme" },
    ) => { segment(input: string): Iterable<{ segment: string }> };
  }
).Segmenter;

/** The first `count` graphemes of `value` (keeps flags/ZWJ sequences whole). */
function firstGraphemes(value: string, count: number): string {
  return [...new Segmenter(undefined, { granularity: "grapheme" }).segment(value)]
    .slice(0, count)
    .map((part) => part.segment)
    .join("");
}

/** Removes the Esc listener of whatever edit session is open. */
let activeEsc: ((event: KeyboardEvent) => void) | null = null;

/**
 * Yes/no dialog used by both delete paths. Resolves `true` only when the user
 * confirms; Esc and Cancel resolve `false`.
 */
export function openConfirm(title: string, text: string): Promise<boolean> {
  const modal = el("confirmModal");
  const yes = el<HTMLButtonElement>("cfYes");
  const no = el<HTMLButtonElement>("cfNo");
  el("cfTitle").textContent = title;
  el("cfText").textContent = text;
  modal.classList.remove("hidden");

  return new Promise<boolean>((resolve) => {
    function finish(confirmed: boolean): void {
      modal.classList.add("hidden");
      yes.onclick = null;
      no.onclick = null;
      document.removeEventListener("keydown", onEsc, true);
      resolve(confirmed);
    }
    function onEsc(event: KeyboardEvent): void {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopImmediatePropagation();
      finish(false);
    }
    yes.onclick = () => finish(true);
    no.onclick = () => finish(false);
    document.addEventListener("keydown", onEsc, true);
  });
}

/** Asks first, then deletes the sound and its card. Returns whether it was removed. */
export async function confirmDeleteSound(id: string): Promise<boolean> {
  const sound = soundById(id);
  if (!sound) return false;
  const confirmed = await openConfirm(`Delete “${sound.name}”?`, "The file will be removed.");
  if (!confirmed) return false;

  try {
    await api.deleteSound(id);
  } catch (err) {
    toast(String(err), "error");
    return false;
  }
  state.playing.delete(id);
  set({ sounds: state.sounds.filter((entry) => entry.id !== id), playing: state.playing });
  toast("Sound deleted");
  return true;
}

/** Opens the edit modal for `id` (no-op if the sound is already gone). */
export function openEdit(id: string): void {
  const found = soundById(id);
  if (!found) return;
  const sound = found;

  const modal = el("editModal");
  const nameEl = el<HTMLInputElement>("edName");
  const volEl = el<HTMLInputElement>("edVol");
  const volVal = el("edVolVal");
  const preview = el("edPrev");
  const emojiRow = el("edEmojis");
  const colorRow = el("edColors");
  const keyBtn = el<HTMLButtonElement>("edKey");
  const keyClear = el<HTMLButtonElement>("edKeyClear");

  const draft: Draft = {
    id: sound.id,
    name: sound.name,
    emoji: sound.emoji,
    color: sound.color,
    volume: sound.volume,
    hotkey: sound.hotkey,
    conflict: null,
  };

  nameEl.value = sound.name;
  volEl.value = String(Math.round(sound.volume * 100));
  const duration = fmtDuration(sound.durationMs);
  el("edFile").textContent = duration ? `${sound.file} · ${duration}` : sound.file;

  function render(): void {
    (modal.querySelector(".modal") as HTMLElement).style.setProperty("--c", draft.color);
    preview.textContent = draft.emoji;
    emojiRow.innerHTML =
      EMOJIS.map((emoji) => `<button class="${emoji === draft.emoji ? "on" : ""}" data-e="${emoji}">${emoji}</button>`).join("") +
      `<input type="text" id="edEmojiCustom" maxlength="4" placeholder="…" title="Type or paste any emoji">`;
    colorRow.innerHTML = COLORS.map(
      (color) => `<button style="--s:${color}" class="${color === draft.color ? "on" : ""}" data-c="${color}" title="${color}"></button>`,
    ).join("");
    volVal.textContent = `${volEl.value}%`;
    keyBtn.classList.toggle("set", !!draft.hotkey);
    keyBtn.innerHTML = draft.hotkey
      ? `${ic("keyboard")}<span class="kbd">${esc(draft.hotkey.label)}</span><span class="spacer"></span><span class="muted" style="font-size:12px">Change</span>`
      : `${ic("keyboard")}Click to set a key`;
    keyClear.classList.toggle("hidden", !draft.hotkey);
  }

  async function pickKey(): Promise<void> {
    const binding = await openCapture({
      forName: nameEl.value.trim() || draft.name,
      current: draft.hotkey,
      excludeSoundId: draft.id,
    });
    if (binding === undefined) return; // cancelled — keep the draft as it was
    draft.hotkey = binding;
    draft.conflict = conflictFor(binding, draft.id);
    render();
  }

  function close(): void {
    modal.classList.add("hidden");
    if (activeEsc) {
      document.removeEventListener("keydown", activeEsc, true);
      activeEsc = null;
    }
  }

  function onEsc(event: KeyboardEvent): void {
    if (event.key !== "Escape") return;
    // A capture or confirm dialog sits on top and owns Esc while it is open.
    if (!el("capModal").classList.contains("hidden") || !el("confirmModal").classList.contains("hidden")) return;
    event.preventDefault();
    close();
  }

  async function save(): Promise<void> {
    const name = nameEl.value.trim();
    if (!name) {
      toast("Name can't be empty", "error");
      return;
    }

    const patch: SoundPatch = {};
    if (name !== sound.name) patch.name = name;
    if (draft.emoji !== sound.emoji) patch.emoji = draft.emoji;
    if (draft.color !== sound.color) patch.color = draft.color;
    const volume = Number(volEl.value) / 100;
    if (volume !== sound.volume) patch.volume = volume;
    if (!sameBinding(draft.hotkey, sound.hotkey)) patch.hotkey = draft.hotkey;

    if (Object.keys(patch).length === 0) {
      close();
      return;
    }

    try {
      // The new key is only claimed now, once Save actually commits it.
      if ("hotkey" in patch) await clearConflict(draft.conflict);
      const updated = await api.updateSound(draft.id, patch);
      set({ sounds: state.sounds.map((entry) => (entry.id === updated.id ? updated : entry)) });
    } catch (err) {
      toast(String(err), "error");
      return;
    }
    close();
    toast("Saved");
  }

  emojiRow.onclick = (event) => {
    const button = (event.target as HTMLElement).closest<HTMLElement>("[data-e]");
    if (!button) return;
    draft.emoji = button.dataset.e ?? draft.emoji;
    render();
  };
  emojiRow.oninput = (event) => {
    const input = event.target as HTMLInputElement;
    if (input.id !== "edEmojiCustom") return;
    // Free text is capped at two graphemes (flags, skin tones, ZWJ stay whole).
    const picked = firstGraphemes(input.value.trim(), 2);
    if (!picked) return;
    draft.emoji = picked;
    preview.textContent = picked;
  };
  colorRow.onclick = (event) => {
    const button = (event.target as HTMLElement).closest<HTMLElement>("[data-c]");
    if (!button) return;
    draft.color = button.dataset.c ?? draft.color;
    render();
  };
  volEl.oninput = () => {
    volVal.textContent = `${volEl.value}%`;
  };
  keyBtn.onclick = () => void pickKey();
  keyClear.onclick = () => {
    draft.hotkey = null;
    draft.conflict = null;
    render();
  };
  el<HTMLButtonElement>("edTest").onclick = () => void api.playSound(draft.id).catch((err: unknown) => toast(String(err), "error"));
  el<HTMLButtonElement>("edDelete").onclick = () => {
    void confirmDeleteSound(draft.id).then((deleted) => {
      if (deleted) close();
    });
  };
  el<HTMLButtonElement>("edSave").onclick = () => void save();
  nameEl.onkeydown = (event) => {
    if (event.key !== "Enter") return;
    event.preventDefault();
    void save();
  };
  modal.querySelectorAll<HTMLElement>("[data-close]").forEach((button) => {
    button.onclick = close;
  });

  if (activeEsc) document.removeEventListener("keydown", activeEsc, true);
  activeEsc = onEsc;
  document.addEventListener("keydown", onEsc, true);

  render();
  modal.classList.remove("hidden");
  nameEl.focus();
  nameEl.select();
}
