import Foundation

public enum PinTarget: Codable, Equatable, Sendable {
    case herdr
    case app(bundleId: String, name: String)

    public var label: String {
        switch self {
        case .herdr: return "Herdr"
        case let .app(_, name): return name
        }
    }
}

public enum Route: Equatable, Sendable {
    case herdrServer
    case herdrApp
    case keys
}

public func route(executable: String, runsHerdr: Bool) -> Route {
    if executable.contains("/Herdr.app/") { return .herdrApp }
    return runsHerdr ? .herdrServer : .keys
}

private func object(_ json: Data) -> Any? {
    try? JSONSerialization.jsonObject(with: json)
}

private func field(_ value: Any?, _ path: [String]) -> Any? {
    path.reduce(value) { ($0 as? [String: Any])?[$1] }
}

public func herdrPaneID(_ json: Data) -> String? {
    field(object(json), ["result", "pane", "pane_id"]) as? String
}

public func herdrAgent(inPane pane: String, agents json: Data) -> String? {
    let agents = field(object(json), ["result", "agents"]) as? [[String: Any]]
    return agents?.first { $0["pane_id"] as? String == pane }?["name"] as? String
}

private func terminal(ofPane pane: String, in value: Any?) -> String? {
    if let fields = value as? [String: Any] {
        if fields["pane"] as? String == pane { return fields["terminal"] as? String }
        return fields.values.lazy.compactMap { terminal(ofPane: pane, in: $0) }.first
    }
    return (value as? [Any])?.lazy.compactMap { terminal(ofPane: pane, in: $0) }.first
}

public func herdrAppTerminal(status: Data, listing: Data) -> String? {
    let state = object(status) as? [String: Any]
    let focused = (state?["focused_pane"] as? String).flatMap { terminal(ofPane: $0, in: object(listing)) }
    return focused ?? state?["agent"] as? String
}

public func herdrExecutable(home: String, exists: (String) -> Bool) -> String? {
    ["\(home)/.local/bin/herdr", "/opt/homebrew/bin/herdr", "/usr/local/bin/herdr"].first(where: exists)
}

public struct RunningProcess: Equatable, Sendable {
    public let pid: Int32
    public let parent: Int32
    public let name: String
}

public func processes(fromPS output: String) -> [RunningProcess] {
    output.split(separator: "\n").compactMap { line in
        let (first, afterFirst) = word(String(line))
        let (second, command) = word(afterFirst)
        guard let pid = Int32(first), let parent = Int32(second), !command.isEmpty else { return nil }
        return RunningProcess(pid: pid, parent: parent, name: URL(fileURLWithPath: command).lastPathComponent)
    }
}

private func word(_ text: String) -> (String, String) {
    let trimmed = text.drop { $0 == " " }
    let head = trimmed.prefix { $0 != " " }
    return (String(head), String(trimmed.dropFirst(head.count).drop { $0 == " " }))
}

public func runs(_ name: String, under root: Int32, in all: [RunningProcess]) -> Bool {
    let parents = Dictionary(all.map { ($0.pid, $0.parent) }, uniquingKeysWith: { first, _ in first })
    return all.filter { $0.name == name }.contains { descends($0.pid, from: root, parents: parents, depth: 64) }
}

private func descends(_ pid: Int32, from root: Int32, parents: [Int32: Int32], depth: Int) -> Bool {
    guard depth > 0, pid > 1 else { return false }
    guard pid != root else { return true }
    return parents[pid].map { descends($0, from: root, parents: parents, depth: depth - 1) } ?? false
}
