# QA results

Automated evidence gathered while implementing the tickets. Rows marked **manual**
need a human (password prompts, listening to audio, a real game).

Environment used: Fedora 44, KDE Plasma 6 (Wayland session, XWayland for screenshots),
PipeWire 1.6.8 with a SteelSeries Arctis 5 headset (mic + "Game"/"Chat" outputs) and a
PCI analog input, three keyboards (`Gaming Keyboard` x2, `Logitech PRO X`).

## Automated verification per ticket

| Ticket | Verification | Result |
|---|---|---|
| T01 | `npm run tauri dev` builds and opens a window titled `SimplySoundboard` (`wmctrl -l` under XWayland); `cargo clippy -D warnings`, `npm run build` | pass |
| T02 | 10 unit tests; config round-trip, corrupt file → `config.json.bak`, missing settings keys defaulted, camelCase field names, patch missing-vs-null | pass |
| T03 | 8 unit tests; live IPC run importing a real `.ogg` + `.mp3`: files copied into `~/.local/share/simplysoundboard/sounds`, `durationMs` probed (1541 ms), `.txt` rejected, volume clamped to 1.0, reorder persisted, delete removed the file | pass |
| T04 | Live: 6 modules created (`ssb_fx`, `ssb_mix`, `ssb_mic`, loopbacks A/B/C); `pw-play --target ssb_fx` recorded through `ssb_mic` at −25.8 dB mean; window close and Ctrl+C unload everything; `kill -9` leaves 6 stale modules that the next launch unloads before creating exactly one graph; descriptions now read `SimplySoundboard_Mic` (pactl truncates any quoted form at the first space, so PLAN.md §9's underscore fallback is used) | pass |
| T05 | Live: restart keeps exactly 1 concurrent `pw-play`, overlap peaks at 2 then drains to 0, stop leaves 0, `stop_all` empties the list; event stream `S3.0 E4.9 S5.0 E5.5 S7.1 E7.4` (no `ended` during a restart); no zombies; `Sound not found` / `Sound file missing: gone.wav`; recording level differs by exactly 1.9 dB at volume 0.8 | pass |
| T06 | Live: graph follows `monitorEnabled` (5↔4 modules), `micPassthrough` and `micSource` (loop C repointed to the PCI mic); volumes land on the right sink-inputs (loop A 150 %, B 0 %, C 50 % after clamping 9/−1/0.5); audible checks 100 % → −18.1 dB, to-mic 20 % → −59.9 dB, monitor 30 % → −49.4 dB (pactl's cubic mapping); use-as-default takes over, is restored on quit, re-applied next launch; `list_mics` never lists `ssb_*`/monitors | pass |
| T07 | 24 unit tests for labels/matching/capture; a test performs a real `/dev/input` scan: `NoPermission keyboards=[] denied=[event3, event4, event8]` — the three keyboards on this machine, which are root-only | pass |
| T08 | Unit tests for capture (Shift+Num 5 → binding, Backspace → clear, Esc → cancel, no action while capturing, stale timeouts ignored) | pass |
| T09 | 5 unit tests (shipped rule content, embedded copy == packaged copy, exact pkexec argv, status transitions, wait-for-keyboard). The polkit dialog itself is **manual** | partial |
| T10 | | pending |
| T11 | | pending |
| T12 | | pending |
| T13 | Live: tray registered on the StatusNotifierWatcher (Ayatana path, id `tray-icon tray app simplysoundboard`) with a menu containing *Show SimplySoundboard* / *Stop all sounds* / separator / *Quit* (read back over `com.canonical.dbusmenu`); close-to-tray hides the window and keeps the audio graph up (`alive_after_close`, `window_hidden_after_close`, 4 modules); a second launch exits 0 and focuses the running instance (1 process, window shown again); clicking the tray's *Quit* item over DBus exits and leaves 0 `ssb_` modules with the default input restored; `startMinimized` shows no window while the app runs, Ctrl+C then unloads everything; close with `closeToTray: false` quits and unloads. Platform note: left-click-to-toggle cannot work on Linux — Tauri hardcodes the AppIndicator backend, which never delivers mouse events, so the window is reached through the menu's *Show* item | pass (with note) |
| T14 | `npm run tauri build` produces AppImage (113 MB), .deb and .rpm (3.5 MB each). `.deb` control: `Package: simply-soundboard`, `Depends: pipewire-pulse, pulseaudio-utils, pipewire-bin, libappindicator3-1, libwebkit2gtk-4.1-0, libgtk-3-0`, long description, and a `postinst`; data contains `/usr/lib/udev/rules.d/70-simplysoundboard-input.rules`, the binary, the `.desktop` (`Categories=AudioVideo;Audio;Music;`) and hicolor icons. `.rpm` requires the three PipeWire packages, ships the same udev rule and carries the postinst as its `%post`. The AppImage runs on this machine out of the box: it read keyboards from `/dev/input/event3`+`event8` immediately and built its 6-module audio graph, and Ctrl+C unloaded everything. Parent note: Tauri's `bundle.category` is its own enum (AudioVideo is not a member), so it is `Music`; the desktop file still gets the AudioVideo category. Installing the packages needs root, so that step is in the manual list | pass (install = manual) |
| T15 | this document | pending |

## Manual checklist (needs a human)

Matrix: Fedora GNOME (Wayland), Kubuntu/KDE (Wayland), an X11 session.

**Audio**

- [ ] "SimplySoundboard Mic" visible in system sound settings and Discord's input list.
- [ ] Discord voice test hears sounds + voice (pass-through on) and only sounds (off).
- [ ] Monitor off → you don't hear sounds, others still do.
- [ ] To-mic and Me sliders are independent.
- [ ] Mic device switch while a call is running.
- [ ] Use-as-default mic restored after Quit, and after `kill` + relaunch + Quit.
- [ ] Headphones plugged in while running → monitor follows the new default output.

**Hotkeys**

- [ ] Click *Enable hotkeys* once (polkit password) on a machine without the packaged rule —
      this is the only step that cannot be automated.
- [ ] Hotkeys work with the app focused, unfocused and minimized to the tray.
- [ ] Hotkeys work in a fullscreen game (native + Proton/XWayland).
- [ ] Combos (Ctrl+F9), numpad keys, F13+.
- [ ] Holding a key does not retrigger.
- [ ] USB keyboard replug keeps hotkeys.
- [ ] Stop-all hotkey.

**Library/UI**

- [ ] Import wav/ogg/flac/mp3/opus via the picker and drag & drop; `.txt` rejected with a toast.
- [ ] Edit name/emoji/color/volume persists; reorder persists; delete removes the file.
- [ ] Key conflict moves the key.
- [ ] Retrigger modes behave as described.
- [ ] Router error state + *Recreate*.
- [ ] Window at the 720×480 minimum is still usable; long names ellipsize.

**Lifecycle**

- [ ] Single instance; close-to-tray; Quit leaves no `ssb_` modules and no `pw-play`.
- [ ] Crash (`kill -9`) → relaunch works, no duplicate devices.

**Packaging**

- [ ] `.rpm` installs on Fedora and `.deb` on Ubuntu 24.04; hotkeys work immediately without the
      *Enable hotkeys* prompt (the udev rule is shipped).
- [ ] AppImage runs on both and the *Enable hotkeys* path works.
- [ ] Uninstall removes the udev rule.
