import SwiftUI
import GhosttyKit

/// Bridges the AppKit TerminalSurfaceView into SwiftUI.
struct TerminalView: NSViewRepresentable {
    let ghosttyApp: ghostty_app_t
    var command: String?
    var isActive: Bool = true
    var focusToken: UInt = 0
    weak var appManager: GhosttyAppManager?

    func makeNSView(context: Context) -> TerminalSurfaceView {
        let view = TerminalSurfaceView()
        view.appManager = appManager
        view.configure(app: ghosttyApp, command: command)
        return view
    }

    func updateNSView(_ nsView: TerminalSurfaceView, context: Context) {
        let shouldBeHidden = !isActive
        if nsView.isHidden != shouldBeHidden {
            nsView.isHidden = shouldBeHidden
        }
        if isActive && focusToken != context.coordinator.lastFocusToken {
            context.coordinator.lastFocusToken = focusToken
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.05) {
                nsView.window?.makeFirstResponder(nsView)
                nsView.forceFocus()
            }
        }
    }

    func makeCoordinator() -> Coordinator { Coordinator(focusToken: focusToken) }

    class Coordinator {
        var lastFocusToken: UInt
        init(focusToken: UInt = 0) { self.lastFocusToken = focusToken }
    }

    static func dismantleNSView(_ nsView: TerminalSurfaceView, coordinator: Coordinator) {
        nsView.teardown()
    }
}
