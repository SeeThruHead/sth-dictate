# SthDictate for Linux

The Linux version of SthDictate: a small Rust service for dictation against your own Whisper server.

- Tap the hotkey to start and tap again to stop, or hold it and let go. A sound plays on start and stop.
- The recording goes to any OpenAI-compatible `/v1/audio/transcriptions` endpoint. The text is pasted at your cursor as one paragraph (copied with `wl-copy`, then Shift+Insert through `wtype`). Quiet recordings are raised before sending, and recordings under 0.3 s are not sent.
- Several recordings can be transcribing at once; they paste in the order you spoke them.
- The service reads the hotkey straight from the kernel's input devices, so it needs no compositor binding and keeps working in fullscreen games. It rescans every 2 s, so a keyboard (or Toshy's virtual keyboard) that appears later is picked up.
- State goes out on a Unix socket (`$XDG_RUNTIME_DIR/sth-dictate.sock`) as one JSON line per change: `{"state":"idle|recording|transcribing|unavailable","message":"..."}`. `sth-dictate watch` prints it and reconnects when the service restarts; the Omarchy bar module runs that.

## Commands

    sth-dictate serve     run the service (the systemd user unit does this)
    sth-dictate watch     print the state as it changes
    sth-dictate status    print the state once
    sth-dictate test      send a second of silence to the endpoint and say how long it took

## Settings

`~/.config/sth-dictate/settings.json`, every field optional:

    {
      "endpoint": "http://192.168.0.149:8000/v1",
      "model": "turbo",
      "hotkey": 188,
      "hotkey_label": "Right ⌘",
      "microphone": "alsa_input.usb-...",
      "sounds": "/home/sth/.local/share/sounds/sth-dictate"
    }

`hotkey` is a Linux key code (188 is F18). On STHDEV, keyd turns Right ⌘ into F18 (`rightmeta = f18`), so Right ⌘ never acts as a modifier. `microphone` is a PipeWire node name or serial passed to `pw-record --target`; leave it out to follow the default input. `sounds` holds `Tink.wav` and `Pop.wav`.

## Needs

`pw-record` and `pw-play` (PipeWire), `wl-copy` (wl-clipboard), `wtype`, and membership of the `input` group to read `/dev/input`.

## Install

    cargo install --path linux --root ~/.local
    cp linux/systemd/sth-dictate.service ~/.config/systemd/user/
    systemctl --user daemon-reload
    systemctl --user enable --now sth-dictate

## Build and test

    cargo test
    cargo clippy --all-targets
