/**
 * Settings drawer, keyboard-access banner and the shared settings writer (T12).
 *
 * Nothing here keeps its own copy of the settings: every control writes the patch
 * through `applySettings`, which mirrors the `Settings` the backend returns into
 * the store, and every control renders from the store — so the drawer and the
 * bottom bar (the other reader) can never drift apart.
 */
import { getVersion } from "@tauri-apps/api/app";
import * as api from "../api";
import type { MicDevice, Settings } from "../api";
import { set, state, subscribe, type State } from "../store";
import { clearConflict, conflictFor, openCapture } from "./captureModal";
import { toast } from "./toast";

/** Banner copy for the missing-permission state (matches the prototype). */
const PERMISSION_BANNER =
  `<b>Hotkeys are off.</b> SimplySoundboard needs permission to see key presses while you're in a game.` +
  `<small>One-time setup · asks for your password</small>`;

/** Shown once the rule is installed but the new ACL still needs a fresh login. */
const ALMOST_DONE = "Almost done — log out and back in to finish.";

const WAITING = "Waiting for password…";

const ENTITIES: Record<string, string> = {
  "&": "&amp;",
  "<": "&lt;",
  ">": "&gt;",
  '"': "&quot;",
  "'": "&#39;",
};

function escapeHtml(value: string): string {
  return value.replace(/[&<>"']/g, (ch: string) => ENTITIES[ch] ?? ch);
}

function byId<T extends HTMLElement = HTMLElement>(id: string): T {
  const found = document.getElementById(id);
  if (!found) throw new Error(`Missing #${id} in index.html`);
  return found as T;
}

/**
 * Sends `patch`, mirrors the returned settings into the store, reports failures.
 * Returns whether the backend accepted the change.
 */
export async function applySettings(patch: Partial<Settings>): Promise<boolean> {
  try {
    set({ settings: await api.updateSettings(patch) });
    return true;
  } catch (err) {
    toast(String(err), "error");
    // The backend persists before it touches live audio, so a patch that failed to
    // apply may still have been saved: re-read instead of leaving the controls lying.
    try {
      set({ settings: (await api.getState()).settings });
    } catch {
      /* the toast above is the useful half of this failure */
    }
    return false;
  }
}

/** Runs `write` once the slider has been quiet for `delay` ms — live, cheap IPC. */
export function debounce(write: () => void, delay = 80): () => void {
  let timer: number | undefined;
  return () => {
    if (timer !== undefined) window.clearTimeout(timer);
    timer = window.setTimeout(() => {
      timer = undefined;
      write();
    }, delay);
  };
}

/**
 * Paints a 0–1.5 volume onto a 0–150 range without fighting a drag in progress.
 * Shared by the drawer's mic slider and the bottom bar's two sliders.
 */
export function syncRange(range: HTMLInputElement, label: HTMLElement, volume: number): void {
  if (document.activeElement !== range) range.value = String(Math.round(volume * 100));
  label.textContent = `${range.value}%`;
}

let drawerEl: HTMLElement | null = null;
let scrimEl: HTMLElement | null = null;
let reloadMics: () => void = () => {};

/** Opens the drawer and re-reads the device list (a headset may have been plugged in). */
export function openDrawer(): void {
  drawerEl?.classList.add("open");
  scrimEl?.classList.remove("hidden");
  reloadMics();
}

export function closeDrawer(): void {
  drawerEl?.classList.remove("open");
  scrimEl?.classList.add("hidden");
}

/** Wires the banner and the drawer. Call once, after the first `get_state`. */
export function mountSettings(): void {
  const banner = byId("banner");
  const bannerText = byId("bannerText");
  const bannerBtn = byId<HTMLButtonElement>("bannerBtn");
  drawerEl = byId("drawer");
  scrimEl = byId("scrim");
  const drawerClose = byId<HTMLButtonElement>("drawerClose");
  const vmCard = byId("vmCard");
  const vmTitle = byId("vmTitle");
  const vmSub = byId("vmSub");
  const defaultSwitch = byId<HTMLInputElement>("sDefault");
  const recreateBtn = byId<HTMLButtonElement>("sRecreate");
  const micSwitch = byId<HTMLInputElement>("sMic");
  const micDev = byId<HTMLSelectElement>("sMicDev");
  const micVol = byId<HTMLInputElement>("sMicVol");
  const micVolVal = byId("sMicVolVal");
  const monitorSwitch = byId<HTMLInputElement>("sMonitor");
  const retrigger = byId("sRetrigger");
  const stopKey = byId<HTMLButtonElement>("sStopKey");
  const access = byId("sAccess");
  const traySwitch = byId<HTMLInputElement>("sTray");
  const startMinSwitch = byId<HTMLInputElement>("sStartMin");
  const about = document.querySelector<HTMLElement>(".drawer .about");

  /** Set when `setup_input_access` installed the rule but the ACL needs a re-login. */
  let almostDone = false;

  // ---------- keyboard access ----------
  async function enableHotkeys(button: HTMLButtonElement): Promise<void> {
    button.disabled = true;
    button.textContent = WAITING;
    try {
      const status = await api.setupInputAccess();
      // `almostDone` has to be in place before `set` notifies the renderers.
      almostDone = status.state !== "ok";
      set({ input: status });
      if (status.state === "ok") toast("Hotkeys enabled");
    } catch (err) {
      toast(String(err), "error");
    } finally {
      button.disabled = false;
      button.textContent = "Enable hotkeys";
    }
  }

  function renderBanner(st: State): void {
    const input = st.input;
    if (input.state === "ok") {
      almostDone = false;
      banner.classList.add("hidden");
      return;
    }
    banner.classList.remove("hidden");
    if (input.state === "noKeyboards") {
      bannerText.innerHTML = `<b>No keyboard detected.</b> Hotkeys need a keyboard to listen to.`;
      bannerBtn.classList.add("hidden");
      return;
    }
    bannerBtn.classList.remove("hidden");
    bannerText.innerHTML = almostDone ? ALMOST_DONE : PERMISSION_BANNER;
  }

  function renderAccess(st: State): void {
    if (st.input.state === "ok") {
      const count = st.input.keyboards.length;
      const names = escapeHtml(st.input.keyboards.join("\n"));
      access.innerHTML =
        `<span class="ok">●</span> Enabled ` +
        `<span class="muted" title="${names}">· ${count} keyboard${count === 1 ? "" : "s"}</span>`;
      return;
    }
    access.innerHTML =
      `<span class="bad">●</span> Not enabled <span class="spacer"></span>` +
      `<button class="btn primary small" data-enable>Enable hotkeys</button>`;
  }

  bannerBtn.addEventListener("click", () => void enableHotkeys(bannerBtn));
  access.addEventListener("click", (event) => {
    const button = (event.target as HTMLElement).closest<HTMLButtonElement>("[data-enable]");
    if (button) void enableHotkeys(button);
  });

  // ---------- drawer open/close ----------
  drawerClose.addEventListener("click", closeDrawer);
  scrimEl.addEventListener("click", closeDrawer);
  document.addEventListener(
    "keydown",
    (event) => {
      if (event.key !== "Escape") return;
      // A modal on top owns Esc while it is open.
      if (document.querySelector(".backdrop:not(.hidden)")) return;
      if (!drawerEl?.classList.contains("open")) return;
      event.preventDefault();
      closeDrawer();
    },
    true,
  );

  // ---------- your microphone ----------
  function renderMics(devices: MicDevice[]): void {
    const current = state.settings.micSource;
    const options = devices.map(
      (device) => `<option value="${escapeHtml(device.name)}">${escapeHtml(device.description)}</option>`,
    );
    // A saved device that is unplugged must stay selectable, or the picker would
    // silently show "System default" while the backend still records the old one.
    if (current && !devices.some((device) => device.name === current)) {
      options.push(`<option value="${escapeHtml(current)}">${escapeHtml(current)} (unavailable)</option>`);
    }
    micDev.innerHTML = `<option value="">System default</option>${options.join("")}`;
    micDev.value = current ?? "";
  }

  async function loadMics(): Promise<void> {
    try {
      renderMics(await api.listMics());
    } catch (err) {
      toast(String(err), "error");
    }
  }
  reloadMics = () => void loadMics();

  const writeMicVolume = debounce(() => void applySettings({ micVolume: Number(micVol.value) / 100 }));

  micSwitch.addEventListener("change", () => void applySettings({ micPassthrough: micSwitch.checked }));
  micDev.addEventListener("change", () => void applySettings({ micSource: micDev.value || null }));
  micVol.addEventListener("input", () => {
    micVolVal.textContent = `${micVol.value}%`;
    writeMicVolume();
  });

  // ---------- virtual microphone ----------
  defaultSwitch.addEventListener("change", () => {
    const enabled = defaultSwitch.checked;
    void applySettings({ useAsDefaultMic: enabled }).then((ok) => {
      if (!ok) return;
      toast(enabled ? "SimplySoundboard Mic is now your default mic" : "Default mic restored");
    });
  });

  recreateBtn.addEventListener("click", () => {
    void api
      .restartRouter()
      .then((status) => {
        set({ router: status });
        if (status.state === "ok") toast("Virtual mic recreated");
        else toast(status.message ?? "Virtual mic is still offline", "error");
      })
      .catch((err: unknown) => toast(String(err), "error"));
  });

  // ---------- playback ----------
  monitorSwitch.addEventListener("change", () => void applySettings({ monitorEnabled: monitorSwitch.checked }));

  retrigger.addEventListener("click", (event) => {
    const button = (event.target as HTMLElement).closest<HTMLElement>("[data-v]");
    const value = button?.dataset.v;
    if (!value) return;
    void applySettings({ retrigger: value as Settings["retrigger"] });
  });

  // ---------- hotkeys ----------
  stopKey.addEventListener("click", () => {
    void (async () => {
      const binding = await openCapture({ forName: "Stop all", current: state.settings.stopAllHotkey });
      if (binding === undefined) return; // cancelled
      const conflict = conflictFor(binding);
      // `conflictFor` only ever reports "stopall" for the key we already hold, so
      // that one is not a conflict — the sound conflicts have to move aside.
      try {
        if (conflict?.kind !== "stopall") await clearConflict(conflict);
      } catch (err) {
        toast(String(err), "error");
        return;
      }
      if (await applySettings({ stopAllHotkey: binding })) {
        toast(binding ? `Stop all → ${binding.label}` : "Stop-all key removed");
      }
    })();
  });

  // ---------- app ----------
  traySwitch.addEventListener("change", () => void applySettings({ closeToTray: traySwitch.checked }));
  startMinSwitch.addEventListener("change", () => void applySettings({ startMinimized: startMinSwitch.checked }));

  void getVersion()
    .then((version) => {
      if (about) about.textContent = `SimplySoundboard ${version} · Requires PipeWire`;
    })
    .catch(() => {
      /* the version is cosmetic: keep the placeholder already in the markup */
    });

  // ---------- render ----------
  function renderSettings(st: State): void {
    const settings = st.settings;

    vmCard.classList.toggle("error", st.router.state === "error");
    if (st.router.state === "ok") {
      vmTitle.textContent = "SimplySoundboard Mic is ready";
      vmSub.textContent = "In your game, Discord or OBS, choose “SimplySoundboard Mic” as the microphone.";
    } else if (st.router.state === "error") {
      vmTitle.textContent = "Virtual mic couldn't be created";
      vmSub.textContent = st.router.message ?? "The virtual microphone could not be started.";
    } else {
      vmTitle.textContent = "Starting virtual mic…";
      vmSub.textContent = "Building the audio graph.";
    }

    defaultSwitch.checked = settings.useAsDefaultMic;
    micSwitch.checked = settings.micPassthrough;
    if (micDev.value !== (settings.micSource ?? "")) micDev.value = settings.micSource ?? "";
    syncRange(micVol, micVolVal, settings.micVolume);
    monitorSwitch.checked = settings.monitorEnabled;
    for (const button of retrigger.querySelectorAll<HTMLElement>("[data-v]")) {
      button.classList.toggle("on", button.dataset.v === settings.retrigger);
    }
    stopKey.classList.toggle("set", settings.stopAllHotkey != null);
    stopKey.innerHTML = settings.stopAllHotkey
      ? `<span class="kbd">${escapeHtml(settings.stopAllHotkey.label)}</span>`
      : `<span class="muted">Set key</span>`;
    traySwitch.checked = settings.closeToTray;
    startMinSwitch.checked = settings.startMinimized;
  }

  subscribe((st) => {
    renderBanner(st);
    renderAccess(st);
    renderSettings(st);
  });
}
