import Cocoa
import DictateCore

final class KeyMonitor {
    private let onEvent: (KeyEvent) -> Void
    private var tap: CFMachPort?
    private var source: CFRunLoopSource?

    init(onEvent: @escaping (KeyEvent) -> Void) {
        self.onEvent = onEvent
    }

    var hasPermission: Bool { CGPreflightListenEventAccess() }

    func requestPermission() {
        CGRequestListenEventAccess()
    }

    func start() -> Bool {
        let mask = [CGEventType.keyDown, .keyUp, .flagsChanged].reduce(CGEventMask(0)) { $0 | (1 << $1.rawValue) }
        let refcon = Unmanaged.passUnretained(self).toOpaque()
        guard let tap = CGEvent.tapCreate(tap: .cgSessionEventTap, place: .headInsertEventTap, options: .listenOnly, eventsOfInterest: mask, callback: { _, type, event, refcon in
            guard let refcon else { return Unmanaged.passUnretained(event) }
            Unmanaged<KeyMonitor>.fromOpaque(refcon).takeUnretainedValue().handle(type: type, event: event)
            return Unmanaged.passUnretained(event)
        }, userInfo: refcon) else { return false }
        let source = CFMachPortCreateRunLoopSource(nil, tap, 0)
        CFRunLoopAddSource(CFRunLoopGetMain(), source, .commonModes)
        CGEvent.tapEnable(tap: tap, enable: true)
        self.tap = tap
        self.source = source
        return true
    }

    private func handle(type: CGEventType, event: CGEvent) {
        if type == .tapDisabledByTimeout || type == .tapDisabledByUserInput {
            tap.map { CGEvent.tapEnable(tap: $0, enable: true) }
            return
        }
        keyEvent(type: type, event: event).map(onEvent)
    }

    private func keyEvent(type: CGEventType, event: CGEvent) -> KeyEvent? {
        let keyCode = UInt16(event.getIntegerValueField(.keyboardEventKeycode))
        let flags = event.flags.rawValue
        switch type {
        case .flagsChanged: return .flagsChanged(keyCode: keyCode, flags: flags)
        case .keyDown: return .keyDown(keyCode: keyCode, flags: flags, isRepeat: event.getIntegerValueField(.keyboardEventAutorepeat) != 0)
        case .keyUp: return .keyUp(keyCode: keyCode, flags: flags)
        default: return nil
        }
    }
}
