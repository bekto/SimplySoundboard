/**
 * Card context menu (T11): Play / Edit… / Change key… / Remove key / Delete.
 *
 * The menu is a single detached-until-shown element (`#menu`); it closes on
 * outside mousedown, Esc, any scroll and window blur, and is flipped/clamped so
 * it always stays inside the viewport. Delete always asks first.
 */
import * as api from "../api";
import type { KeyBinding } from "../api";
import { ic } from "../icons";
import { set, soundById, state } from "../store";
import { clearConflict, conflictFor, openCapture } from "./captureModal";
import { confirmDeleteSound, openEdit } from "./editModal";
import { toast } from "./toast";

/** Tears down the listeners of the currently open menu (if any). */
let disposeMenu: (() => void) | null = null;

/**
 * Captures a new key for `id` (or removes it) and persists the change, moving
 * any conflicting binding out of the way first. Shared with the grid's "Set key"
 * chip via `openMenu`'s module (`assignKey`).
 */
export async function assignKey(id: string): Promise<void> {
  const sound = soundById(id);
  if (!sound) return;

  const binding = await openCapture({ forName: sound.name, current: sound.hotkey, excludeSoundId: id });
  if (binding === undefined) return; // cancelled

  try {
    await saveKey(id, binding);
  } catch (err) {
    toast(String(err), "error");
    return;
  }
  const name = soundById(id)?.name ?? sound.name;
  toast(binding ? `“${name}” → ${binding.label}` : `Removed key from “${name}”`);
}

/** Clears whoever owns the combo, then writes it to `id` (which may be a removal). */
async function saveKey(id: string, binding: KeyBinding | null): Promise<void> {
  await clearConflict(conflictFor(binding, id));
  const updated = await api.updateSound(id, { hotkey: binding });
  set({ sounds: state.sounds.map((sound) => (sound.id === updated.id ? updated : sound)) });
}

/** Opens the menu for card `id` at viewport coordinates `x, y`. */
export function openMenu(id: string, x: number, y: number): void {
  const menu = document.getElementById("menu") as HTMLElement;
  const sound = soundById(id);
  if (!sound) return;

  disposeMenu?.();

  menu.innerHTML = `
    <button data-m="play">${ic("play")}Play</button>
    <button data-m="edit">${ic("edit")}Edit…</button>
    <button data-m="key">${ic("keyboard")}${sound.hotkey ? "Change key…" : "Set key…"}</button>
    ${sound.hotkey ? `<button data-m="unkey">${ic("unlink")}Remove key</button>` : ""}
    <hr>
    <button data-m="del" class="danger">${ic("trash")}Delete</button>`;
  menu.classList.remove("hidden");

  // Flip toward the click when the menu would run off the bottom/right edge.
  const rect = menu.getBoundingClientRect();
  const left = x + rect.width > innerWidth - 8 ? x - rect.width : x;
  const top = y + rect.height > innerHeight - 8 ? y - rect.height : y;
  menu.style.left = `${Math.min(Math.max(8, left), Math.max(8, innerWidth - rect.width - 8))}px`;
  menu.style.top = `${Math.min(Math.max(8, top), Math.max(8, innerHeight - rect.height - 8))}px`;

  function close(): void {
    menu.classList.add("hidden");
    disposeMenu?.();
    disposeMenu = null;
  }

  function onOutside(event: MouseEvent): void {
    if (!(event.target as HTMLElement).closest("#menu")) close();
  }

  function onKey(event: KeyboardEvent): void {
    if (event.key !== "Escape") return;
    // Modal dialogs are stacked above the menu and own Esc while they are open.
    if (!(document.getElementById("capModal") as HTMLElement).classList.contains("hidden")) return;
    if (!(document.getElementById("editModal") as HTMLElement).classList.contains("hidden")) return;
    event.preventDefault();
    close();
  }

  disposeMenu = () => {
    document.removeEventListener("mousedown", onOutside, true);
    document.removeEventListener("scroll", close, true);
    document.removeEventListener("keydown", onKey, true);
    window.removeEventListener("blur", close);
  };
  document.addEventListener("mousedown", onOutside, true);
  document.addEventListener("scroll", close, true); // capture: `#main` scrolls, not the document
  document.addEventListener("keydown", onKey, true);
  window.addEventListener("blur", close);

  menu.onclick = (event) => {
    const action = (event.target as HTMLElement).closest<HTMLElement>("[data-m]")?.dataset.m;
    if (!action) return;
    close();
    if (action === "play") void api.playSound(id).catch((err: unknown) => toast(String(err), "error"));
    else if (action === "edit") openEdit(id);
    else if (action === "key") void assignKey(id);
    else if (action === "unkey") void removeKey(id);
    else if (action === "del") void confirmDeleteSound(id);
  };
}

/** "Remove key" menu item: clears the binding without opening the capture modal. */
async function removeKey(id: string): Promise<void> {
  const sound = soundById(id);
  if (!sound) return;
  try {
    await saveKey(id, null);
  } catch (err) {
    toast(String(err), "error");
    return;
  }
  toast(`Removed key from “${soundById(id)?.name ?? sound.name}”`);
}
