import AVFoundation
import AudioToolbox
import DictateCore
import os

private let log = Logger(subsystem: "com.sth.dictate", category: "recorder")

final class Recorder {
    private let queue = DispatchQueue(label: "com.sth.dictate.recorder")
    private var engine = AVAudioEngine()
    private var observer: NSObjectProtocol?
    private var recording = false
    private let target = AVAudioFormat(commonFormat: .pcmFormatFloat32, sampleRate: Double(DictateCore.sampleRate), channels: 1, interleaved: false)!
    private let lock = NSLock()
    private var samples: [Float] = []
    private var generation = 0

    private var device: AudioDeviceID?
    private var engineDevice: AudioDeviceID?
    private var channel: Int?

    func start(microphone uid: String?, channel picked: Int?, onFailure: @escaping (String) -> Void) {
        let current = lock.withLock {
            samples = []
            generation += 1
            return generation
        }
        queue.async { [self] in
            device = uid.flatMap(Microphones.device(uid:))
            if uid != nil, device == nil { log.notice("chosen microphone not connected, using the system input") }
            restarts = 0
            channel = device == nil ? nil : picked
            recording = true
            let wanted = device ?? Microphones.defaultInput()
            let fresh = wanted != engineDevice
            if fresh {
                engine = AVAudioEngine()
                engineDevice = wanted
                watchConfiguration()
            }
            guard let message = begin(generation: current, selectDevice: fresh) else { return }
            engineDevice = nil
            recording = false
            DispatchQueue.main.async { onFailure(message) }
        }
    }

    func stop() -> [Float] {
        queue.sync {
            recording = false
            engine.inputNode.removeTap(onBus: 0)
            engine.stop()
        }
        return lock.withLock {
            generation += 1
            return samples
        }
    }

    private func begin(generation current: Int, selectDevice: Bool = true) -> String? {
        let input = engine.inputNode
        if selectDevice, var id = device, let unit = input.audioUnit {
            let status = AudioUnitSetProperty(unit, kAudioOutputUnitProperty_CurrentDevice, kAudioUnitScope_Global, 0, &id, UInt32(MemoryLayout<AudioDeviceID>.size))
            if status != noErr { return "Could not use the chosen microphone (error \(status))" }
        }
        guard input.inputFormat(forBus: 0).sampleRate > 0 else { return "No microphone input available" }
        let converters = Box([Double: (AVAudioFormat, AVAudioConverter)]())
        input.removeTap(onBus: 0)
        input.installTap(onBus: 0, bufferSize: 4096, format: input.inputFormat(forBus: 0)) { [weak self] buffer, _ in
            guard let self, let (mono, converter) = self.converter(for: buffer.format, cache: converters) else { return }
            let mixed = Recorder.mixDown(buffer, into: mono, channel: self.channel)
            self.append(mixed.map { self.convert($0, with: converter) } ?? [], generation: current)
        }
        engine.prepare()
        do {
            try engine.start()
            log.notice("input started at \(input.outputFormat(forBus: 0).sampleRate, format: .fixed(precision: 0)) Hz")
            return nil
        } catch {
            input.removeTap(onBus: 0)
            return "Microphone could not start: \(error.localizedDescription)"
        }
    }

    private func converter(for format: AVAudioFormat, cache: Box<[Double: (AVAudioFormat, AVAudioConverter)]>) -> (AVAudioFormat, AVAudioConverter)? {
        if let known = cache.value[format.sampleRate] { return known }
        guard format.sampleRate > 0, let mono = AVAudioFormat(standardFormatWithSampleRate: format.sampleRate, channels: 1), let converter = AVAudioConverter(from: mono, to: target) else { return nil }
        cache.value[format.sampleRate] = (mono, converter)
        return (mono, converter)
    }

    private func watchConfiguration() {
        observer.map(NotificationCenter.default.removeObserver)
        observer = NotificationCenter.default.addObserver(forName: .AVAudioEngineConfigurationChange, object: engine, queue: nil) { [weak self] _ in
            self?.queue.async { self?.reattach() }
        }
    }

    private var restarts = 0

    private func reattach() {
        guard recording else { return }
        let current = lock.withLock { generation }
        engine.inputNode.removeTap(onBus: 0)
        engine.stop()
        if device != nil, restarts < 3 {
            restarts += 1
            log.notice("input changed while recording, restarting on the same device")
            begin(generation: current, selectDevice: false).map { log.error("\($0, privacy: .public)") }
            return
        }
        guard device == nil else {
            log.error("input keeps changing, giving up on this recording")
            return
        }
        log.notice("input changed while recording, starting a fresh engine")
        engine = AVAudioEngine()
        engineDevice = Microphones.defaultInput()
        watchConfiguration()
        begin(generation: current).map { log.error("\($0, privacy: .public)") }
    }

    private func append(_ chunk: [Float], generation tapped: Int) {
        lock.withLock {
            guard tapped == generation else { return }
            samples.append(contentsOf: chunk)
        }
    }

    private static func mixDown(_ buffer: AVAudioPCMBuffer, into mono: AVAudioFormat, channel picked: Int?) -> AVAudioPCMBuffer? {
        guard let channels = buffer.floatChannelData, let out = AVAudioPCMBuffer(pcmFormat: mono, frameCapacity: buffer.frameLength), let target = out.floatChannelData?[0] else { return nil }
        let frames = Int(buffer.frameLength)
        let count = Int(buffer.format.channelCount)
        let energy = (0..<count).map { channel in (0..<frames).reduce(Float(0)) { $0 + channels[channel][$1] * channels[channel][$1] } }
        let loudest = energy.indices.max { energy[$0] < energy[$1] } ?? 0
        let source = picked.flatMap { $0 < count ? $0 : nil } ?? loudest
        (0..<frames).forEach { target[$0] = channels[source][$0] }
        out.frameLength = buffer.frameLength
        return out
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
