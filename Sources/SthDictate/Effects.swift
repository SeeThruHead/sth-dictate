import Cocoa
import DictateCore

enum Transcriber {
    static func transcribe(_ audio: Data, settings: DictateSettings) async -> Result<String, EndpointError> {
        switch transcriptionURL(settings.endpoint) {
        case let .failure(error):
            return .failure(error)
        case let .success(url):
            let form = multipart(boundary: "sthdictate-\(UUID().uuidString)", audio: audio, model: settings.model)
            var request = URLRequest(url: url, timeoutInterval: 60)
            request.httpMethod = "POST"
            request.setValue(form.contentType, forHTTPHeaderField: "Content-Type")
            do {
                let (body, response) = try await URLSession.shared.upload(for: request, from: form.body)
                return transcriptText(status: (response as? HTTPURLResponse)?.statusCode ?? 0, body: body)
            } catch {
                return .failure(.invalid("Could not reach \(url.host ?? "the server"): \(error.localizedDescription)"))
            }
        }
    }

    static func test(settings: DictateSettings) async -> String {
        let started = Date()
        let result = await transcribe(wav(silence(seconds: 1)), settings: settings)
        let ms = Int(Date().timeIntervalSince(started) * 1000)
        switch result {
        case .success: return "Endpoint OK, answered in \(ms) ms"
        case let .failure(.invalid(message)): return message
        }
    }
}

enum Paster {
    static func paste(_ text: String) {
        let board = NSPasteboard.general
        board.clearContents()
        board.setString(text, forType: .string)
        postCommandV()
    }

    static var hasPermission: Bool { AXIsProcessTrusted() }

    static func requestPermission() {
        AXIsProcessTrustedWithOptions(["AXTrustedCheckOptionPrompt": true] as CFDictionary)
    }

    static func pressReturn() {
        let source = CGEventSource(stateID: .combinedSessionState)
        let down = CGEvent(keyboardEventSource: source, virtualKey: 36, keyDown: true)
        let up = CGEvent(keyboardEventSource: source, virtualKey: 36, keyDown: false)
        down?.post(tap: .cghidEventTap)
        up?.post(tap: .cghidEventTap)
    }

    private static func postCommandV() {
        let source = CGEventSource(stateID: .combinedSessionState)
        let down = CGEvent(keyboardEventSource: source, virtualKey: 9, keyDown: true)
        let up = CGEvent(keyboardEventSource: source, virtualKey: 9, keyDown: false)
        down?.flags = .maskCommand
        up?.flags = .maskCommand
        down?.post(tap: .cghidEventTap)
        up?.post(tap: .cghidEventTap)
    }
}

struct Pin: Codable, Equatable {
    var on = false
    var chosen: PinTarget?

    var target: PinTarget? { on ? (chosen ?? .herdr) : nil }
}

enum PinStore {
    private static let key = "pin"

    static func load() -> Pin {
        UserDefaults.standard.data(forKey: key).flatMap { try? JSONDecoder().decode(Pin.self, from: $0) } ?? Pin()
    }

    static func save(_ pin: Pin) {
        (try? JSONEncoder().encode(pin)).map { UserDefaults.standard.set($0, forKey: key) }
    }
}

enum Deliver {
    private static let home = NSHomeDirectory()

    static func send(_ text: String, to target: PinTarget, lastHerdr: Route) -> String? {
        switch target {
        case .herdr:
            return send(text, by: lastHerdr, app: nil)
        case let .app(bundleId, name):
            guard let app = NSRunningApplication.runningApplications(withBundleIdentifier: bundleId).first else { return "\(name) is not running: pin another app" }
            return send(text, by: routeFor(app), app: app)
        }
    }

    static func routeFor(_ app: NSRunningApplication) -> Route {
        let executable = app.executableURL?.path ?? ""
        let all = processes(fromPS: run("/bin/ps", ["-axo", "pid=,ppid=,comm="]).text)
        return route(executable: executable, runsHerdr: runs("herdr", under: app.processIdentifier, in: all))
    }

    private static func send(_ text: String, by route: Route, app: NSRunningApplication?) -> String? {
        switch route {
        case .herdrServer: return toHerdrServer(text)
        case .herdrApp: return toHerdrApp(text)
        case .keys: return app.map { keys(text, into: $0) } ?? "No app is pinned"
        }
    }

    private static func tool(_ name: String) -> String? {
        let path = "\(home)/.local/bin/\(name)"
        return FileManager.default.isExecutableFile(atPath: path) ? path : nil
    }

    private static func toHerdrServer(_ text: String) -> String? {
        guard let herdr = herdrExecutable(home: home, exists: FileManager.default.isExecutableFile(atPath:)) else { return "Herdr is not installed in ~/.local/bin or Homebrew" }
        guard let pane = herdrPaneID(run(herdr, ["pane", "current"]).output) else { return "Herdr has no focused pane" }
        if let name = herdrAgent(inPane: pane, agents: run(herdr, ["agent", "list"]).output) {
            return run(herdr, ["agent", "prompt", name, text]).status == 0 ? nil : "Herdr could not prompt \(name)"
        }
        guard run(herdr, ["pane", "send-text", pane, text]).status == 0 else { return "Herdr could not send to pane \(pane)" }
        return run(herdr, ["pane", "send-keys", pane, "enter"]).status == 0 ? nil : "Herdr could not press Enter in pane \(pane)"
    }

    private static func toHerdrApp(_ text: String) -> String? {
        guard let app = tool("herdr-app"), let hc = tool("hc") else { return "herdr-app and hc are not in ~/.local/bin" }
        let status = run(app, ["status"])
        guard status.status == 0 else { return "The Herdr app is not open" }
        guard let terminal = herdrAppTerminal(status: status.output, listing: run(app, ["ls"]).output) else { return "The Herdr app has no focused terminal" }
        return run(hc, ["send", terminal, text]).status == 0 ? nil : "hc could not send to \(terminal)"
    }

    private static func keys(_ text: String, into app: NSRunningApplication) -> String? {
        let board = NSPasteboard.general
        board.clearContents()
        board.setString(text, forType: .string)
        Keys.post(virtualKey: 9, flags: .maskCommand, to: app.processIdentifier)
        usleep(80_000)
        Keys.post(virtualKey: 36, flags: [], to: app.processIdentifier)
        return nil
    }

    private static func run(_ path: String, _ arguments: [String]) -> (status: Int32, output: Data, text: String) {
        let process = Process()
        let pipe = Pipe()
        process.executableURL = URL(fileURLWithPath: path)
        process.arguments = arguments
        process.environment = ProcessInfo.processInfo.environment.filter { $0.key != "HERDR_PANE_ID" }
        process.standardOutput = pipe
        process.standardError = FileHandle.nullDevice
        guard (try? process.run()) != nil else { return (-1, Data(), "") }
        let output = pipe.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        return (process.terminationStatus, output, String(decoding: output, as: UTF8.self))
    }
}

enum Keys {
    static func post(virtualKey: CGKeyCode, flags: CGEventFlags, to pid: pid_t) {
        let source = CGEventSource(stateID: .privateState)
        let down = CGEvent(keyboardEventSource: source, virtualKey: virtualKey, keyDown: true)
        let up = CGEvent(keyboardEventSource: source, virtualKey: virtualKey, keyDown: false)
        down?.flags = flags
        up?.flags = flags
        down?.postToPid(pid)
        up?.postToPid(pid)
    }
}

enum SettingsStore {
    private static let key = "settings"

    static func load() -> DictateSettings {
        UserDefaults.standard.data(forKey: key).flatMap { try? JSONDecoder().decode(DictateSettings.self, from: $0) } ?? .initial
    }

    static func save(_ settings: DictateSettings) {
        (try? JSONEncoder().encode(settings)).map { UserDefaults.standard.set($0, forKey: key) }
    }
}
