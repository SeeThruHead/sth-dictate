import AVFoundation
import Cocoa
import DictateCore
import SwiftUI
import os

private let log = Logger(subsystem: "com.sth.dictate", category: "app")

final class AppController: NSObject, NSMenuDelegate {
    private var settings = SettingsStore.load()
    private var session = Session()
    private var tracker = HotkeyTracker()
    private var status = "Starting"
    private var capturingHotkey = false
    private var lastTranscript = ""
    private let copyLast = NSMenuItem(title: "Copy Last Transcript", action: nil, keyEquivalent: "")
    private let pinMenu = NSMenu(title: "Pin")
    private var pin = PinStore.load()
    private var lastHerdr = Route.herdrServer
    private let deliveryQueue = DispatchQueue(label: "com.sth.dictate.delivery")
    private var listener = Listener()
    private var pendingAudio: [Float] = []
    private var speaking = false
    private let recorder = Recorder()
    private let statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
    private let statusLine = NSMenuItem(title: "", action: nil, keyEquivalent: "")
    private let microphoneMenu = NSMenu(title: "Microphone")
    private var settingsWindow: NSWindow?
    private var listening = false
    private let broadcaster = StateBroadcast("com.sth.dictate")
    private lazy var keyMonitor = KeyMonitor { [weak self] event, at in self?.key(event, at: at) }

    func launch() {
        let menu = NSMenu()
        statusLine.isEnabled = false
        menu.addItem(statusLine)
        menu.addItem(.separator())
        copyLast.action = #selector(copyLastTranscript)
        copyLast.target = self
        copyLast.isEnabled = false
        menu.autoenablesItems = false
        menu.addItem(copyLast)
        let pinItem = NSMenuItem(title: "Pin", action: nil, keyEquivalent: "")
        pinItem.submenu = pinMenu
        pinMenu.delegate = self
        pinMenu.autoenablesItems = false
        menu.addItem(pinItem)
        watchActivations()
        let microphoneItem = NSMenuItem(title: "Microphone", action: nil, keyEquivalent: "")
        microphoneItem.submenu = microphoneMenu
        microphoneMenu.delegate = self
        microphoneMenu.autoenablesItems = false
        menu.addItem(microphoneItem)
        menu.addItem(withTitle: "Settings…", action: #selector(openSettings), keyEquivalent: ",").target = self
        menu.addItem(withTitle: "Quit SthDictate", action: #selector(quit), keyEquivalent: "q").target = self
        statusItem.menu = menu
        AVCaptureDevice.requestAccess(for: .audio) { _ in }
        startKeyMonitor()
    }

    private let waitingForInputMonitoring = "Allow SthDictate in Privacy & Security › Input Monitoring"

    private func startKeyMonitor() {
        guard keyMonitor.hasPermission, keyMonitor.start() else {
            if status != waitingForInputMonitoring { show(waitingForInputMonitoring) }
            DispatchQueue.main.asyncAfter(deadline: .now() + 2) { [weak self] in self?.startKeyMonitor() }
            return
        }
        listening = true
        show("Ready: \(settings.hotkeyLabel) to dictate")
    }

    private func key(_ event: KeyEvent, at: Double) {
        if case let .flagsChanged(keyCode, flags) = event { log.notice("flagsChanged key \(keyCode) flags \(String(flags, radix: 16), privacy: .public) at \(at, format: .fixed(precision: 3)) uptime \(ProcessInfo.processInfo.systemUptime, format: .fixed(precision: 3))") }
        guard !capturingHotkey else { return }
        let (next, signal) = tracker.step(event, hotkey: settings.hotkey)
        tracker = next
        signal.map { dispatch(.hotkey($0, at: at)) }
    }

    private func dispatch(_ action: Action) {
        let (next, effects) = reduce(session, action)
        session = next
        effects.forEach(run)
        refreshIcon()
    }

    private func run(_ effect: Effect) {
        switch effect {
        case let .startCapture(id):
            startCapture(id)
        case let .stopCaptureAndTranscribe(id):
            NSSound(named: "Pop")?.play()
            transcribe(id: id, samples: recorder.stop(), streamed: false)
        case .discardCapture:
            _ = recorder.stop()
        case .startStreaming:
            startStreaming()
        case .stopStreaming:
            _ = recorder.stop()
            speaking = false
            NSSound(named: "Pop")?.play()
            show("Streaming off")
        case let .transcribePiece(id, samples):
            transcribe(id: id, samples: samples, streamed: true)
        case let .paste(text, streamed):
            lastTranscript = text
            copyLast.isEnabled = true
            deliver(text, streamed: streamed)
        case let .report(message):
            show(message)
        }
    }

    private func startCapture(_ id: Int) {
        recorder.start(microphone: settings.microphone, channel: settings.channel) { [weak self] message in self?.dispatch(.transcriptionFailed(id: id, message: message)) }
        NSSound(named: "Tink")?.play()
        show("Listening…")
    }

    private func startStreaming() {
        listener = Listener()
        pendingAudio = []
        speaking = false
        recorder.start(microphone: settings.microphone, channel: settings.channel, onChunk: { [weak self] chunk in self?.hear(chunk) }) { [weak self] message in self?.show(message) }
        NSSound(named: "Tink")?.play()
        show(streamingMessage)
    }

    private func hear(_ chunk: [Float]) {
        guard session.isStreaming else { return }
        let (cut, rest) = frames(pending: pendingAudio, chunk: chunk)
        pendingAudio = rest
        cut.forEach { frame in
            let (next, heard) = listener.hear(frame, tuning: .standard)
            listener = next
            heard.map(onHeard)
        }
    }

    private func onHeard(_ heard: Heard) {
        switch heard {
        case .speaking:
            speaking = true
        case .noise:
            speaking = false
        case let .piece(samples):
            speaking = false
            dispatch(.heard(samples))
        }
        show(streamingMessage)
    }

    private var streamingMessage: String {
        if speaking { return "Streaming: hearing you…" }
        if session.isTranscribing { return "Streaming: sending…" }
        return "Streaming: each pause sends what you said"
    }

    private func transcribe(id: Int, samples: [Float], streamed: Bool) {
        let audio = wav(pcm16(louder(samples)))
        let peak = samples.map(abs).max() ?? 0
        log.notice("recorded \(Double(samples.count) / Double(DictateCore.sampleRate), format: .fixed(precision: 1))s, peak \(peak, format: .fixed(precision: 3))")
        guard streamed || !tooShort(samples) else {
            dispatch(.transcribed(id: id, text: ""))
            show("Too short, nothing sent")
            return
        }
        let cache = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0].appendingPathComponent("com.sth.dictate")
        try? FileManager.default.createDirectory(at: cache, withIntermediateDirectories: true)
        try? audio.write(to: cache.appendingPathComponent("last.wav"))
        let settings = settings
        show(streamed ? streamingMessage : "Transcribing…")
        Task {
            let result = await Transcriber.transcribe(audio, settings: settings)
            await MainActor.run {
                switch result {
                case let .success(text): self.dispatch(.transcribed(id: id, text: streamed && isFiller(text) ? "" : text))
                case let .failure(.invalid(message)): self.dispatch(.transcriptionFailed(id: id, message: message))
                }
            }
        }
    }

    private func show(_ message: String) {
        log.notice("status: \(message, privacy: .public)")
        status = message
        statusLine.title = message
        refreshIcon()
    }

    private var iconSymbol: String {
        if !listening { return "exclamationmark.triangle" }
        if session.isStreaming { return speaking ? "waveform.circle.fill" : "dot.radiowaves.left.and.right" }
        if session.isRecording { return "waveform.circle.fill" }
        if session.isTranscribing { return "ellipsis.circle" }

        return "waveform"
    }

    private var stateName: String {
        if !listening { return "unavailable" }
        if session.isStreaming { return "streaming" }
        if session.isRecording { return "recording" }
        if session.isTranscribing { return "transcribing" }

        return "idle"
    }

    private func broadcast() {
        broadcaster.publish(["state": stateName, "pinned_to": pin.target?.label ?? ""])
    }

    private func refreshIcon() {
        broadcast()
        let symbol = iconSymbol
        let image = NSImage(systemSymbolName: symbol, accessibilityDescription: status)
        let hot = session.isRecording || speaking
        image?.isTemplate = !hot && !session.isStreaming
        statusItem.button?.image = image
        statusItem.button?.contentTintColor = hot ? .systemRed : session.isStreaming ? .systemBlue : nil
    }

    @objc private func openSettings() {
        let view = SettingsView(
            settings: settings,
            onSave: { [weak self] in self?.save($0) },
            onCapturing: { [weak self] in self?.capturingHotkey = $0 },
            onTest: { await Transcriber.test(settings: $0) }
        )
        let window = settingsWindow ?? NSWindow(contentRect: .zero, styleMask: [.titled, .closable], backing: .buffered, defer: false)
        window.title = "SthDictate"
        let hosting = NSHostingView(rootView: view)
        window.contentView = hosting
        window.setContentSize(hosting.fittingSize)
        window.isReleasedWhenClosed = false
        window.center()
        settingsWindow = window
        NSApp.activate(ignoringOtherApps: true)
        window.makeKeyAndOrderFront(nil)
    }

    private func save(_ next: DictateSettings) {
        settings = next
        tracker = HotkeyTracker()
        SettingsStore.save(next)
        show("Ready: \(next.hotkeyLabel) to dictate")
    }

    private func deliver(_ text: String, streamed: Bool) {
        if let target = pin.target { return sendToPin(text, target: target) }
        guard Paster.hasPermission else {
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(text, forType: .string)
            Paster.requestPermission()
            show("Copied, not pasted: allow SthDictate in Accessibility to paste")
            return
        }
        Paster.paste(text)
        if streamed { DispatchQueue.main.asyncAfter(deadline: .now() + 0.08) { Paster.pressReturn() } }
        show(session.isStreaming ? streamingMessage : "Pasted \(text.count) characters")
    }

    private func sendToPin(_ text: String, target: PinTarget) {
        let lastHerdr = lastHerdr
        deliveryQueue.async {
            let failure = Deliver.send(text, to: target, lastHerdr: lastHerdr)
            DispatchQueue.main.async { self.show(failure ?? (self.session.isStreaming ? self.streamingMessage : "Sent \(text.count) characters to \(target.label)")) }
        }
    }

    private func watchActivations() {
        NSWorkspace.shared.notificationCenter.addObserver(forName: NSWorkspace.didActivateApplicationNotification, object: nil, queue: .main) { [weak self] note in
            guard let app = note.userInfo?[NSWorkspace.applicationUserInfoKey] as? NSRunningApplication else { return }
            self?.deliveryQueue.async {
                let route = Deliver.routeFor(app)
                guard route != .keys else { return }
                DispatchQueue.main.async { self?.lastHerdr = route }
            }
        }
    }

    private var frontApp: NSRunningApplication? {
        NSWorkspace.shared.frontmostApplication.flatMap { $0.bundleIdentifier == Bundle.main.bundleIdentifier ? nil : $0 }
    }

    private func setPin(_ next: Pin) {
        pin = next
        PinStore.save(next)
        refreshIcon()
        show(next.target.map { "Pinned to \($0.label): dictation goes there, then Enter" } ?? "Not pinned: dictation pastes at the cursor")
    }

    @objc private func pinFrontApp(_ sender: NSMenuItem) {
        guard let target = sender.representedObject as? Data, let chosen = try? JSONDecoder().decode(PinTarget.self, from: target) else { return }
        setPin(Pin(on: true, chosen: chosen))
    }

    @objc private func pinHerdr() {
        setPin(Pin(on: true, chosen: .herdr))
    }

    @objc private func unpin() {
        setPin(Pin(on: false, chosen: pin.chosen))
    }

    private func pinItems() -> [NSMenuItem] {
        let header = NSMenuItem(title: pin.target.map { "Pinned to \($0.label)" } ?? "Not pinned: pasting at the cursor", action: nil, keyEquivalent: "")
        header.isEnabled = false
        let front = frontApp.flatMap { app -> NSMenuItem? in
            guard let bundleId = app.bundleIdentifier, let name = app.localizedName else { return nil }
            let item = NSMenuItem(title: "Pin to \(name)", action: #selector(pinFrontApp(_:)), keyEquivalent: "")
            item.target = self
            item.representedObject = try? JSONEncoder().encode(PinTarget.app(bundleId: bundleId, name: name))
            item.state = pin.target == .app(bundleId: bundleId, name: name) ? .on : .off
            return item
        }
        let herdr = NSMenuItem(title: "Pin to Herdr (its focused pane)", action: #selector(pinHerdr), keyEquivalent: "")
        herdr.target = self
        herdr.state = pin.target == .herdr ? .on : .off
        let off = NSMenuItem(title: "Unpin", action: #selector(unpin), keyEquivalent: "")
        off.target = self
        off.isEnabled = pin.on
        return [header, .separator()] + (front.map { [$0] } ?? []) + [herdr, off]
    }

    @objc private func copyLastTranscript() {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(lastTranscript, forType: .string)
        show("Copied last transcript")
    }

    func menuNeedsUpdate(_ menu: NSMenu) {
        if menu === pinMenu {
            menu.removeAllItems()
            pinItems().forEach(menu.addItem)
            return
        }
        guard menu === microphoneMenu else { return }
        menu.removeAllItems()
        let follow = NSMenuItem(title: "Follow system input", action: #selector(chooseMicrophone(_:)), keyEquivalent: "")
        follow.target = self
        follow.state = settings.microphone == nil ? .on : .off
        menu.addItem(follow)
        menu.addItem(.separator())
        Microphones.all().forEach { microphone in
            let item = NSMenuItem(title: microphone.name, action: #selector(chooseMicrophone(_:)), keyEquivalent: "")
            item.target = self
            item.representedObject = microphone.uid
            item.state = settings.microphone == microphone.uid ? .on : .off
            menu.addItem(item)
        }
        channelItems().forEach(menu.addItem)
        if let uid = settings.microphone, Microphones.device(uid: uid) == nil {
            let missing = NSMenuItem(title: "Chosen microphone is not connected, using the system input", action: nil, keyEquivalent: "")
            missing.isEnabled = false
            menu.addItem(missing)
        }
    }

    private func channelItems() -> [NSMenuItem] {
        guard let uid = settings.microphone, let microphone = Microphones.all().first(where: { $0.uid == uid }), microphone.channels > 1 else { return [] }
        let header = NSMenuItem(title: "Channel on \(microphone.name)", action: nil, keyEquivalent: "")
        header.isEnabled = false
        let loudest = channelItem("Loudest channel (automatic)", channel: nil)
        let inputs = (0..<microphone.channels).map { channelItem("Input \($0 + 1)", channel: $0) }
        return [.separator(), header, loudest] + inputs
    }

    private func channelItem(_ title: String, channel: Int?) -> NSMenuItem {
        let item = NSMenuItem(title: title, action: #selector(chooseChannel(_:)), keyEquivalent: "")
        item.target = self
        item.representedObject = channel
        item.state = settings.channel == channel ? .on : .off
        return item
    }

    @objc private func chooseChannel(_ sender: NSMenuItem) {
        let channel = sender.representedObject as? Int
        settings = settings.with(channel: channel)
        SettingsStore.save(settings)
        show(channel.map { "Channel: Input \($0 + 1)" } ?? "Channel: loudest, chosen automatically")
    }

    @objc private func chooseMicrophone(_ sender: NSMenuItem) {
        let uid = sender.representedObject as? String
        settings = settings.with(microphone: uid)
        SettingsStore.save(settings)
        show(uid == nil ? "Microphone: follows the system input" : "Microphone: \(sender.title)")
    }

    @objc private func quit() {
        NSApp.terminate(nil)
    }
}
