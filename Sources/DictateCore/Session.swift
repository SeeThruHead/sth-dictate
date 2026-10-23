public enum KeyPhase: Equatable, Sendable {
    case heldSinceStart(at: Double)
    case toggled
    case stopPressDown
}

public struct Session: Equatable, Sendable {
    public let recording: (id: Int, key: KeyPhase)?
    public let nextId: Int
    public let nextToPaste: Int
    public let finished: [Int: String]

    public init(recording: (id: Int, key: KeyPhase)? = nil, nextId: Int = 0, nextToPaste: Int = 0, finished: [Int: String] = [:]) {
        self.recording = recording
        self.nextId = nextId
        self.nextToPaste = nextToPaste
        self.finished = finished
    }

    public static func == (lhs: Session, rhs: Session) -> Bool {
        lhs.recording?.id == rhs.recording?.id && lhs.recording?.key == rhs.recording?.key && lhs.nextId == rhs.nextId
            && lhs.nextToPaste == rhs.nextToPaste && lhs.finished == rhs.finished
    }

    public var isRecording: Bool { recording != nil }
    public var isTranscribing: Bool { nextToPaste < nextId - (recording == nil ? 0 : 1) }

    func with(recording: (id: Int, key: KeyPhase)?) -> Session {
        Session(recording: recording, nextId: nextId, nextToPaste: nextToPaste, finished: finished)
    }
}

public enum Action: Equatable, Sendable {
    case hotkey(HotkeySignal, at: Double)
    case transcribed(id: Int, text: String)
    case transcriptionFailed(id: Int, message: String)
}

public enum Effect: Equatable, Sendable {
    case startCapture(id: Int)
    case stopCaptureAndTranscribe(id: Int)
    case discardCapture(id: Int)
    case paste(String)
    case report(String)
}

public let holdThresholdSeconds = 0.4

public func reduce(_ session: Session, _ action: Action) -> (Session, [Effect]) {
    switch action {
    case let .hotkey(signal, at):
        return session.recording.map { onKey(session, recording: $0, signal: signal, at: at) } ?? onIdleKey(session, signal: signal, at: at)
    case let .transcribed(id, text):
        return flush(session, id: id, text: text.trimmingCharacters(in: .whitespacesAndNewlines), extra: [])
    case let .transcriptionFailed(id, message):
        return flush(session, id: id, text: "", extra: [.report(message)])
    }
}

private func onIdleKey(_ session: Session, signal: HotkeySignal, at: Double) -> (Session, [Effect]) {
    guard signal == .pressed else { return (session, []) }
    let id = session.nextId
    let started = Session(recording: (id, .heldSinceStart(at: at)), nextId: id + 1, nextToPaste: session.nextToPaste, finished: session.finished)
    return (started, [.startCapture(id: id)])
}

private func onKey(_ session: Session, recording: (id: Int, key: KeyPhase), signal: HotkeySignal, at: Double) -> (Session, [Effect]) {
    switch (recording.key, signal) {
    case let (.heldSinceStart(start), .released):
        return at - start >= holdThresholdSeconds ? stop(session, id: recording.id) : (session.with(recording: (recording.id, .toggled)), [])
    case (.heldSinceStart, .interrupted):
        return discard(session, id: recording.id)
    case (.toggled, .pressed):
        return (session.with(recording: (recording.id, .stopPressDown)), [])
    case (.stopPressDown, .released):
        return stop(session, id: recording.id)
    case (.stopPressDown, .interrupted):
        return (session.with(recording: (recording.id, .toggled)), [])
    default:
        return (session, [])
    }
}

private func stop(_ session: Session, id: Int) -> (Session, [Effect]) {
    (session.with(recording: nil), [.stopCaptureAndTranscribe(id: id)])
}

private func discard(_ session: Session, id: Int) -> (Session, [Effect]) {
    flush(session.with(recording: nil), id: id, text: "", extra: [.discardCapture(id: id)])
}

private func flush(_ session: Session, id: Int, text: String, extra: [Effect]) -> (Session, [Effect]) {
    let finished = session.finished.merging([id: text]) { _, new in new }
    let ready = readyRun(from: session.nextToPaste, finished: finished)
    let pastes = ready.compactMap { finished[$0] }.filter { !$0.isEmpty }.map(Effect.paste)
    let remaining = finished.filter { !ready.contains($0.key) }
    let next = Session(recording: session.recording, nextId: session.nextId, nextToPaste: session.nextToPaste + ready.count, finished: remaining)
    return (next, extra + pastes)
}

private func readyRun(from start: Int, finished: [Int: String]) -> [Int] {
    finished[start] == nil ? [] : [start] + readyRun(from: start + 1, finished: finished)
}
