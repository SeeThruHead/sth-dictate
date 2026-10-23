import DictateCore
import Foundation

struct Check {
    let name: String
    let passed: Bool
}

func check(_ name: String, _ condition: @autoclosure () -> Bool) -> Check {
    Check(name: name, passed: condition())
}

let rightCmd = Hotkey.rightCommand
let optionSpace = Hotkey(keyCode: 49, modifiers: ModifierFlag.option)
let down: KeyEvent = .flagsChanged(keyCode: 54, flags: ModifierFlag.command | 0x10)
let up: KeyEvent = .flagsChanged(keyCode: 54, flags: 0)

func signals(_ events: [KeyEvent], hotkey: Hotkey) -> [HotkeySignal] {
    events.reduce((HotkeyTracker(), [HotkeySignal]())) { acc, event in
        let (tracker, signal) = acc.0.step(event, hotkey: hotkey)
        return (tracker, acc.1 + (signal.map { [$0] } ?? []))
    }.1
}

func play(_ actions: [Action], from start: Session = Session()) -> (Session, [Effect]) {
    actions.reduce((start, [Effect]())) { acc, action in
        let (next, effects) = reduce(acc.0, action)
        return (next, acc.1 + effects)
    }
}

func tap(at t: Double) -> [Action] {
    [.hotkey(.pressed, at: t), .hotkey(.released, at: t + 0.1)]
}

let toggle = play(tap(at: 0) + tap(at: 5))
let hold = play([.hotkey(.pressed, at: 0), .hotkey(.released, at: 2)])
let shortcutMidDictation = play(tap(at: 0) + [.hotkey(.pressed, at: 3), .hotkey(.interrupted, at: 3.1)])
let shortcutAtStart = play([.hotkey(.pressed, at: 0), .hotkey(.interrupted, at: 0.1)])
let restartWhileTranscribing = play(tap(at: 0) + tap(at: 5) + tap(at: 5.3))
let outOfOrder = play(tap(at: 0) + tap(at: 2) + tap(at: 3) + tap(at: 4) + [
    .transcribed(id: 1, text: " second "), .transcribed(id: 0, text: "first"),
])
let failureKeepsOrder = play(tap(at: 0) + tap(at: 2) + tap(at: 3) + tap(at: 4) + [
    .transcribed(id: 1, text: "second"), .transcriptionFailed(id: 0, message: "down"),
])

func taps(_ times: [Double], width: Double = 0.01) -> [Action] {
    times.flatMap { [Action.hotkey(.pressed, at: $0), .hotkey(.released, at: $0 + width)] }
}

func kinds(_ effects: [Effect]) -> [String] {
    effects.map {
        switch $0 {
        case let .startCapture(id): "start\(id)"
        case let .stopCaptureAndTranscribe(id): "stop\(id)"
        case let .discardCapture(id): "discard\(id)"
        case let .paste(text): "paste:\(text)"
        case let .report(message): "report:\(message)"
        }
    }
}

let burst = play(taps([0, 0.03, 0.06, 0.09, 0.12, 0.15]))
let stopThenInstantStart = play(taps([0, 3, 3.02]))
let instantTap = play(taps([0, 2], width: 0.001))
let releaseAndPressSameInstant = play([.hotkey(.pressed, at: 0), .hotkey(.released, at: 0.05), .hotkey(.pressed, at: 2), .hotkey(.released, at: 2.0), .hotkey(.pressed, at: 2.0), .hotkey(.released, at: 2.0)])
let lateResultsAfterBurst = play(taps([0, 0.03, 0.06, 0.09, 0.12, 0.15]) + [
    .transcribed(id: 2, text: "three"), .transcribed(id: 1, text: "two"), .transcribed(id: 0, text: "one"),
])
let doubleDown = signals([down, down, up, up], hotkey: rightCmd)
let strayUp = signals([up, down, up], hotkey: rightCmd)
let fastFlutter = signals(Array(repeating: [down, up], count: 20).flatMap { $0 }, hotkey: rightCmd)

let multipartText = String(decoding: multipart(boundary: "B", audio: Data("WAV".utf8), model: "turbo").body, as: UTF8.self)
let noModelText = String(decoding: multipart(boundary: "B", audio: Data("WAV".utf8), model: "").body, as: UTF8.self)
let header = wav([0, 1, -1])

let checks: [Check] = [
    check("right ⌘ press and release", signals([down, up], hotkey: rightCmd) == [.pressed, .released]),
    check("left ⌘ is not right ⌘", signals([.flagsChanged(keyCode: 55, flags: ModifierFlag.command | 0x08), .flagsChanged(keyCode: 55, flags: 0)], hotkey: rightCmd).isEmpty),
    check("right ⌘ used in a shortcut is an interruption", signals([down, .keyDown(keyCode: 8, flags: ModifierFlag.command | 0x10, isRepeat: false), up], hotkey: rightCmd) == [.pressed, .interrupted]),
    check("combo needs exact modifiers, ignores repeats", signals([.keyDown(keyCode: 49, flags: 0, isRepeat: false), .keyDown(keyCode: 49, flags: ModifierFlag.option, isRepeat: false), .keyDown(keyCode: 49, flags: ModifierFlag.option, isRepeat: true), .keyUp(keyCode: 49, flags: ModifierFlag.option)], hotkey: optionSpace) == [.pressed, .released]),
    check("tap starts, second tap stops", toggle.1 == [.startCapture(id: 0), .stopCaptureAndTranscribe(id: 0)] && !toggle.0.isRecording),
    check("holding then releasing stops", hold.1 == [.startCapture(id: 0), .stopCaptureAndTranscribe(id: 0)]),
    check("using ⌘ as a shortcut mid-dictation keeps recording", shortcutMidDictation.1 == [.startCapture(id: 0)] && shortcutMidDictation.0.isRecording),
    check("using ⌘ as a shortcut on the first press discards", shortcutAtStart.1 == [.startCapture(id: 0), .discardCapture(id: 0)] && !shortcutAtStart.0.isRecording),
    check("a new tap right after stopping starts a new recording", restartWhileTranscribing.1 == [.startCapture(id: 0), .stopCaptureAndTranscribe(id: 0), .startCapture(id: 1)] && restartWhileTranscribing.0.isTranscribing),
    check("results paste in the order spoken, trimmed", outOfOrder.1.filter { if case .paste = $0 { true } else { false } } == [.paste("first"), .paste("second")]),
    check("a failure is reported and later results still paste", failureKeepsOrder.1.suffix(2) == [.report("down"), .paste("second")]),
    check("empty transcripts paste nothing", reduce(Session(nextId: 1), .transcribed(id: 0, text: "  ")).1.isEmpty),
    check("wav header is 16 kHz mono PCM", header.count == 50 && String(data: header.prefix(4), encoding: .ascii) == "RIFF" && Array(header[24..<28]) == [0x80, 0x3e, 0, 0] && Array(header[44..<50]) == [0, 0, 1, 0, 0xff, 0xff]),
    check("pcm clamps and scales", pcm16([2, -2, 0]) == [Int16.max, -Int16.max, 0]),
    check("endpoint gets the transcription path once", (try? transcriptionURL("http://h:8000/v1/").get().absoluteString) == "http://h:8000/v1/audio/transcriptions" && (try? transcriptionURL("http://h:8000/v1/audio/transcriptions").get().absoluteString) == "http://h:8000/v1/audio/transcriptions"),
    check("garbage endpoint is rejected", (try? transcriptionURL("not a url").get()) == nil),
    check("multipart carries audio and optional model", multipartText.contains("name=\"model\"\r\n\r\nturbo") && multipartText.contains("filename=\"audio.wav\"\r\nContent-Type: audio/wav\r\n\r\nWAV\r\n") && multipartText.hasSuffix("--B--\r\n") && !noModelText.contains("name=\"model\"")),
    check("transcript needs success and a text field", (try? transcriptText(status: 200, body: Data(#"{"text":"hi"}"#.utf8)).get()) == "hi" && (try? transcriptText(status: 500, body: Data("boom".utf8)).get()) == nil && (try? transcriptText(status: 200, body: Data("{}".utf8)).get()) == nil),
    check("six taps 30 ms apart give three separate recordings", kinds(burst.1) == ["start0", "stop0", "start1", "stop1", "start2", "stop2"] && !burst.0.isRecording),
    check("a tap 20 ms after stopping starts a new recording", kinds(stopThenInstantStart.1) == ["start0", "stop0", "start1"] && stopThenInstantStart.0.isRecording),
    check("a 1 ms tap still counts", kinds(instantTap.1) == ["start0", "stop0"]),
    check("release and next press in the same instant both register", kinds(releaseAndPressSameInstant.1) == ["start0", "stop0", "start1"]),
    check("results from a fast burst paste in spoken order", kinds(lateResultsAfterBurst.1).filter { $0.hasPrefix("paste") } == ["paste:one", "paste:two", "paste:three"]),
    check("a repeated down without an up triggers once", doubleDown == [.pressed, .released]),
    check("a stray up before any down is ignored", strayUp == [.pressed, .released]),
    check("20 fast taps give 20 press and release pairs", fastFlutter.count == 40 && fastFlutter.enumerated().allSatisfy { $0.element == ($0.offset % 2 == 0 ? .pressed : .released) }),
    check("hotkey names read naturally", hotkeyName(rightCmd, keyName: { _ in "?" }) == "Right ⌘" && hotkeyName(optionSpace, keyName: { _ in "Space" }) == "⌥Space"),
]

checks.forEach { print("\($0.passed ? "ok  " : "FAIL") \($0.name)") }
let failed = checks.filter { !$0.passed }.count
print(failed == 0 ? "\(checks.count) checks passed" : "\(failed) of \(checks.count) checks failed")
exit(failed == 0 ? 0 : 1)
