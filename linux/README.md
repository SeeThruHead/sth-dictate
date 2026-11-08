# SthDictate for Linux

The Linux version of SthDictate: a small Rust service for dictation against your own Whisper server.

- Tap the hotkey to start and tap again to stop, or hold it and let go. A sound plays on start and stop.
- The recording goes to any OpenAI-compatible `/v1/audio/transcriptions` endpoint. The text arrives at your cursor as one paragraph, pasted or typed, as the `delivery` setting says. Pasting copies with `wl-copy`, then presses the app's paste keys on a virtual kernel keyboard (`/dev/uinput`), so they take the same path as your own keyboard and work in Wine games too. Quiet recordings are raised before sending, and recordings under 0.3 s are not sent.
- Several recordings can be transcribing at once; they paste in the order you spoke them.
- The service reads the hotkey straight from the kernel's input devices, so it needs no compositor binding and keeps working in fullscreen games. It rescans every 2 s, so a keyboard (or Toshy's virtual keyboard) that appears later is picked up.
- Pinned to Herdr (`sth-dictate pin`), the text goes to the pane focused inside Herdr through `herdr pane send-text` instead of the cursor, so it reaches the agent you are looking at while a game keeps focus. The pin is kept in `~/.local/state/sth-dictate/pinned` across restarts.
- State goes out on a Unix socket (`$XDG_RUNTIME_DIR/sth-dictate.sock`) as one JSON line per change: `{"state":"idle|recording|transcribing|unavailable","message":"...","pinned":false}`. A client may write `pin on`, `pin off` or `pin toggle` to it. `sth-dictate watch` prints it and reconnects when the service restarts; the Omarchy bar module runs that.

## Commands

    sth-dictate serve     run the service (the systemd user unit does this)
    sth-dictate watch     print the state as it changes
    sth-dictate status    print the state once
    sth-dictate test      send a second of silence to the endpoint and say how long it took
    sth-dictate pin [on|off|toggle]
                          send the text to the focused Herdr pane instead of the cursor

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

`hotkey` is a Linux key code (188 is F18). On STHDEV, keyd turns Right ⌘ into F18 (`rightmeta = f18`), so Right ⌘ never acts as a modifier. `microphone` is a PipeWire node name or serial passed to `pw-record --target`; leave it out to follow the default input. `sounds` holds `Tink.wav` and `Pop.wav`. `delivery` is `paste` (copy to the clipboard, then press the paste keys) or `type` (send the text as keystrokes through `wtype`, which leaves the clipboard alone but does not work in Wine games such as WoW, where the keys land as raw keybindings). `paste_keys` says which keys paste in which app, by Hyprland window class, with `default` for every other app; built in is only `default: shift+Insert`. A combo is modifiers and a key joined by `+` (`ctrl+v`, `ctrl+shift+v`), checked when the service starts.

## Needs

`pw-record` and `pw-play` (PipeWire), `wl-copy` (wl-clipboard), `hyprctl`, `herdr` when pinned, `wtype` for typing mode, read access to `/dev/input` (the `input` group) and write access to `/dev/uinput`.

## Install

    brew install seethruhead/tap/sth-dictate
    systemctl --user link /home/linuxbrew/.linuxbrew/opt/sth-dictate/share/systemd/user/sth-dictate.service
    systemctl --user enable --now sth-dictate

The formula builds from a `linux-v<version>` tag. To release one, bump `version` in `Cargo.toml`, tag `linux-v<version>`, push the tag, and point `Formula/sth-dictate.rb` in the tap at it with the new tarball's sha256.

## Build and test

    cargo test
    cargo clippy --all-targets
