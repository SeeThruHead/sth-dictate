import AVFoundation
import DictateCore

final class Recorder {
    private let engine = AVAudioEngine()
    private let target = AVAudioFormat(commonFormat: .pcmFormatFloat32, sampleRate: Double(DictateCore.sampleRate), channels: 1, interleaved: false)!
    private let lock = NSLock()
    private var samples: [Float] = []
    private var generation = 0

    func start() -> Result<Void, EndpointError> {
        let input = engine.inputNode
        let format = input.outputFormat(forBus: 0)
        guard format.sampleRate > 0, let converter = AVAudioConverter(from: format, to: target) else {
            return .failure(.invalid("No microphone input available"))
        }
        let current = lock.withLock {
            samples = []
            generation += 1
            return generation
        }
        input.removeTap(onBus: 0)
        input.installTap(onBus: 0, bufferSize: 4096, format: format) { [weak self] buffer, _ in
            self?.append(self?.convert(buffer, with: converter) ?? [], generation: current)
        }
        engine.prepare()
        do {
            try engine.start()
            return .success(())
        } catch {
            input.removeTap(onBus: 0)
            return .failure(.invalid("Microphone could not start: \(error.localizedDescription)"))
        }
    }

    func stop() -> [Float] {
        engine.inputNode.removeTap(onBus: 0)
        engine.stop()
        return lock.withLock {
            generation += 1
            return samples
        }
    }

    private func append(_ chunk: [Float], generation tapped: Int) {
        lock.withLock {
            guard tapped == generation else { return }
            samples.append(contentsOf: chunk)
        }
    }

    private func convert(_ buffer: AVAudioPCMBuffer, with converter: AVAudioConverter) -> [Float] {
        let ratio = target.sampleRate / buffer.format.sampleRate
        let capacity = AVAudioFrameCount(Double(buffer.frameLength) * ratio) + 32
        guard let out = AVAudioPCMBuffer(pcmFormat: target, frameCapacity: capacity) else { return [] }
        let fed = Box(false)
        var error: NSError?
        converter.convert(to: out, error: &error) { _, status in
            if fed.value {
                status.pointee = .noDataNow
                return nil
            }
            fed.value = true
            status.pointee = .haveData
            return buffer
        }
        guard error == nil, let channel = out.floatChannelData?[0] else { return [] }
        return Array(UnsafeBufferPointer(start: channel, count: Int(out.frameLength)))
    }
}

private final class Box<T> {
    var value: T
    init(_ value: T) { self.value = value }
}
