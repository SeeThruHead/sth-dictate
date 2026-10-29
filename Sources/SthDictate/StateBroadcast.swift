import AppKit

final class StateBroadcast {
    private let name: String
    private let file: URL
    private var last: [String: String]?

    init(_ name: String) {
        self.name = name
        let folder = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent(name)
        try? FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        file = folder.appendingPathComponent("state.json")
        NotificationCenter.default.addObserver(forName: NSApplication.willTerminateNotification, object: nil, queue: nil) { [weak self] _ in
            self?.publish(["state": "stopped"])
        }
    }

    func publish(_ fields: [String: String]) {
        guard fields != last else { return }
        last = fields
        let stamped = fields.merging([
            "pid": String(ProcessInfo.processInfo.processIdentifier),
            "updatedAt": ISO8601DateFormatter().string(from: Date()),
        ]) { current, _ in current }
        if let data = try? JSONSerialization.data(withJSONObject: stamped, options: [.sortedKeys]) {
            try? data.write(to: file, options: .atomic)
        }
        DistributedNotificationCenter.default().postNotificationName(
            Notification.Name("\(name).state"),
            object: nil,
            userInfo: stamped,
            deliverImmediately: true
        )
    }
}
