import Cocoa
import SwiftUI

struct DragHelperView: View {
    let permission: Permission
    let onDone: () -> Void

    private var icon: NSImage { NSWorkspace.shared.icon(forFile: Bundle.main.bundlePath) }

    var body: some View {
        VStack(spacing: 10) {
            Image(nsImage: icon)
                .resizable()
                .frame(width: 64, height: 64)
                .onDrag { NSItemProvider(object: Bundle.main.bundleURL as NSURL) }
            Text("Drag SthDictate into the \(permission.title) list")
                .font(.headline)
            Text("Then switch it on. This closes by itself once it's granted.")
                .font(.caption)
                .foregroundStyle(.secondary)
            Button("Close", action: onDone)
        }
        .padding(18)
        .frame(width: 300)
        .onReceive(Timer.publish(every: 1, on: .main, in: .common).autoconnect()) { _ in
            if permission.isGranted { onDone() }
        }
    }
}

final class DragHelper {
    static let shared = DragHelper()

    private var panel: NSPanel?

    func show(for permission: Permission) {
        close()
        let panel = NSPanel(contentRect: .zero, styleMask: [.titled, .closable, .utilityWindow], backing: .buffered, defer: false)
        let hosting = NSHostingView(rootView: DragHelperView(permission: permission, onDone: { [weak self] in self?.close() }))
        panel.title = "SthDictate"
        panel.contentView = hosting
        panel.setContentSize(hosting.fittingSize)
        panel.level = .floating
        panel.isReleasedWhenClosed = false
        panel.hidesOnDeactivate = false
        place(panel)
        panel.orderFrontRegardless()
        self.panel = panel
    }

    func close() {
        panel?.close()
        panel = nil
    }

    private func place(_ panel: NSPanel) {
        guard let screen = NSScreen.main?.visibleFrame else { return panel.center() }
        panel.setFrameOrigin(NSPoint(x: screen.maxX - panel.frame.width - 40, y: screen.minY + 80))
    }
}
