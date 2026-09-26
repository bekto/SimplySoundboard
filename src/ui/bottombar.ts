/**
 * Bottom bar: Stop all, what is playing, and the volume controls.
 *
 * The switch and the two sliders write straight through to the backend and render
 * from the store, so they stay in step with the settings drawer (the other viewer
 * of the same settings).
 */
import * as api from "../api";
import { soundById, subscribe, type State } from "../store";
import { applySettings, debounce, syncRange } from "./settings";
import { toast } from "./toast";

export function mountBottombar(): void {
  const stopAllBtn = document.getElementById("stopAll") as HTMLButtonElement;
  const stopKbd = document.getElementById("stopKbd") as HTMLElement;
  const nowPlaying = document.getElementById("nowPlaying") as HTMLElement;
  const micSwitch = document.getElementById("bbMic") as HTMLInputElement;
  const toMicRange = document.getElementById("bbToMic") as HTMLInputElement;
  const toMicValue = document.getElementById("bbToMicVal") as HTMLElement;
  const monitorRange = document.getElementById("bbMe") as HTMLInputElement;
  const monitorValue = document.getElementById("bbMeVal") as HTMLElement;
  const monitorWrap = document.getElementById("bbMeWrap") as HTMLElement;

  const writeToMic = debounce(() => void applySettings({ toMicVolume: Number(toMicRange.value) / 100 }));
  const writeMonitor = debounce(() => void applySettings({ monitorVolume: Number(monitorRange.value) / 100 }));

  stopAllBtn.addEventListener("click", () => {
    void api.stopAll().catch((err: unknown) => toast(String(err), "error"));
  });
  micSwitch.addEventListener("change", () => void applySettings({ micPassthrough: micSwitch.checked }));
  toMicRange.addEventListener("input", () => {
    toMicValue.textContent = `${toMicRange.value}%`;
    writeToMic();
  });
  monitorRange.addEventListener("input", () => {
    monitorValue.textContent = `${monitorRange.value}%`;
    writeMonitor();
  });

  function renderNowPlaying(st: State): void {
    const names = [...st.playing.keys()]
      .map((id) => soundById(id)?.name)
      .filter((name): name is string => name != null);

    nowPlaying.textContent = names.length > 0 ? "Now playing: " : "Nothing playing";
    if (names.length > 0) {
      const bold = document.createElement("b");
      bold.textContent = names.join(", ");
      nowPlaying.append(bold);
    }
  }

  subscribe((st) => {
    const settings = st.settings;
    stopKbd.textContent = settings.stopAllHotkey?.label ?? "—";
    micSwitch.checked = settings.micPassthrough;
    syncRange(toMicRange, toMicValue, settings.toMicVolume);
    syncRange(monitorRange, monitorValue, settings.monitorVolume);
    monitorWrap.classList.toggle("disabled", !settings.monitorEnabled);
    monitorRange.disabled = !settings.monitorEnabled;
    renderNowPlaying(st);
  });
}
