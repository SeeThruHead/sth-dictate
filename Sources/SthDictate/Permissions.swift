import AVFoundation
import Cocoa

enum Permission: CaseIterable, Identifiable {
    case microphone
    case inputMonitoring
    case accessibility

    var id: Self { self }

    var title: String {
        switch self {
        case .microphone: "Microphone"
        case .inputMonitoring: "Input Monitoring"
        case .accessibility: "Accessibility"
        }
    }

    var purpose: String {
        switch self {
        case .microphone: "to record your voice"
        case .inputMonitoring: "to hear the hotkey"
        case .accessibility: "to paste the text"
        }
    }

    var isGranted: Bool {
        switch self {
        case .microphone: AVCaptureDevice.authorizationStatus(for: .audio) == .authorized
        case .inputMonitoring: CGPreflightListenEventAccess()
        case .accessibility: AXIsProcessTrusted()
        }
    }

    private var pane: String {
        switch self {
        case .microphone: "Privacy_Microphone"
        case .inputMonitoring: "Privacy_ListenEvent"
        case .accessibility: "Privacy_Accessibility"
        }
    }

    func grant() {
        if self == .microphone, AVCaptureDevice.authorizationStatus(for: .audio) == .notDetermined {
            AVCaptureDevice.requestAccess(for: .audio) { _ in }
            return
        }
        URL(string: "x-apple.systempreferences:com.apple.preference.security?\(pane)").map { NSWorkspace.shared.open($0) }
        if self != .microphone { DragHelper.shared.show(for: self) }
    }
}
