/**
 * Key-capture modal (T11).
 *
 * Hardware capture belongs to the Rust evdev listener: `begin_key_capture` arms
 * it and the combo arrives as a `key_captured` event — never a DOM keydown.
 * `Esc` cancels (`key_capture_cancelled`) and `Backspace` removes the key
 * (`key_captured` with a `null` payload), both interpreted by the backend.
 *
 * `openCapture` is pure with respect to the config: it resolves with the chosen
 * binding and leaves conflict resolution to the caller (`conflictFor` +
 * `clearConflict` are exported for that).
 */
import * as api from "../api";
import type { KeyBinding } from "../api";
import { set, state } from "../store";
import { toast } from "./toast";

/** A hotkey already taken by another sound or by the global "stop all" hotkey. */
export type Conflict = { kind: "sound"; id: string; name: string } | { kind: "stopall"; name: string };

export type CaptureOptions = {
  /** Sound name shown under "Press a key" (`""` for the stop-all hotkey). */
  forName: string;
  /** Combo currently bound, shown until a new one is captured. */
  current?: KeyBinding | null;
  /** Sound being edited: its own hotkey never counts as a conflict. */
  excludeSoundId?: string | null;
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

/** Same physical key and same modifier set, regardless of the order mods arrive in. */
export function sameBinding(a: KeyBinding | null, b: KeyBinding | null): boolean {
  if (a === b) return true; // both null, or literally the same binding
  if (!a || !b) return false;
  return a.code === b.code && [...a.mods].sort().join() === [...b.mods].sort().join();
}

/**
 * Who currently owns `binding`, ignoring `excludeSoundId`'s own hotkey.
 * Exported so the conflict can be recomputed at save time (the binding is only
 * "taken" at the moment the caller writes it).
 */
export function conflictFor(binding: KeyBinding | null, excludeSoundId: string | null = null): Conflict | null {
  if (!binding) return null;
  const owner = state.sounds.find((sound) => sound.id !== excludeSoundId && sameBinding(sound.hotkey, binding));
  if (owner) return { kind: "sound", id: owner.id, name: owner.name };
  if (sameBinding(state.settings.stopAllHotkey, binding)) return { kind: "stopall", name: "Stop all" };
  return null;
}

/** Releases whatever holds `conflict` so the caller can claim the binding. */
export async function clearConflict(conflict: Conflict | null): Promise<void> {
  if (!conflict) return;
  if (conflict.kind === "sound") {
    const updated = await api.updateSound(conflict.id, { hotkey: null });
    set({ sounds: state.sounds.map((sound) => (sound.id === updated.id ? updated : sound)) });
  } else {
    set({ settings: await api.updateSettings({ stopAllHotkey: null }) });
  }
}

/**
 * Opens the capture modal and resolves with the accepted binding, `null` when
 * the key should be removed, or `undefined` when the user cancelled.
 */
export function openCapture(options: CaptureOptions): Promise<KeyBinding | null | undefined> {
  const modal = document.getElementById("capModal") as HTMLElement;
  const ready = document.getElementById("capReady") as HTMLElement;
  const noAccess = document.getElementById("capNoAccess") as HTMLElement;
  const forEl = document.getElementById("capFor") as HTMLElement;
  const display = document.getElementById("capDisplay") as HTMLElement;
  const warn = document.getElementById("capWarn") as HTMLElement;
  const okBtn = document.getElementById("capOk") as HTMLButtonElement;
  const enableBtn = document.getElementById("capEnable") as HTMLButtonElement;
  const cancelBtn = document.getElementById("capCancel") as HTMLButtonElement;

  const inputOk = state.input.state === "ok";

  forEl.textContent = options.forName ? `for “${options.forName}”` : "";
  ready.classList.toggle("hidden", !inputOk);
  noAccess.classList.toggle("hidden", inputOk);
  okBtn.classList.toggle("hidden", !inputOk);
  enableBtn.classList.toggle("hidden", inputOk);
  okBtn.disabled = true;
  warn.classList.add("hidden");
  display.innerHTML = options.current
    ? `<span class="kbd">${esc(options.current.label)}</span>`
    : `<span class="listening">Listening…</span>`;
  modal.classList.remove("hidden");

  return new Promise<KeyBinding | null | undefined>((resolve) => {
    let settled = false;
    let pending: KeyBinding | null = null;
    let conflict: Conflict | null = null;
    const unlisten: Array<() => void> = [];

    // `begin_key_capture` and `cancel_key_capture` are serialised: accepting a key
    // right after a capture would otherwise be able to cancel *before* the
    // re-arm landed, leaving the backend in capture mode (hotkeys mute) until the
    // 20 s timeout.
    let backend: Promise<void> = Promise.resolve();
    function queue(operation: () => Promise<void>): Promise<void> {
      backend = backend.then(operation, operation);
      return backend;
    }

    function close(result: KeyBinding | null | undefined): void {
      if (settled) return;
      settled = true;
      document.removeEventListener("keydown", onDomEsc, true);
      for (const off of unlisten) off();
      // Leaving a capture armed would keep the backend deaf to hotkeys.
      void queue(() => api.cancelKeyCapture().catch(() => {}));
      modal.classList.add("hidden");
      resolve(result);
    }

    // Fallback only: the real `Esc`/`Backspace` handling is the backend's.
    function onDomEsc(event: KeyboardEvent): void {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopImmediatePropagation();
      close(undefined);
    }

    function onCaptured(binding: KeyBinding | null): void {
      pending = binding;
      if (binding === null) {
        // Backspace: the backend asks for the key to be removed.
        close(null);
        return;
      }
      conflict = conflictFor(binding, options.excludeSoundId ?? null);
      display.innerHTML = `<span class="kbd">${esc(binding.label)}</span>`;
      warn.classList.toggle("hidden", !conflict);
      if (conflict) warn.textContent = `Used by “${conflict.name}” — it will be moved here.`;
      okBtn.disabled = false;
      void arm(); // keep the listener armed so another key can be tried
    }

    function arm(): Promise<void> {
      return queue(async () => {
        if (settled) return;
        try {
          await api.beginKeyCapture();
        } catch (err) {
          toast(String(err), "error");
          ready.classList.add("hidden");
          noAccess.classList.remove("hidden");
          okBtn.classList.add("hidden");
          enableBtn.classList.remove("hidden");
        }
      });
    }

    async function register(): Promise<void> {
      const offs = await Promise.all([
        api.onKeyCaptured(onCaptured),
        api.onKeyCaptureCancelled(() => close(undefined)),
      ]);
      if (settled) {
        for (const off of offs) off();
        return;
      }
      unlisten.push(...offs);
      await arm();
    }

    async function enable(): Promise<void> {
      const label = enableBtn.textContent;
      enableBtn.disabled = true;
      enableBtn.textContent = "Waiting for password…";
      try {
        const status = await api.setupInputAccess();
        set({ input: status });
        if (status.state !== "ok") {
          toast("Keyboard access is still not available", "error");
          return;
        }
      } catch (err) {
        toast(String(err), "error");
        return;
      } finally {
        enableBtn.disabled = false;
        enableBtn.textContent = label;
      }
      ready.classList.remove("hidden");
      noAccess.classList.add("hidden");
      okBtn.classList.remove("hidden");
      enableBtn.classList.add("hidden");
      await register();
    }

    cancelBtn.onclick = () => close(undefined);
    okBtn.onclick = () => close(pending);
    enableBtn.onclick = () => void enable();
    document.addEventListener("keydown", onDomEsc, true);

    if (inputOk) void register();
  });
}
