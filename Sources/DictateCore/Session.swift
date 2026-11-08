public enum KeyPhase: Equatable, Sendable {
    case heldSinceStart(at: Double)
    case toggled(at: Double)
    case stopPressDown
}

public struct Streaming: Equatable, Sendable {
    public let lastPress: Double?

    public init(lastPress: Double? = nil) {
        self.lastPress = lastPress
    }
}

public struct Session: Equatable, Sendable {
    public let recording: (id: Int, key: KeyPhase)?
    public let streaming: Streaming?
    public let nextId: Int
    public let nextToPaste: Int
    public let finished: [Int: String]
    public let streamed: Set<Int>

    public init(recording: (id: Int, key: KeyPhase)? = nil, streaming: Streaming? = nil, nextId: Int = 0, nextToPaste: Int = 0, finished: [Int: String] = [:], streamed: Set<Int> = []) {
        self.recording = recording
        self.streaming = streaming
        self.nextId = nextId
        self.nextToPaste = nextToPaste
        self.finished = finished
        self.streamed = streamed
    }

    public static func == (lhs: Session, rhs: Session) -> Bool {
        lhs.recording?.id == rhs.recording?.id && lhs.recording?.key == rhs.recording?.key && lhs.streaming == rhs.streaming
            && lhs.nextId == rhs.nextId && lhs.nextToPaste == rhs.nextToPaste && lhs.finished == rhs.finished && lhs.streamed == rhs.streamed
    }

    public var isRecording: Bool { recording != nil }
    public var isStreaming: Bool { streaming != nil }
    public var isTranscribing: Bool { nextToPaste < nextId - (recording == nil ? 0 : 1) }

    func with(recording: (id: Int, key: KeyPhase)?) -> Session {
        Session(recording: recording, streaming: streaming, nextId: nextId, nextToPaste: nextToPaste, finished: finished, streamed: streamed)
    }

    func with(streaming: Streaming?) -> Session {
        Session(recording: recording, streaming: streaming, nextId: nextId, nextToPaste: nextToPaste, finished: finished, streamed: streamed)
    }
}

public enum Action: Equatable, Sendable {
    case hotkey(HotkeySignal, at: Double)
    case heard([Float])
    case transcribed(id: Int, text: String)
    case transcriptionFailed(id: Int, message: String)
}

public enum Effect: Equatable, Sendable {
    case startCapture(id: Int)
    case stopCaptureAndTranscribe(id: Int)
    case discardCapture(id: Int)
    case startStreaming
    case stopStreaming
    case transcribePiece(id: Int, samples: [Float])
    case paste(String, streamed: Bool)
    case report(String)
}

public let holdThresholdSeconds = 0.4
public let doubleTapSeconds = 0.4

public func reduce(_ session: Session, _ action: Action) -> (Session, [Effect]) {
    switch action {
    case let .hotkey(signal, at):
        return onHotkey(session, signal: signal, at: at)
    case let .heard(samples):
        return onHeard(session, samples: samples)
    case let .transcribed(id, text):
        return flush(session, id: id, text: text.trimmingCharacters(in: .whitespacesAndNewlines), extra: [])
    case let .transcriptionFailed(id, message):
        return flush(session, id: id, text: "", extra: [.report(message)])
    }
}

private func onHotkey(_ session: Session, signal: HotkeySignal, at: Double) -> (Session, [Effect]) {
    if let streaming = session.streaming { return onStreamingKey(session, streaming: streaming, signal: signal, at: at) }
    return session.recording.map { onKey(session, recording: $0, signal: signal, at: at) } ?? onIdleKey(session, signal: signal, at: at)
}

private func onStreamingKey(_ session: Session, streaming: Streaming, signal: HotkeySignal, at: Double) -> (Session, [Effect]) {
    guard signal == .pressed else { return (session, []) }
    guard let last = streaming.lastPress, at - last < doubleTapSeconds else { return (session.with(streaming: Streaming(lastPress: at)), []) }
    return (session.with(streaming: nil), [.stopStreaming])
}

private func onHeard(_ session: Session, samples: [Float]) -> (Session, [Effect]) {
    guard session.isStreaming else { return (session, []) }
    let id = session.nextId
    let next = Session(recording: session.recording, streaming: session.streaming, nextId: id + 1, nextToPaste: session.nextToPaste, finished: session.finished, streamed: session.streamed.union([id]))
    return (next, [.transcribePiece(id: id, samples: samples)])
}

private func onIdleKey(_ session: Session, signal: HotkeySignal, at: Double) -> (Session, [Effect]) {
    guard signal == .pressed else { return (session, []) }
    let id = session.nextId
    let started = Session(recording: (id, .heldSinceStart(at: at)), streaming: nil, nextId: id + 1, nextToPaste: session.nextToPaste, finished: session.finished, streamed: session.streamed)
    return (started, [.startCapture(id: id)])
}

private func onKey(_ session: Session, recording: (id: Int, key: KeyPhase), signal: HotkeySignal, at: Double) -> (Session, [Effect]) {
    switch (recording.key, signal) {
    case let (.heldSinceStart(start), .released):
        return at - start >= holdThresholdSeconds ? stop(session, id: recording.id) : (session.with(recording: (recording.id, .toggled(at: start))), [])
    case (.heldSinceStart, .interrupted):
        return discard(session, id: recording.id)
    case let (.toggled(start), .pressed):
        return at - start < doubleTapSeconds ? startStreaming(session, id: recording.id) : (session.with(recording: (recording.id, .stopPressDown)), [])
    case (.stopPressDown, .released):
        return stop(session, id: recording.id)
    case (.stopPressDown, .interrupted):
        return (session.with(recording: (recording.id, .toggled(at: -.infinity))), [])
    default:
        return (session, [])
    }
}

private func startStreaming(_ session: Session, id: Int) -> (Session, [Effect]) {
    let (discarded, effects) = discard(session, id: id)
    return (discarded.with(streaming: Streaming()), effects + [.startStreaming])
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
    let pastes = ready.compactMap { readyId in finished[readyId].flatMap { $0.isEmpty ? nil : Effect.paste($0, streamed: session.streamed.contains(readyId)) } }
    let remaining = finished.filter { !ready.contains($0.key) }
    let next = Session(recording: session.recording, streaming: session.streaming, nextId: session.nextId, nextToPaste: session.nextToPaste + ready.count, finished: remaining, streamed: session.streamed.subtracting(ready))
    return (next, extra + pastes)
}

private func readyRun(from start: Int, finished: [Int: String]) -> [Int] {
    finished[start] == nil ? [] : [start] + readyRun(from: start + 1, finished: finished)
}
