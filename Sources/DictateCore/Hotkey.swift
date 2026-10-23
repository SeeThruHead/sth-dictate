public struct Hotkey: Codable, Equatable, Sendable {
    public let keyCode: UInt16
    public let modifiers: UInt64

    public init(keyCode: UInt16, modifiers: UInt64) {
        self.keyCode = keyCode
        self.modifiers = modifiers
    }

    public static let rightCommand = Hotkey(keyCode: 54, modifiers: 0)

    public var isModifierOnly: Bool { ModifierKey.byKeyCode[keyCode] != nil }
}

public struct ModifierKey: Sendable {
    public let name: String
    public let deviceMask: UInt64

    public static let byKeyCode: [UInt16: ModifierKey] = [
        54: ModifierKey(name: "Right ⌘", deviceMask: 0x10),
        55: ModifierKey(name: "Left ⌘", deviceMask: 0x08),
        56: ModifierKey(name: "Left ⇧", deviceMask: 0x02),
        60: ModifierKey(name: "Right ⇧", deviceMask: 0x04),
        58: ModifierKey(name: "Left ⌥", deviceMask: 0x20),
        61: ModifierKey(name: "Right ⌥", deviceMask: 0x40),
        59: ModifierKey(name: "Left ⌃", deviceMask: 0x01),
        62: ModifierKey(name: "Right ⌃", deviceMask: 0x2000),
        63: ModifierKey(name: "fn", deviceMask: 0x800000),
    ]
}

public enum ModifierFlag {
    public static let shift: UInt64 = 0x20000
    public static let control: UInt64 = 0x40000
    public static let option: UInt64 = 0x80000
    public static let command: UInt64 = 0x100000
    public static let relevant: UInt64 = shift | control | option | command
}

public enum KeyEvent: Equatable, Sendable {
    case flagsChanged(keyCode: UInt16, flags: UInt64)
    case keyDown(keyCode: UInt16, flags: UInt64, isRepeat: Bool)
    case keyUp(keyCode: UInt16, flags: UInt64)
}

public enum HotkeySignal: Equatable, Sendable {
    case pressed
    case released
    case interrupted
}

public struct HotkeyTracker: Equatable, Sendable {
    public let isDown: Bool

    public init(isDown: Bool = false) {
        self.isDown = isDown
    }

    public func step(_ event: KeyEvent, hotkey: Hotkey) -> (HotkeyTracker, HotkeySignal?) {
        hotkey.isModifierOnly ? stepModifier(event, hotkey: hotkey) : stepCombo(event, hotkey: hotkey)
    }

    private func stepModifier(_ event: KeyEvent, hotkey: Hotkey) -> (HotkeyTracker, HotkeySignal?) {
        switch event {
        case let .flagsChanged(keyCode, flags) where keyCode == hotkey.keyCode:
            let down = (flags & (ModifierKey.byKeyCode[keyCode]?.deviceMask ?? 0)) != 0
            return transition(to: down)
        case .keyDown where isDown:
            return (HotkeyTracker(isDown: false), .interrupted)
        default:
            return (self, nil)
        }
    }

    private func stepCombo(_ event: KeyEvent, hotkey: Hotkey) -> (HotkeyTracker, HotkeySignal?) {
        switch event {
        case let .keyDown(keyCode, flags, isRepeat) where keyCode == hotkey.keyCode && !isRepeat:
            return (flags & ModifierFlag.relevant) == hotkey.modifiers ? transition(to: true) : (self, nil)
        case let .keyUp(keyCode, _) where keyCode == hotkey.keyCode:
            return transition(to: false)
        default:
            return (self, nil)
        }
    }

    private func transition(to down: Bool) -> (HotkeyTracker, HotkeySignal?) {
        guard down != isDown else { return (self, nil) }
        return (HotkeyTracker(isDown: down), down ? .pressed : .released)
    }
}

public func hotkeyName(_ hotkey: Hotkey, keyName: (UInt16) -> String) -> String {
    if let modifier = ModifierKey.byKeyCode[hotkey.keyCode] { return modifier.name }
    let parts: [(UInt64, String)] = [
        (ModifierFlag.control, "⌃"), (ModifierFlag.option, "⌥"), (ModifierFlag.shift, "⇧"), (ModifierFlag.command, "⌘"),
    ]
    let prefix = parts.filter { hotkey.modifiers & $0.0 != 0 }.map(\.1).joined()
    return prefix + keyName(hotkey.keyCode)
}
