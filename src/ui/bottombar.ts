/**
 * Bottom bar: Stop all, what is playing, and the volume controls.
 * The switch/ranges render from settings; T12 makes them editable.
 */
import * as api from "../api";
import { soundById, subscribe, type State } from "../store";
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

  stopAllBtn.addEventListener("click", () => {
    void api.stopAll().catch((err: unknown) => toast(String(err), "error"));
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
    stopKbd.textContent = st.settings.stopAllHotkey?.label ?? "—";
    micSwitch.checked = st.settings.micPassthrough;
    toMicRange.value = String(Math.round(st.settings.toMicVolume * 100));
    toMicValue.textContent = `${toMicRange.value}%`;
    monitorRange.value = String(Math.round(st.settings.monitorVolume * 100));
    monitorValue.textContent = `${monitorRange.value}%`;
    monitorWrap.classList.toggle("disabled", !st.settings.monitorEnabled);
    renderNowPlaying(st);
  });
}
