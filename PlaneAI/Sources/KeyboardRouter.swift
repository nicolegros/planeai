import AppKit
import PlaneAICore

/// Maps NSEvent key events to AppCoordinator intents.
/// Owns the event monitor lifecycle — install/uninstall via `start()`/`stop()`.
final class KeyboardRouter {
    private weak var coordinator: AppCoordinator?
    private var monitor: Any?

    init(coordinator: AppCoordinator) {
        self.coordinator = coordinator
    }

    func start() {
        guard monitor == nil else { return }
        monitor = NSEvent.addLocalMonitorForEvents(matching: [.keyDown, .flagsChanged]) { [weak self] event in
            self?.handle(event) ?? event
        }
    }

    func stop() {
        if let monitor {
            NSEvent.removeMonitor(monitor)
        }
        monitor = nil
    }

    deinit { stop() }

    // MARK: - Event Dispatch

    private func handle(_ event: NSEvent) -> NSEvent? {
        guard let coordinator else { return event }

        // Tab switcher active — handle confirm/cancel/navigate
        if coordinator.showTabSwitcher {
            if event.type == .flagsChanged && !event.modifierFlags.contains(.control) {
                coordinator.confirmTabSwitch()
                return nil
            }
            if event.type == .keyDown {
                switch event.keyCode {
                case 36: coordinator.confirmTabSwitch(); return nil  // Enter
                case 53: coordinator.cancelTabSwitch(); return nil   // Escape
                default: break
                }
            }
        }

        guard event.type == .keyDown else { return event }

        let cmd = event.modifierFlags.contains(.command)
        let opt = event.modifierFlags.contains(.option)
        let shift = event.modifierFlags.contains(.shift)
        let ctrl = event.modifierFlags.contains(.control)

        // Cmd+W — close pane (intercept before system Close Window)
        if cmd && !shift && !opt && event.keyCode == 13 {
            coordinator.closeActivePane()
            return nil
        }

        // Cmd+K — toggle command palette
        if cmd && !shift && !opt && event.keyCode == 40 {
            coordinator.showCommandPalette.toggle()
            return nil
        }

        // Cmd+Opt+h/j/k/l — vim pane navigation
        if cmd && opt {
            switch event.keyCode {
            case 4:  coordinator.focusAdjacentPane(direction: .left); return nil
            case 38: coordinator.focusAdjacentPane(direction: .down); return nil
            case 40: coordinator.focusAdjacentPane(direction: .up); return nil
            case 37: coordinator.focusAdjacentPane(direction: .right); return nil
            default: break
            }
        }

        // Ctrl+Tab / Ctrl+Shift+Tab — MRU session switching
        if ctrl && event.keyCode == 48 {
            if shift {
                coordinator.switchToNextSession()
            } else {
                coordinator.switchToPreviousSession()
            }
            return nil
        }

        return event
    }
}
