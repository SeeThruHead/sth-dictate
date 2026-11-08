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

enum Herdr {
    static func send(_ text: String) -> String? {
        guard let herdr = herdrExecutable(home: NSHomeDirectory(), exists: FileManager.default.isExecutableFile(atPath:)) else { return "Herdr is not installed in ~/.local/bin or Homebrew" }
        guard let pane = herdrPaneID(run(herdr, ["pane", "current"]).output) else { return "Herdr has no focused pane" }
        let sent = run(herdr, ["pane", "send-text", pane, text])
        return sent.status == 0 ? nil : "Herdr could not send to pane \(pane)"
    }

    private static func run(_ path: String, _ arguments: [String]) -> (status: Int32, output: Data) {
        let process = Process()
        let pipe = Pipe()
        process.executableURL = URL(fileURLWithPath: path)
        process.arguments = arguments
        process.environment = ProcessInfo.processInfo.environment.filter { $0.key != "HERDR_PANE_ID" }
        process.standardOutput = pipe
        process.standardError = FileHandle.nullDevice
        guard (try? process.run()) != nil else { return (-1, Data()) }
        let output = pipe.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        return (process.terminationStatus, output)
    }
}

enum PinStore {
    private static let key = "pinToHerdr"

    static func load() -> Bool { UserDefaults.standard.bool(forKey: key) }

    static func save(_ pinned: Bool) { UserDefaults.standard.set(pinned, forKey: key) }
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
