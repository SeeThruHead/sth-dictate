import Foundation

public let sampleRate = 16_000

public func pcm16(_ samples: [Float]) -> [Int16] {
    samples.map { Int16(max(-1, min(1, $0)) * Float(Int16.max)) }
}

public func wav(_ samples: [Int16], sampleRate: Int = sampleRate) -> Data {
    let dataBytes = samples.count * 2
    let header: [Data] = [
        Data("RIFF".utf8), le32(36 + dataBytes), Data("WAVE".utf8),
        Data("fmt ".utf8), le32(16), le16(1), le16(1), le32(sampleRate), le32(sampleRate * 2), le16(2), le16(16),
        Data("data".utf8), le32(dataBytes),
    ]
    let body = samples.reduce(into: Data(capacity: dataBytes)) { $0.append(contentsOf: [UInt8(truncatingIfNeeded: $1), UInt8(truncatingIfNeeded: $1 >> 8)]) }
    return header.reduce(Data(), +) + body
}

public func silence(seconds: Double, sampleRate: Int = sampleRate) -> [Int16] {
    Array(repeating: 0, count: Int(seconds * Double(sampleRate)))
}

private func le16(_ value: Int) -> Data {
    Data([UInt8(value & 0xff), UInt8((value >> 8) & 0xff)])
}

private func le32(_ value: Int) -> Data {
    Data((0..<4).map { UInt8((value >> ($0 * 8)) & 0xff) })
}
