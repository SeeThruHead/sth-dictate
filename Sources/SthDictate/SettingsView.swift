import Cocoa
import DictateCore
import SwiftUI

struct SettingsView: View {
    let settings: DictateSettings
    let onSave: (DictateSettings) -> Void
    let onCapturing: (Bool) -> Void
    let onTest: (DictateSettings) async -> String

    @State private var endpoint = ""
    @State private var model = ""
    @State private var hotkey = Hotkey.rightCommand
    @State private var hotkeyLabel = ""
    @State private var capturing = false
    @State private var monitor: Any?
    @State private var testResult = ""
    @State private var testing = false

    var body: some View {
        Form {
            TextField("Endpoint", text: $endpoint, prompt: Text("http://host:8000/v1"))
            TextField("Model", text: $model, prompt: Text("optional"))
            LabeledContent("Hotkey") {
                Button(capturing ? "Press a key…" : hotkeyLabel) { capturing ? stopCapture() : startCapture() }
            }
            LabeledContent("Test") {
                HStack {
                    Button(testing ? "Testing…" : "Test endpoint") { test() }.disabled(testing)
                    Text(testResult).foregroundStyle(.secondary).lineLimit(2)
                }
            }
        }
        .formStyle(.grouped)
        .frame(width: 440)
        .onAppear(perform: load)
        .onDisappear(perform: stopCapture)
        .onChange(of: endpoint) { save() }
        .onChange(of: model) { save() }
    }

    private var current: DictateSettings { settings.with(endpoint: endpoint, model: model, hotkey: hotkey, hotkeyLabel: hotkeyLabel) }

    private func load() {
        endpoint = settings.endpoint
        model = settings.model
        hotkey = settings.hotkey
        hotkeyLabel = settings.hotkeyLabel
    }

    private func save() {
        onSave(current)
    }

    private func test() {
        testing = true
        testResult = ""
        let snapshot = current
        Task {
            let result = await onTest(snapshot)
            testResult = result
            testing = false
        }
    }

    private func startCapture() {
        capturing = true
        onCapturing(true)
        monitor = NSEvent.addLocalMonitorForEvents(matching: [.keyDown, .flagsChanged]) { event in
            captured(event).map(choose)
            return nil
        }
    }

    private func stopCapture() {
        monitor.map(NSEvent.removeMonitor)
        monitor = nil
        capturing = false
        onCapturing(false)
    }

    private func choose(_ choice: (Hotkey, String)) {
        hotkey = choice.0
        hotkeyLabel = choice.1
        stopCapture()
        save()
    }

    private func captured(_ event: NSEvent) -> (Hotkey, String)? {
        let flags = UInt64(event.modifierFlags.rawValue) & ModifierFlag.relevant
        switch event.type {
        case .flagsChanged:
            let modifier = Hotkey(keyCode: event.keyCode, modifiers: 0)
            let isPress = ModifierKey.byKeyCode[event.keyCode].map { UInt64(event.modifierFlags.rawValue) & $0.deviceMask != 0 } ?? false
            return isPress ? (modifier, hotkeyName(modifier, keyName: { _ in "" })) : nil
        case .keyDown:
            let combo = Hotkey(keyCode: event.keyCode, modifiers: flags)
            let key = event.charactersIgnoringModifiers.map { $0 == " " ? "Space" : $0.uppercased() } ?? "Key \(event.keyCode)"
            return (combo, hotkeyName(combo, keyName: { _ in key }))
        default:
            return nil
        }
    }
}
