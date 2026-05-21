import SwiftUI
import GhosttyKit

/// Bridges the AppKit TerminalSurfaceView into SwiftUI.
struct TerminalView: NSViewRepresentable {
    let ghosttyApp: ghostty_app_t
    var command: String?
    var isActive: Bool = true
    var focusToken: UInt = 0

    func makeNSView(context: Context) -> TerminalSurfaceView {
        let view = TerminalSurfaceView()
        view.configure(app: ghosttyApp, command: command)
        return view
    }

    func updateNSView(_ nsView: TerminalSurfaceView, context: Context) {
        nsView.isHidden = !isActive
        if isActive {
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.05) {
                nsView.window?.makeFirstResponder(nsView)
                nsView.forceFocus()
            }
        }
    }

    static func dismantleNSView(_ nsView: TerminalSurfaceView, coordinator: ()) {
        nsView.teardown()
    }
}
