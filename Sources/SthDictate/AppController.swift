import AVFoundation
import Cocoa
import DictateCore
import SwiftUI

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
        if !Paster.hasPermission { Paster.requestPermission() }
        startKeyMonitor()
    }

    private func startKeyMonitor() {
        if keyMonitor.start() {
            show("Ready: \(settings.hotkeyLabel) to dictate")
            return
        }
        keyMonitor.requestPermission()
        show("Allow SthDictate in Privacy & Security › Input Monitoring")
        DispatchQueue.main.asyncAfter(deadline: .now() + 2) { [weak self] in self?.startKeyMonitor() }
    }

    private func key(_ event: KeyEvent) {
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
            Paster.paste(text)
            show("Pasted \(text.count) characters")
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
        let audio = wav(pcm16(samples))
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
        status = message
        statusLine.title = message
        refreshIcon()
    }

    private func refreshIcon() {
        let symbol = session.isRecording ? "mic.fill" : session.isTranscribing ? "ellipsis.circle" : "mic"
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
        window.contentView = NSHostingView(rootView: view)
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

    @objc private func copyLastTranscript() {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(lastTranscript, forType: .string)
        show("Copied last transcript")
    }

    @objc private func quit() {
        NSApp.terminate(nil)
    }
}
