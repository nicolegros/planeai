import AppKit
import GhosttyKit

/// NSView subclass that hosts a ghostty terminal surface.
final class TerminalSurfaceView: NSView {
    private var ghosttyApp: ghostty_app_t?
    private var surface: ghostty_surface_t?
    private var command: String?

    override var acceptsFirstResponder: Bool { true }
    override var isFlipped: Bool { true }

    func configure(app: ghostty_app_t, command: String? = nil) {
        self.ghosttyApp = app
        self.command = command
        self.wantsLayer = true // Ensure Metal layer is available
    }

    private func createSurface() {
        guard let app = ghosttyApp, surface == nil else { return }
        var cfg = ghostty_surface_config_new()
        cfg.platform_tag = GHOSTTY_PLATFORM_MACOS
        cfg.platform.macos.nsview = Unmanaged.passUnretained(self).toOpaque()
        cfg.userdata = Unmanaged.passUnretained(self).toOpaque()
        cfg.scale_factor = Double(window?.backingScaleFactor ?? 2.0)
        cfg.font_size = 0
        cfg.context = GHOSTTY_SURFACE_CONTEXT_WINDOW

        // If a command is specified, pass it to ghostty
        let commandCString = command.flatMap { strdup($0) }
        cfg.command = UnsafePointer(commandCString)

        surface = ghostty_surface_new(app, &cfg)

        commandCString.map { free($0) }

        if surface == nil {
            NSLog("PlaneAI: failed to create ghostty surface")
        }
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        if window != nil && surface == nil {
            createSurface()
        }
        guard let surface else { return }
        let scale = window?.backingScaleFactor ?? 2.0
        ghostty_surface_set_content_scale(surface, scale, scale)
        updateSurfaceSize()
        ghostty_surface_set_focus(surface, true)
        window?.makeFirstResponder(self)
    }

    override func setFrameSize(_ newSize: NSSize) {
        super.setFrameSize(newSize)
        updateSurfaceSize()
    }

    private func updateSurfaceSize() {
        guard let surface, frame.width > 0, frame.height > 0 else { return }
        let scale = window?.backingScaleFactor ?? 2.0
        let w = UInt32(frame.width * scale)
        let h = UInt32(frame.height * scale)
        ghostty_surface_set_size(surface, w, h)
    }

    override func becomeFirstResponder() -> Bool {
        if let surface { ghostty_surface_set_focus(surface, true) }
        return super.becomeFirstResponder()
    }

    override func resignFirstResponder() -> Bool {
        if let surface { ghostty_surface_set_focus(surface, false) }
        return super.resignFirstResponder()
    }

    // MARK: - Keyboard Input

    override func keyDown(with event: NSEvent) {
        guard let surface else { super.keyDown(with: event); return }

        var key = ghostty_input_key_s()
        key.action = GHOSTTY_ACTION_PRESS
        key.mods = Self.translateModifiers(event.modifierFlags)
        key.consumed_mods = GHOSTTY_MODS_NONE
        key.keycode = UInt32(event.keyCode)
        key.composing = false
        key.text = nil
        key.unshifted_codepoint = 0

        if ghostty_surface_key(surface, key) {
            return
        }

        // Pass text through if ghostty didn't consume the key
        if let chars = event.characters, !chars.isEmpty {
            chars.withCString { ptr in
                ghostty_surface_text(surface, ptr, UInt(chars.utf8.count))
            }
        }
    }

    override func keyUp(with event: NSEvent) {
        guard let surface else { super.keyUp(with: event); return }
        var key = ghostty_input_key_s()
        key.action = GHOSTTY_ACTION_RELEASE
        key.mods = Self.translateModifiers(event.modifierFlags)
        key.consumed_mods = GHOSTTY_MODS_NONE
        key.keycode = UInt32(event.keyCode)
        key.composing = false
        key.text = nil
        key.unshifted_codepoint = 0
        _ = ghostty_surface_key(surface, key)
    }

    override func flagsChanged(with event: NSEvent) {
        guard let surface else { return }
        var key = ghostty_input_key_s()
        key.action = GHOSTTY_ACTION_PRESS
        key.mods = Self.translateModifiers(event.modifierFlags)
        key.consumed_mods = GHOSTTY_MODS_NONE
        key.keycode = UInt32(event.keyCode)
        key.composing = false
        key.text = nil
        key.unshifted_codepoint = 0
        _ = ghostty_surface_key(surface, key)
    }

    // MARK: - Mouse Input

    override func mouseDown(with event: NSEvent) {
        guard let surface else { return }
        window?.makeFirstResponder(self)
        _ = ghostty_surface_mouse_button(surface,
            GHOSTTY_MOUSE_PRESS, GHOSTTY_MOUSE_LEFT,
            Self.translateModifiers(event.modifierFlags))
    }

    override func mouseUp(with event: NSEvent) {
        guard let surface else { return }
        _ = ghostty_surface_mouse_button(surface,
            GHOSTTY_MOUSE_RELEASE, GHOSTTY_MOUSE_LEFT,
            Self.translateModifiers(event.modifierFlags))
    }

    override func mouseMoved(with event: NSEvent) {
        reportMousePos(event)
    }

    override func mouseDragged(with event: NSEvent) {
        reportMousePos(event)
    }

    override func scrollWheel(with event: NSEvent) {
        guard let surface else { return }
        // ghostty_input_scroll_mods_t is an int32 packed struct
        let scrollMods: ghostty_input_scroll_mods_t = 0
        ghostty_surface_mouse_scroll(surface,
            event.scrollingDeltaX, event.scrollingDeltaY,
            scrollMods)
    }

    private func reportMousePos(_ event: NSEvent) {
        guard let surface else { return }
        let pt = convert(event.locationInWindow, from: nil)
        let scale = window?.backingScaleFactor ?? 2.0
        ghostty_surface_mouse_pos(surface,
            pt.x * scale, pt.y * scale,
            Self.translateModifiers(event.modifierFlags))
    }

    // MARK: - Helpers

    private static func translateModifiers(_ flags: NSEvent.ModifierFlags) -> ghostty_input_mods_e {
        var raw: UInt32 = 0
        if flags.contains(.shift) { raw |= UInt32(GHOSTTY_MODS_SHIFT.rawValue) }
        if flags.contains(.control) { raw |= UInt32(GHOSTTY_MODS_CTRL.rawValue) }
        if flags.contains(.option) { raw |= UInt32(GHOSTTY_MODS_ALT.rawValue) }
        if flags.contains(.command) { raw |= UInt32(GHOSTTY_MODS_SUPER.rawValue) }
        return ghostty_input_mods_e(rawValue: raw)
    }

    // MARK: - Cleanup

    func teardown() {
        if let surface {
            ghostty_surface_free(surface)
            self.surface = nil
        }
    }

    deinit {
        teardown()
    }
}
