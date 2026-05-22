import Foundation

/// Tracks the pane layout for a single session.
public final class PaneLayout {
    public let sessionName: String
    public private(set) var primaryPaneId: String
    public var paneIds: [String]

    public init(sessionName: String, primaryPaneId: String) {
        self.sessionName = sessionName
        self.primaryPaneId = primaryPaneId
        self.paneIds = [primaryPaneId]
    }

    public var secondaryPaneIds: [String] {
        paneIds.filter { $0 != primaryPaneId }
    }

    public func isPrimary(_ paneId: String) -> Bool {
        paneId == primaryPaneId
    }

    /// Syncs pane list from tmux.
    public func refresh(tmux: TmuxManager = TmuxManager()) {
        let panes = tmux.listPanes(sessionName: sessionName)
        paneIds = panes.map(\.id)
    }

    /// Records a new pane was added.
    public func addPane(_ paneId: String) {
        if !paneIds.contains(paneId) {
            paneIds.append(paneId)
        }
    }

    /// Records a pane was removed.
    public func removePane(_ paneId: String) {
        paneIds.removeAll { $0 == paneId }
    }
}

/// Manages PaneLayouts across all active sessions.
public final class PaneLayoutStore {
    public private(set) var layouts: [String: PaneLayout] = [:]

    public init() {}

    /// Gets or creates a layout for a session.
    public func layout(for sessionName: String, tmux: TmuxManager = TmuxManager()) -> PaneLayout {
        if let existing = layouts[sessionName] { return existing }
        let panes = tmux.listPanes(sessionName: sessionName)
        let primaryId = panes.first?.id ?? ""
        let layout = PaneLayout(sessionName: sessionName, primaryPaneId: primaryId)
        layout.paneIds = panes.map(\.id)
        layouts[sessionName] = layout
        return layout
    }

    /// Removes layout tracking for a session.
    public func removeLayout(for sessionName: String) {
        layouts.removeValue(forKey: sessionName)
    }
}
