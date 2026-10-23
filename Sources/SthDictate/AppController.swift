import AVFoundation
import Cocoa
import DictateCore
import SwiftUI
import os

private let log = Logger(subsystem: "com.sth.dictate", category: "app")

final class AppController: NSObject {
    private var settings = SettingsStore.load()
    private var session = Session()
    private var tracker = HotkeyTracker()
    private var status = "Starting"
    private var capturingHotkey = false
    private var lastTranscript = ""
    private let copyLast = NSMenuItem(title: "Copy Last Transcript", action: nil, keyEquivalent: "")
    private let recorder = Recorder()
    private let statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
    private let statusLine = NSMenuItem(title: "", action: nil, keyEquivalent: "")
    private var settingsWindow: NSWindow?
    private var listening = false
    private lazy var keyMonitor = KeyMonitor { [weak self] event in self?.key(event) }

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

    private func key(_ event: KeyEvent) {
        if case let .flagsChanged(keyCode, flags) = event { log.notice("flagsChanged key \(keyCode) flags \(String(flags, radix: 16), privacy: .public)") }
        guard !capturingHotkey else { return }
        let (next, signal) = tracker.step(event, hotkey: settings.hotkey)
        tracker = next
        signal.map { dispatch(.hotkey($0, at: ProcessInfo.processInfo.systemUptime)) }
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
            transcribe(id: id, samples: recorder.stop())
        case .discardCapture:
            _ = recorder.stop()
        case let .paste(text):
            lastTranscript = text
            copyLast.isEnabled = true
            deliver(text)
        case let .report(message):
            show(message)
        }
    }

    private func startCapture(_ id: Int) {
        switch recorder.start() {
        case .success:
            NSSound(named: "Tink")?.play()
            show("Listening…")
        case let .failure(.invalid(message)):
            dispatch(.transcriptionFailed(id: id, message: message))
        }
    }

    private func transcribe(id: Int, samples: [Float]) {
        let audio = wav(pcm16(louder(samples)))
        let peak = samples.map(abs).max() ?? 0
        log.notice("recorded \(Double(samples.count) / Double(DictateCore.sampleRate), format: .fixed(precision: 1))s, peak \(peak, format: .fixed(precision: 3))")
        guard !tooShort(samples) else {
            dispatch(.transcribed(id: id, text: ""))
            show("Too short, nothing sent")
            return
        }
        let cache = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0].appendingPathComponent("com.sth.dictate")
        try? FileManager.default.createDirectory(at: cache, withIntermediateDirectories: true)
        try? audio.write(to: cache.appendingPathComponent("last.wav"))
        let settings = settings
        show("Transcribing…")
        Task {
            let result = await Transcriber.transcribe(audio, settings: settings)
            await MainActor.run {
                switch result {
                case let .success(text): self.dispatch(.transcribed(id: id, text: text))
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
        if session.isRecording { return "waveform.circle.fill" }
        if session.isTranscribing { return "ellipsis.circle" }

        return "waveform"
    }

    private func refreshIcon() {
        let symbol = iconSymbol
        let image = NSImage(systemSymbolName: symbol, accessibilityDescription: status)
        image?.isTemplate = !session.isRecording
        statusItem.button?.image = image
        statusItem.button?.contentTintColor = session.isRecording ? .systemRed : nil
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

    private func deliver(_ text: String) {
        guard Paster.hasPermission else {
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(text, forType: .string)
            Paster.requestPermission()
            show("Copied, not pasted: allow SthDictate in Accessibility to paste")
            return
        }
        Paster.paste(text)
        show("Pasted \(text.count) characters")
    }

    @objc private func copyLastTranscript() {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(lastTranscript, forType: .string)
        show("Copied last transcript")
    }

    @objc private func quit() {
        NSApp.terminate(nil)
    }
}
