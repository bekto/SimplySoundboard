/**
 * Typed wrapper around the Tauri command + event IPC (PLAN.md §6 is the contract).
 * Rust structs use `#[serde(rename_all = "camelCase")]`, so the wire format is
 * camelCase and the types below mirror the backend field for field.
 */
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type Mod = "ctrl" | "shift" | "alt" | "super";

export type KeyBinding = { code: number; mods: Mod[]; label: string };

export type Sound = {
  id: string;
  name: string;
  file: string;
  emoji: string;
  color: string;
  volume: number;
  durationMs: number | null;
  hotkey: KeyBinding | null;
};

export type Settings = {
  micPassthrough: boolean;
  micSource: string | null;
  micVolume: number;
  toMicVolume: number;
  monitorEnabled: boolean;
  monitorVolume: number;
  useAsDefaultMic: boolean;
  retrigger: "restart" | "overlap" | "stop";
  stopAllHotkey: KeyBinding | null;
  closeToTray: boolean;
  startMinimized: boolean;
};

export type RouterStatus = { state: "starting" | "ok" | "error"; message: string | null };

export type InputStatus = { state: "ok" | "noPermission" | "noKeyboards"; keyboards: string[] };

export type MicDevice = { name: string; description: string; isDefault: boolean };

export type AppState = {
  sounds: Sound[];
  settings: Settings;
  router: RouterStatus;
  input: InputStatus;
};

export type SoundPatch = Partial<Pick<Sound, "name" | "emoji" | "color" | "volume" | "hotkey">>;

/** `import_sounds` result: the cards that were added plus the file names that were skipped. */
export type ImportResult = { added: Sound[]; rejected: string[] };

/** `playback` event payload. */
export type PlaybackEvent = { id: string; state: "started" | "ended" };

// ---------- commands ----------

export const getState = (): Promise<AppState> => invoke<AppState>("get_state");

export const importSounds = (paths: string[]): Promise<ImportResult> =>
  invoke<ImportResult>("import_sounds", { paths });

export const updateSound = (id: string, patch: SoundPatch): Promise<Sound> =>
  invoke<Sound>("update_sound", { id, patch });

export const deleteSound = (id: string): Promise<void> => invoke<void>("delete_sound", { id });

export const reorderSounds = (ids: string[]): Promise<void> => invoke<void>("reorder_sounds", { ids });

export const playSound = (id: string): Promise<void> => invoke<void>("play_sound", { id });

export const stopSound = (id: string): Promise<void> => invoke<void>("stop_sound", { id });

export const stopAll = (): Promise<void> => invoke<void>("stop_all");

export const updateSettings = (patch: Partial<Settings>): Promise<Settings> =>
  invoke<Settings>("update_settings", { patch });

export const listMics = (): Promise<MicDevice[]> => invoke<MicDevice[]>("list_mics");

export const restartRouter = (): Promise<RouterStatus> => invoke<RouterStatus>("restart_router");

export const beginKeyCapture = (): Promise<void> => invoke<void>("begin_key_capture");

export const cancelKeyCapture = (): Promise<void> => invoke<void>("cancel_key_capture");

export const setupInputAccess = (): Promise<InputStatus> => invoke<InputStatus>("setup_input_access");

// ---------- events ----------

export const onPlayback = (handler: (payload: PlaybackEvent) => void): Promise<UnlistenFn> =>
  listen<PlaybackEvent>("playback", (event) => handler(event.payload));

export const onKeyCaptured = (handler: (binding: KeyBinding | null) => void): Promise<UnlistenFn> =>
  listen<KeyBinding | null>("key_captured", (event) => handler(event.payload));

export const onKeyCaptureCancelled = (handler: () => void): Promise<UnlistenFn> =>
  listen("key_capture_cancelled", () => handler());

export const onRouterStatus = (handler: (status: RouterStatus) => void): Promise<UnlistenFn> =>
  listen<RouterStatus>("router_status", (event) => handler(event.payload));

export const onInputStatus = (handler: (status: InputStatus) => void): Promise<UnlistenFn> =>
  listen<InputStatus>("input_status", (event) => handler(event.payload));
