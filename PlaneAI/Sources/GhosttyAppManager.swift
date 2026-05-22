import AppKit
import GhosttyKit

/// Manages the ghostty app lifecycle — config, runtime callbacks, and tick loop.
@Observable
final class GhosttyAppManager {
    private(set) var app: ghostty_app_t?
    private var tickTimer: Timer?

    enum State { case loading, ready, error }
    private(set) var state: State = .loading

    init() {
        // Set resources dir for shell integration and terminfo
        if let resourcesPath = Bundle.main.resourcePath {
            let ghosttyResources = resourcesPath + "/ghostty"
            setenv("GHOSTTY_RESOURCES_DIR", ghosttyResources, 1)
        }

        guard ghostty_init(0, nil) == GHOSTTY_SUCCESS else {
            NSLog("PlaneAI: ghostty_init failed")
            state = .error
            return
        }

        // Create config
        guard let config = ghostty_config_new() else {
            state = .error
            return
        }
        ghostty_config_load_default_files(config)
        ghostty_config_finalize(config)

        // Create runtime config with callbacks
        var runtime = ghostty_runtime_config_s()
        runtime.userdata = Unmanaged.passUnretained(self).toOpaque()
        runtime.supports_selection_clipboard = true
        runtime.wakeup_cb = { ud in
            guard let ud else { return }
            let mgr = Unmanaged<GhosttyAppManager>.fromOpaque(ud).takeUnretainedValue()
            DispatchQueue.main.async { mgr.tick() }
        }
        runtime.action_cb = { _, _, _ in false }
        runtime.read_clipboard_cb = { ud, loc, state in
            guard let ud else { return false }
            guard let pasteboard = NSPasteboard.general.string(forType: .string) else { return false }
            let view = Unmanaged<TerminalSurfaceView>.fromOpaque(ud).takeUnretainedValue()
            guard let surface = view.exposedSurface else { return false }
            pasteboard.withCString { ptr in
                ghostty_surface_complete_clipboard_request(surface, ptr, state, false)
            }
            return true
        }
        runtime.confirm_read_clipboard_cb = nil
        runtime.write_clipboard_cb = { _, _, content, len, _ in
            guard let content, len > 0 else { return }
            let str = String(cString: content.pointee.data)
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(str, forType: .string)
        }
        runtime.close_surface_cb = { _, _ in }

        guard let ghosttyApp = ghostty_app_new(&runtime, config) else {
            ghostty_config_free(config)
            state = .error
            return
        }

        self.app = ghosttyApp
        ghostty_config_free(config)
        state = .ready

        // Start tick timer for rendering
        tickTimer = Timer.scheduledTimer(withTimeInterval: 1.0 / 120.0, repeats: true) { [weak self] _ in
            self?.tick()
        }
    }

    func tick() {
        guard let app else { return }
        ghostty_app_tick(app)
    }

    /// The currently focused surface, set by TerminalSurfaceView on focus.
    weak var focusedSurfaceView: TerminalSurfaceView?

    func currentSurface() -> ghostty_surface_t? {
        focusedSurfaceView?.exposedSurface
    }

    deinit {
        tickTimer?.invalidate()
        if let app { ghostty_app_free(app) }
    }
}
