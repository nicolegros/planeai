import SwiftUI
import GhosttyKit

/// Bridges the AppKit TerminalSurfaceView into SwiftUI.
struct TerminalView: NSViewRepresentable {
    let ghosttyApp: ghostty_app_t

    func makeNSView(context: Context) -> TerminalSurfaceView {
        let view = TerminalSurfaceView()
        view.configure(app: ghosttyApp)
        return view
    }

    func updateNSView(_ nsView: TerminalSurfaceView, context: Context) {}

    static func dismantleNSView(_ nsView: TerminalSurfaceView, coordinator: ()) {
        nsView.teardown()
    }
}
