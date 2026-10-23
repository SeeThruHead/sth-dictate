import Foundation

public struct DictateSettings: Codable, Equatable, Sendable {
    public let endpoint: String
    public let model: String
    public let hotkey: Hotkey
    public let hotkeyLabel: String

    public init(endpoint: String, model: String, hotkey: Hotkey, hotkeyLabel: String) {
        self.endpoint = endpoint
        self.model = model
        self.hotkey = hotkey
        self.hotkeyLabel = hotkeyLabel
    }

    public static let initial = DictateSettings(endpoint: "http://192.168.0.149:8000/v1", model: "", hotkey: .rightCommand, hotkeyLabel: "Right ⌘")

    public func with(endpoint: String? = nil, model: String? = nil, hotkey: Hotkey? = nil, hotkeyLabel: String? = nil) -> DictateSettings {
        DictateSettings(endpoint: endpoint ?? self.endpoint, model: model ?? self.model, hotkey: hotkey ?? self.hotkey, hotkeyLabel: hotkeyLabel ?? self.hotkeyLabel)
    }
}

public enum EndpointError: Error, Equatable {
    case invalid(String)
}

public func transcriptionURL(_ endpoint: String) -> Result<URL, EndpointError> {
    let trimmed = endpoint.trimmingCharacters(in: .whitespacesAndNewlines)
    let base = trimmed.hasSuffix("/") ? String(trimmed.dropLast()) : trimmed
    let full = base.hasSuffix("/audio/transcriptions") ? base : base + "/audio/transcriptions"
    guard let url = URL(string: full), let scheme = url.scheme, ["http", "https"].contains(scheme), url.host != nil else {
        return .failure(.invalid("Endpoint must be an http(s) URL, like http://host:8000/v1"))
    }
    return .success(url)
}

public struct Multipart: Equatable {
    public let contentType: String
    public let body: Data
}

public func multipart(boundary: String, audio: Data, model: String) -> Multipart {
    let modelPart = model.isEmpty ? [] : [field(boundary, name: "model", value: model)]
    let parts = modelPart + [
        field(boundary, name: "response_format", value: "json"),
        Data("--\(boundary)\r\nContent-Disposition: form-data; name=\"file\"; filename=\"audio.wav\"\r\nContent-Type: audio/wav\r\n\r\n".utf8) + audio + Data("\r\n".utf8),
        Data("--\(boundary)--\r\n".utf8),
    ]
    return Multipart(contentType: "multipart/form-data; boundary=\(boundary)", body: parts.reduce(Data(), +))
}

private func field(_ boundary: String, name: String, value: String) -> Data {
    Data("--\(boundary)\r\nContent-Disposition: form-data; name=\"\(name)\"\r\n\r\n\(value)\r\n".utf8)
}

private struct TranscriptionResponse: Decodable {
    let text: String
}

public func transcriptText(status: Int, body: Data) -> Result<String, EndpointError> {
    guard (200..<300).contains(status) else {
        let detail = String(data: body.prefix(300), encoding: .utf8) ?? ""
        return .failure(.invalid("Server answered \(status): \(detail)"))
    }
    guard let decoded = try? JSONDecoder().decode(TranscriptionResponse.self, from: body) else {
        return .failure(.invalid("Server answered without a JSON \"text\" field"))
    }
    return .success(decoded.text)
}
