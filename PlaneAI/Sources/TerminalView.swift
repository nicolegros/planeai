import SwiftUI
import GhosttyKit

/// Bridges the AppKit TerminalSurfaceView into SwiftUI.
struct TerminalView: NSViewRepresentable {
    let ghosttyApp: ghostty_app_t
    var command: String?

    init(ghosttyApp: ghostty_app_t, command: String? = nil) {
        self.ghosttyApp = ghosttyApp
        self.command = command
    }

    func makeNSView(context: Context) -> TerminalSurfaceView {
        let view = TerminalSurfaceView()
        view.configure(app: ghosttyApp, command: command)
        return view
    }

    func updateNSView(_ nsView: TerminalSurfaceView, context: Context) {}

    static func dismantleNSView(_ nsView: TerminalSurfaceView, coordinator: ()) {
        nsView.teardown()
    }
}
