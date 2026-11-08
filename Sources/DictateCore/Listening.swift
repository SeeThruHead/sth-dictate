import Foundation

public let listeningFrame = 480

private let frameSeconds = 0.03
private let startFrames = 3
private let prerollFrames = 10
private let shortestSpeechFrames = 10
private let tailFrames = 5
private let longestFrames = 1000
private let floorStart = 100.0 / 32767.0
private let floorRate = 0.02
private let aboveFloor = 3.0

public struct Tuning: Equatable, Sendable {
    public let pauseFrames: Int
    public let voiceLevel: Double

    public init(pauseSeconds: Double, voiceLevel: Double) {
        pauseFrames = max(1, Int((pauseSeconds / frameSeconds).rounded()))
        self.voiceLevel = voiceLevel
    }

    public static let standard = Tuning(pauseSeconds: 1.0, voiceLevel: 150.0 / 32767.0)
}

public enum Heard: Equatable, Sendable {
    case speaking
    case piece([Float])
    case noise
}

public struct Listener: Equatable, Sendable {
    let floor: Double
    let preroll: [[Float]]
    let voicedRun: Int
    let speech: [[Float]]?
    let silentRun: Int
    let voiced: Int

    public init() {
        self.init(floor: floorStart, preroll: [], voicedRun: 0, speech: nil, silentRun: 0, voiced: 0)
    }

    init(floor: Double, preroll: [[Float]], voicedRun: Int, speech: [[Float]]?, silentRun: Int, voiced: Int) {
        self.floor = floor
        self.preroll = preroll
        self.voicedRun = voicedRun
        self.speech = speech
        self.silentRun = silentRun
        self.voiced = voiced
    }

    public func hear(_ frame: [Float], tuning: Tuning) -> (Listener, Heard?) {
        let loudness = level(frame)
        let isVoiced = loudness > max(tuning.voiceLevel, floor * aboveFloor)
        let nextFloor = isVoiced ? floor : floor + (loudness - floor) * floorRate
        guard let speech else { return quietFrame(floor: nextFloor, frame: frame, isVoiced: isVoiced) }
        return speechFrame(floor: nextFloor, speech: speech + [frame], isVoiced: isVoiced, tuning: tuning)
    }

    private func quietFrame(floor: Double, frame: [Float], isVoiced: Bool) -> (Listener, Heard?) {
        let kept = Array((preroll + [frame]).suffix(prerollFrames))
        let run = isVoiced ? voicedRun + 1 : 0
        guard run >= startFrames else { return (Listener(floor: floor, preroll: kept, voicedRun: run, speech: nil, silentRun: 0, voiced: 0), nil) }
        return (Listener(floor: floor, preroll: [], voicedRun: 0, speech: kept, silentRun: 0, voiced: run), .speaking)
    }

    private func speechFrame(floor: Double, speech: [[Float]], isVoiced: Bool, tuning: Tuning) -> (Listener, Heard?) {
        let silent = isVoiced ? 0 : silentRun + 1
        let count = voiced + (isVoiced ? 1 : 0)
        guard silent >= tuning.pauseFrames || speech.count >= longestFrames else {
            return (Listener(floor: floor, preroll: [], voicedRun: 0, speech: speech, silentRun: silent, voiced: count), nil)
        }
        let done = Listener(floor: floor, preroll: [], voicedRun: 0, speech: nil, silentRun: 0, voiced: 0)
        guard count >= shortestSpeechFrames else { return (done, .noise) }
        let keep = speech.count - max(0, silent - tailFrames)
        return (done, .piece(Array(speech.prefix(keep).joined())))
    }
}

public func level(_ frame: [Float]) -> Double {
    guard !frame.isEmpty else { return 0 }
    return (frame.reduce(0.0) { $0 + Double($1) * Double($1) } / Double(frame.count)).squareRoot()
}

public func frames(pending: [Float], chunk: [Float]) -> (frames: [[Float]], rest: [Float]) {
    let all = pending + chunk
    let whole = all.count / listeningFrame
    let cut = (0..<whole).map { Array(all[($0 * listeningFrame)..<(($0 + 1) * listeningFrame)]) }
    return (cut, Array(all[(whole * listeningFrame)...]))
}

private let fillers: Set<String> = ["thank you", "thanks", "thanks for watching", "thank you for watching", "you", "bye"]

public func isFiller(_ text: String) -> Bool {
    let kept = String(text.lowercased().unicodeScalars.filter { CharacterSet.alphanumerics.contains($0) || CharacterSet.whitespaces.contains($0) })
    let words = kept.split(whereSeparator: \.isWhitespace).joined(separator: " ")
    return words.isEmpty || fillers.contains(words)
}
