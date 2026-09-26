# QA results

Evidence gathered while implementing the tickets. Rows marked **manual** need a human
(password prompts they must approve, listening to audio, a real game or a second distro).

Environment: Fedora 44, KDE Plasma 6 (Wayland session; XWayland used for screenshots),
PipeWire 1.6.8, SteelSeries Arctis 5 headset (mic plus "Game"/"Chat" outputs), a PCI analog
input, and three keyboards (`Gaming Keyboard` x2, `Logitech PRO X`). Key presses were driven
with a `/dev/uinput` virtual keyboard, so hotkeys were exercised through the real kernel
input path rather than simulated in the DOM.

## Per-ticket verification

| Ticket | Verification | Result |
|---|---|---|
| T01 | `npm run tauri dev` compiles and opens a window titled `SimplySoundboard` (`wmctrl -l` under XWayland); `cargo clippy -D warnings`, `npm run build` clean | pass |
| T02 | 10 unit tests: config round-trip, corrupt file → `config.json.bak`, missing settings keys defaulted, camelCase wire names, patch missing-vs-null | pass |
| T03 | 8 unit tests; live IPC import of a real `.ogg` + `.mp3`: files copied into `~/.local/share/simplysoundboard/sounds`, `durationMs` probed (1541 ms), `.txt` rejected, volume clamped, reorder persisted, delete removed the file | pass |
| T04 | Live: 6 modules created (`ssb_fx`, `ssb_mix`, `ssb_mic`, loopbacks A/B/C); `pw-play --target ssb_fx` recorded through `ssb_mic` at −25.8 dB mean; window close and Ctrl+C unload everything; `kill -9` leaves 6 stale modules that the next launch unloads before creating exactly one graph; labels read `SimplySoundboard_Mic` (pactl truncates every quoted form at the first space, so PLAN §9's underscore fallback is used) | pass |
| T05 | Live: restart keeps exactly 1 concurrent `pw-play`, overlap peaks at 2 then drains to 0, stop leaves 0, `stop_all` empties the list; event stream `S3.0 E4.9 S5.0 E5.5 S7.1 E7.4` (no `ended` during a restart, so the UI never flickers); no zombies; errors `Sound not found` / `Sound file missing: gone.wav`; a recording through `ssb_mic` is exactly 1.9 dB quieter at volume 0.8 | pass |
| T06 | Live: graph follows `monitorEnabled` (5↔4 modules), `micPassthrough` and `micSource` (loop C repointed to the PCI mic); volumes land on the right sink-inputs (loop A 150 %, B 0 %, C 50 % after clamping 9/−1/0.5); audible checks 100 % → −18.1 dB, to-mic 20 % → −59.9 dB, monitor 30 % → −49.4 dB, i.e. pactl's cubic percentage mapping; use-as-default takes over, is restored on quit, re-applied next launch; `list_mics` never lists `ssb_*` or monitors; the feedback guard refused to loop a default source that was itself an `ssb_*` device | pass |
| T07 | 24 unit tests (labels incl. the three evdev function-key blocks, modifier merging, exact-match matching, autorepeat, capture). Live with a uinput keyboard while **another window had focus**: Num 3 started playback (`pw-play` = 1, recorded at −18.1 dB through `ssb_mic`), Num 0 (stop-all) emptied it, a burst of 10 presses left exactly 1 process, and unplugging/replugging the keyboard was picked up again within 4 s (`Stopped reading … No such device` → `Reading keys from …`). A real scan before the udev rule reported `NoPermission keyboards=[] denied=[event3, event4, event8]`, matching the three root-only keyboards on this machine | pass |
| T08 | Unit tests for capture (Shift+Num 5 → binding, Backspace → clear, Esc → cancel, no action while capturing, stale 20 s timeouts ignored) plus live uinput captures: a real key produced a `key_captured` payload, Esc/Backspace worked with the window unfocused, and a conflict moved the key | pass |
| T09 | Unit tests (shipped rule content, embedded copy == packaged copy, exact pkexec argv, status transitions, wait-for-keyboard). Live: pressing *Enable hotkeys* in the UI ran the real pkexec/udev install and `/dev/input/event*` gained `user:<me>:rw-` ACLs — hotkeys worked immediately with no relogin (rule at `/etc/udev/rules.d/70-simplysoundboard-input.rules`). Cancelling/interrupting pkexec returns `Installing the keyboard rule was interrupted`, which the UI surfaces as a red toast | pass |
| T10 | Real app screenshots: empty state, grid with key chips/durations, playing glow + progress, indeterminate bar for unknown durations, search/no-results, picker import, drag & drop add and reject, ⋯ menu branch. Parent check: the window matches the prototype layout at 1000×680 | pass |
| T11 | Real clicks + uinput keys: right-click menu (Play/Edit…/Change key…/Remove key/Delete, viewport flipping at 720×480), delete confirmation before the file is removed, edit modal (emoji, colour, volume, Enter-to-save, Clear, Test), capture modal both branches, conflict warnings and moves (sound→sound and sound→stop-all), deferred conflict resolution on Save, reorder by drag persisting across restart | pass |
| T12 | Live: sliders show up as the right loopback sink-input volumes (52/34/39 %), pass-through and monitor toggles add/remove their modules, device picker repoints loopback C, use-as-default takes over and restores, everything persists across restart, a PATH without `pactl` produced the red pill plus `router.message` and *Recreate* recovered, keyboard-access line tracks live device changes, banner variants for `noPermission`/`noKeyboards` (state injected only for the screenshot, never committed) | pass |
| T13 | Live: tray registered with a menu containing *Show SimplySoundboard* / *Stop all sounds* / separator / *Quit* (read back over `com.canonical.dbusmenu`); close-to-tray hides and keeps the audio graph; a second launch exits 0 and focuses the running instance; clicking *Quit* over DBus exits with 0 `ssb_` modules and the default input restored; `startMinimized` shows no window; `closeToTray: false` quits on close. Platform note: left-click-to-toggle cannot work on Linux because Tauri hardcodes the AppIndicator tray backend, which never delivers mouse events, so the *Show* menu item is the way back | pass (note) |
| T14 | `npm run tauri build` produces an AppImage (113 MB) plus .deb and .rpm (3.5 MB). The `.deb` control has the right `Depends`, long description and a `postinst`; its data contains `/usr/lib/udev/rules.d/70-simplysoundboard-input.rules`, the binary, the `.desktop` (`Categories=AudioVideo;Audio;Music;`) and hicolor icons. The `.rpm` requires the three PipeWire packages, ships the same rule and carries the postinst as `%post`. The AppImage runs here and immediately read `/dev/input/event3`+`event8`, then unloaded everything on Ctrl+C | pass (install = manual) |
| T15 | Packaged-build smoke with the final AppImage, driven by a uinput keyboard: the app started, read `/dev/input/event22`+`event3`+`event8`, showed its window, played the sound on its Num 3 hotkey (`pw-play` = 1, then 0 when it ended), stopped it with the Num 0 hotkey, and `SIGTERM` shut it down logging `Shutting down` with 0 `ssb_` modules left and the default input untouched. Caveat seen in the AppImage log: `GStreamer element appsink not found` (WebKit warning about the missing media plugins in an AppImage; irrelevant here because playback goes through `pw-play`) | pass |

## Still needs a human

Automation covered the Linux desktop behaviour, audio graph and the keyboard path. What is left
genuinely needs another machine, other software or a person:

**Audio**
- [ ] Discord/other-voice-app end-to-end: *SimplySoundboard Mic* appears in the app's input list and
      others hear sounds (plus voice with pass-through on, only sounds with it off).
- [ ] Listen on real headphones: monitor on/off, *To mic* vs *Me* independence by ear.
- [ ] Switching mic device while a call is running.
- [ ] Plugging in headphones while running → the monitor follows the new default output
      (loopback B is created without `sink=`, so PipeWire moves it; only its module-level
      attachment was verified here).

**Hotkeys**
- [ ] Inside a fullscreen game, native and Proton/XWayland (this needs a game).
- [ ] Keys that the game also binds (the key still reaches the game — never grabbed).
- [ ] Anti-cheat tolerance.

**Other desktops / distros**
- [ ] Fedora GNOME and an X11 session (tray absent on plain GNOME → the app must behave like a
      normal window; that fallback path is code-verified only).
- [ ] `.rpm` install on Fedora and `.deb` install on Ubuntu 24.04 (both need root).
- [ ] Uninstall removes the udev rule (it is a tracked package file, so `dpkg`/`rpm` should; only
      the file list was verified here).

## Notes for the next run

- Installing the udev rule (through the app's own *Enable hotkeys*) is a machine-wide change; it is
  live on the development machine. Remove with
  `sudo rm /etc/udev/rules.d/70-simplysoundboard-input.rules && sudo udevadm control --reload && sudo udevadm trigger --subsystem-match=input --action=change`.
- Tauri's `bundle.category` is its own enum and rejects `AudioVideo`; `Music` is used while the
  desktop file still carries `Categories=AudioVideo;Audio;Music;`.
- `linux.deb/rpm.files` maps **destination → source** (`tauri-bundler` copies the value to the key).
