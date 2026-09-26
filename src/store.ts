/**
 * Tiny observable app store — no framework, no dependencies.
 *
 * `state` holds the backend's `AppState` plus the two pieces of view state the
 * shell owns: which sounds are playing right now and the search query. Later
 * tickets mutate both through `set`.
 */
import type { AppState, Sound, Settings } from "./api";

/** Sound id -> `performance.now()` at the moment the backend reported "started". */
export type PlayingMap = Map<string, number>;

export type State = AppState & { playing: PlayingMap; query: string };

export type Listener = (state: State) => void;

const DEFAULT_SETTINGS: Settings = {
  micPassthrough: true,
  micSource: null,
  micVolume: 1,
  toMicVolume: 1,
  monitorEnabled: true,
  monitorVolume: 0.6,
  useAsDefaultMic: false,
  retrigger: "restart",
  stopAllHotkey: null,
  closeToTray: true,
  startMinimized: false,
};

/** Rendered before `get_state` answers (and if it fails). */
export const state: State = {
  sounds: [],
  settings: { ...DEFAULT_SETTINGS },
  router: { state: "starting", message: null },
  input: { state: "noKeyboards", keyboards: [] },
  playing: new Map(),
  query: "",
};

const listeners = new Set<Listener>();

/** Shallow-merges `partial` into the state, then notifies every subscriber. */
export function set(partial: Partial<State>): void {
  Object.assign(state, partial);
  for (const listener of [...listeners]) listener(state);
}

/**
 * Registers `fn`, calls it once with the current state, and returns an
 * unsubscribe function.
 */
export function subscribe(fn: Listener): () => void {
  listeners.add(fn);
  fn(state);
  return () => {
    listeners.delete(fn);
  };
}

/** The card with this id, or undefined when it was just deleted. */
export function soundById(id: string): Sound | undefined {
  return state.sounds.find((sound) => sound.id === id);
}
