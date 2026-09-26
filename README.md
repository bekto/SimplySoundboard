# SimplySoundboard

A small, friendly **Linux-only** soundboard. Sounds live on cards; each card can have a global
hotkey that works **inside games**. Sounds are mixed into a **virtual microphone** so teammates on
Discord / in-game voice hear them.

- Global hotkeys via `/dev/input` (evdev, read-only) — works on X11, every Wayland compositor and
  fullscreen games.
- Virtual mic **"SimplySoundboard Mic"** = soundboard sounds + optional pass-through of your real mic.
- Optional monitor so you hear the sounds yourself.

## Dev prerequisites

Fedora:

```sh
sudo dnf install webkit2gtk4.1-devel openssl-devel curl wget file libappindicator-gtk3-devel librsvg2-devel pipewire-utils pulseaudio-utils
sudo dnf group install c-development
```

Ubuntu/Debian:

```sh
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file libssl-dev libayatana-appindicator3-dev librsvg2-dev pipewire-bin pulseaudio-utils
```

Arch:

```sh
sudo pacman -S webkit2gtk-4.1 base-devel curl wget file openssl libappindicator-gtk3 librsvg pipewire libpulse
```

## Run

```sh
npm install
npm run tauri dev
```

## Checks

```sh
cd src-tauri && cargo clippy -- -D warnings && cargo test
npm run build
```

## License

MIT
