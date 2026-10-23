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
        let previous = board.string(forType: .string)
        board.clearContents()
        board.setString(text, forType: .string)
        let ours = board.changeCount
        postCommandV()
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) {
            guard board.changeCount == ours, let previous else { return }
            board.clearContents()
            board.setString(previous, forType: .string)
        }
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

enum SettingsStore {
    private static let key = "settings"

    static func load() -> DictateSettings {
        UserDefaults.standard.data(forKey: key).flatMap { try? JSONDecoder().decode(DictateSettings.self, from: $0) } ?? .initial
    }

    static func save(_ settings: DictateSettings) {
        (try? JSONEncoder().encode(settings)).map { UserDefaults.standard.set($0, forKey: key) }
    }
}
