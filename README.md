# SthDictate

A tiny macOS menu bar app for dictation against your own Whisper server.

- Tap the hotkey (Right ⌘ by default) to start, tap again to stop, or hold and release. Taps register instantly, even right after stopping. A sound plays on start and stop, and the icon turns into a red waveform while recording.
- The recording is sent to any OpenAI-compatible transcription endpoint (`/v1/audio/transcriptions`), and the text is pasted at your cursor as one paragraph.
- Microphone menu: follow the system input, or pin one device (say a USB interface while Bluetooth headphones stay in music mode; AirPods take about 3 seconds to switch into headset mode on every press). On a multi-channel interface, use the loudest channel or pick one input. Quiet recordings are raised before sending.
- Settings: endpoint, optional model name, hotkey, a Test button, and a Permissions section with a Grant button for each permission.

## Install

    brew install --cask seethruhead/tap/sth-dictate

Then open Settings from the menu bar icon and grant Microphone, Input Monitoring and Accessibility. Grant opens the right System Settings page with a small panel to drag the app from. A warning triangle in the menu bar means it can't hear the hotkey yet.

## Build

    ./scripts/bundle.sh 0.3.0      # builds build/SthDictate.app
    swift run CoreChecks           # runs the logic checks
    ./scripts/release.sh 0.3.0     # zips the app for a GitHub release

`bundle.sh` signs with a local "SthDictate Local" identity when its keychain exists, so macOS keeps the permissions across rebuilds; otherwise it signs ad hoc.
