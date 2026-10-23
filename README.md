# SthDictate

A tiny macOS menu bar app for dictation against your own Whisper server.

- Tap the hotkey (Right ⌘ by default) to start, tap again to stop, or hold and release. Taps register instantly, even right after stopping.
- The recording is sent to any OpenAI-compatible transcription endpoint (`/v1/audio/transcriptions`), and the text is pasted at your cursor.
- Settings: endpoint, optional model name, hotkey, and a Test button.
- Copy Last Transcript in the menu if a paste lands in the wrong place.

Needs Microphone, Input Monitoring and Accessibility permission on first run.

## Build

    ./scripts/bundle.sh 0.1.0      # builds build/SthDictate.app
    swift run CoreChecks           # runs the logic checks
