# SthDictate

A tiny macOS menu bar app for dictation against your own Whisper server.

- Tap the hotkey (Right ⌘ by default) to start, tap again to stop, or hold and release. Taps register instantly, even right after stopping. A sound plays on start and stop, and the icon turns into a red waveform while recording.
- The recording is sent to any OpenAI-compatible transcription endpoint (`/v1/audio/transcriptions`), and the text is pasted at your cursor as one paragraph.
- Microphone menu: follow the system input, or pin one device (say a USB interface while Bluetooth headphones stay in music mode; AirPods take about 3 seconds to switch into headset mode on every press). On a multi-channel interface, use the loudest channel or pick one input. Quiet recordings are raised before sending.
- Settings: endpoint, optional model name, hotkey, a Test button, and a Permissions section with a Grant button for each permission.
- Double tap the hotkey to stream: it keeps listening, and every pause sends what you said, then presses Return. Double tap again to stop. The menu bar icon turns blue while streaming and red while it hears you.
- Pin (menu): pin the app in front, or Herdr, and dictation goes into it followed by Return while you stay in another app. A Herdr terminal or the Herdr app gets the text in its focused pane through Herdr itself (`herdr agent prompt`, `hc send`). Any other app gets ⌘V and Return posted straight to its process, without bringing it forward; a few apps ignore keys sent that way. Unpinned, it pastes at the cursor as before.

## Install

    brew install --cask seethruhead/tap/sth-dictate

Then open Settings from the menu bar icon and grant Microphone, Input Monitoring and Accessibility. Grant opens the right System Settings page with a small panel to drag the app from. A warning triangle in the menu bar means it can't hear the hotkey yet.

## Build

    ./scripts/bundle.sh 0.3.0      # builds build/SthDictate.app
    swift run CoreChecks           # runs the logic checks
    ./scripts/release.sh 0.3.0     # zips the app for a GitHub release

`bundle.sh` signs with a local "SthDictate Local" identity when its keychain exists, so macOS keeps the permissions across rebuilds; otherwise it signs ad hoc.
