# SthDictate for Linux

The Linux version of SthDictate: a small Rust service for dictation against your own Whisper server.

- Tap the hotkey to start and tap again to stop, or hold it and let go. A sound plays on start and stop.
- Double tap to stream: it keeps listening, and every pause sends what you said, then presses Enter. Double tap again to stop. A pause is `pause_seconds` of quiet (1 s by default); speech is anything louder than `voice_level` (150 on the 16-bit scale) and three times the room's own noise, which it keeps learning. Whisper's "Thank you." heard in silence is dropped.
- The recording goes to any OpenAI-compatible `/v1/audio/transcriptions` endpoint. The text arrives at your cursor as one paragraph, pasted or typed, as the `delivery` setting says. Pasting copies with `wl-copy`, then presses the app's paste keys on a virtual kernel keyboard (`/dev/uinput`), so they take the same path as your own keyboard and work in Wine games too. Quiet recordings are raised before sending, and recordings under 0.3 s are not sent.
- Several recordings can be transcribing at once; they paste in the order you spoke them.
- The service reads the hotkey straight from the kernel's input devices, so it needs no compositor binding and keeps working in fullscreen games. It rescans every 2 s, so a keyboard (or Toshy's virtual keyboard) that appears later is picked up.
- Pin an app and dictation goes into it, then Enter, while you stay in another one (a game). Pin from the tray: the service shows a tray icon whose menu lists the open windows to pin to, Herdr, and Unpin, with the current state on top. `sth-dictate pin` pins the window in front from a script. A Herdr window (Herdr in kitty, or the Herdr app) gets the text in its focused pane through Herdr itself (`herdr agent prompt` or `send-text`, `hc send` for the app), so no window focus is touched. Any other app gets the clipboard and its paste keys plus Return through Hyprland's `send_shortcut` aimed at that window: Hyprland hands that window the keyboard for those two keys and gives it straight back, without switching windows. An X11 window is never typed into while another X11 app (a Wine game) is in front, since Hyprland would send the keys to the game. `pin on` with nothing pinned yet means Herdr: the focused pane of whichever Herdr window was used last. The pin is kept in `~/.local/state/sth-dictate/pin.json` across restarts.
- State goes out on a Unix socket (`$XDG_RUNTIME_DIR/sth-dictate.sock`) as one JSON line per change: `{"state":"idle|recording|transcribing|streaming|unavailable","message":"...","pinned":false,"pinned_to":""}`. A client may write `pin on`, `pin off`, `pin toggle` or `pin to <target json>` to it. `sth-dictate watch` prints it and reconnects when the service restarts; the Omarchy bar module runs that.

## Commands

    sth-dictate serve     run the service (the systemd user unit does this)
    sth-dictate watch     print the state as it changes
    sth-dictate status    print the state once
    sth-dictate test      send a second of silence to the endpoint and say how long it took
    sth-dictate pin       pin the window in front: dictation goes into it, then Enter
    sth-dictate pin herdr pin Herdr: the focused pane of the Herdr used last
    sth-dictate pin off   stop pinning
    sth-dictate pin on    pin again, to the last pinned window or Herdr

## Settings

`~/.config/sth-dictate/settings.json`, every field optional:

    {
      "endpoint": "http://192.168.0.149:8000/v1",
      "model": "turbo",
      "hotkey": 188,
      "hotkey_label": "Right ⌘",
      "microphone": "alsa_input.usb-...",
      "sounds": "/home/sth/.local/share/sounds/sth-dictate",
      "delivery": "paste",
      "paste_keys": { "default": "shift+Insert", "wow.exe": "ctrl+v" }
    }

`hotkey` is a Linux key code (188 is F18). On STHDEV, keyd turns Right ⌘ into F18 (`rightmeta = f18`), so Right ⌘ never acts as a modifier. `microphone` is a PipeWire node name or serial passed to `pw-record --target`; leave it out to follow the default input. `sounds` holds `Tink.wav` and `Pop.wav`. `delivery` is `paste` (copy to the clipboard, then press the paste keys) or `type` (send the text as keystrokes through `wtype`, which leaves the clipboard alone but does not work in Wine games such as WoW, where the keys land as raw keybindings). `paste_keys` says which keys paste in which app (at the cursor and in a pinned window), by Hyprland window class, with `default` for every other app; built in is only `default: shift+Insert`. A combo is modifiers and a key joined by `+` (`ctrl+v`, `ctrl+shift+v`), checked when the service starts.

## Needs

`pw-record` and `pw-play` (PipeWire), `wl-copy` (wl-clipboard), `hyprctl`, `herdr`, or `herdr-app` and `hc` (looked for in `~/.local/bin`), when pinned to Herdr, `wtype` for typing mode, read access to `/dev/input` (the `input` group) and write access to `/dev/uinput`.

## Install

    brew install seethruhead/tap/sth-dictate
    systemctl --user link /home/linuxbrew/.linuxbrew/opt/sth-dictate/share/systemd/user/sth-dictate.service
    systemctl --user enable --now sth-dictate

The formula builds from a `linux-v<version>` tag. To release one, bump `version` in `Cargo.toml`, tag `linux-v<version>`, push the tag, and point `Formula/sth-dictate.rb` in the tap at it with the new tarball's sha256.

## Build and test

    cargo test --workspace
    cargo clippy --workspace --all-targets

The `voice` crate (sth-voice) holds what any voice tool here needs: recording through PipeWire, pause detection, the Whisper client, reading keys from the kernel's input devices, and a virtual keyboard that types US-layout text. sth-wow-chat (its own repo) uses it as a git dependency on this repo.
