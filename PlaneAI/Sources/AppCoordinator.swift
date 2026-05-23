import SwiftUI
import PlaneAICore

@Observable
final class AppCoordinator {
    // MARK: - Navigation State

    var activeSessionId: String?
    var selectedSessionId: String?
    var sessionHistory: [String] = []
    var activatedSessions: [ActivatedSession] = []
    var sidebarVisibility: NavigationSplitViewVisibility = .automatic
    var focusToken: UInt = 0

    // MARK: - Palette Presentation

    var showCommandPalette = false
    var showNewSessionPalette = false

    // MARK: - Confirmation Dialogs

    var showArchiveWorktreePrompt = false
    var pendingArchiveId: String?
    var showClosePaneConfirmation = false

    // MARK: - Dependencies

    let projectStore: ProjectStore
    var sessionStore: SessionStore
    var selectedProjectID: UUID?
    private let dbManager: DatabaseManager?
    private var _projectsVersion = 0

    var projects: [Project] {
        _ = _projectsVersion
        return projectStore.projects
    }

    init(projectStore: ProjectStore, sessionStore: SessionStore, dbManager: DatabaseManager?) {
        self.projectStore = projectStore
        self.sessionStore = sessionStore
        self.dbManager = dbManager
    }

    func addProject(name: String, repoPath: String, defaultProvider: String, defaultAutoApprove: Bool, defaultBranchStrategy: BranchStrategy) throws {
        try projectStore.add(name: name, repoPath: repoPath, defaultProvider: defaultProvider, defaultAutoApprove: defaultAutoApprove, defaultBranchStrategy: defaultBranchStrategy)
        _projectsVersion += 1
    }

    func deleteProject(id: UUID) throws {
        try projectStore.delete(id: id)
        if selectedProjectID == id { selectedProjectID = nil }
        _projectsVersion += 1
    }

    func renameProject(id: UUID, to newName: String) throws {
        try projectStore.rename(id: id, to: newName)
        _projectsVersion += 1
    }

    // MARK: - Navigation Intents

    func activateSession(_ session: SessionInfo) {
        if let current = activeSessionId, current != session.id {
            sessionHistory.removeAll { $0 == current }
            sessionHistory.append(current)
        }
        selectedSessionId = session.id
        let tmux = TmuxSession(name: session.id, workingDirectory: "/")
        let cmd = TmuxManager().attachCommand(for: tmux).joined(separator: " ")
        if !activatedSessions.contains(where: { $0.id == session.id }) {
            activatedSessions.append(ActivatedSession(id: session.id, command: cmd))
        }
        activeSessionId = session.id
        focusToken &+= 1
    }

    func jumpToSession(at index: Int) {
        let all = groupedSessions.flatMap(\.sessions)
        guard index < all.count else { return }
        activateSession(all[index])
    }

    func toggleSidebar() {
        withAnimation {
            sidebarVisibility = sidebarVisibility == .detailOnly ? .automatic : .detailOnly
        }
    }

    // MARK: - Session Lifecycle Intents

    func completeSession(id: String) {
        sessionStore.complete(sessionId: id)
    }

    func archiveSession(id: String) {
        sessionStore.persistScrollback(sessionId: id)
        let worktreeExists = worktreeExists(for: id)
        if worktreeExists {
            pendingArchiveId = id
            showArchiveWorktreePrompt = true
        } else {
            sessionStore.archive(sessionId: id)
            if selectedSessionId == id { selectedSessionId = nil }
        }
    }

    func confirmArchive(removeWorktree shouldRemove: Bool) {
        guard let id = pendingArchiveId else { return }
        if shouldRemove {
            removeWorktree(for: id)
        }
        sessionStore.archive(sessionId: id)
        if selectedSessionId == id { selectedSessionId = nil }
        pendingArchiveId = nil
    }

    func cancelArchive() {
        pendingArchiveId = nil
    }

    func deleteSession(id: String) {
        let tmux = TmuxManager()
        try? tmux.killSession(named: id)
        let scrollbackPath = SessionStore.scrollbackDirectory.appendingPathComponent("\(id).txt")
        try? FileManager.default.removeItem(at: scrollbackPath)
        removeWorktree(for: id)
        sessionStore.delete(sessionId: id)
    }

    func restoreSession(id: String) {
        sessionStore.restore(sessionId: id)
    }

    // MARK: - Session Creation

    func sessionCreated(_ session: Session) {
        let cmd = TmuxManager().attachCommand(for: session.tmuxSession).joined(separator: " ")
        if let current = activeSessionId, current != session.tmuxSession.name {
            sessionHistory.removeAll { $0 == current }
            sessionHistory.append(current)
        }
        if !activatedSessions.contains(where: { $0.id == session.tmuxSession.name }) {
            activatedSessions.append(ActivatedSession(id: session.tmuxSession.name, command: cmd))
        }
        activeSessionId = session.tmuxSession.name
        selectedSessionId = session.tmuxSession.name
        focusToken &+= 1
        refreshSessions()
    }

    // MARK: - Grouped Sessions

    var groupedSessions: [(project: String, sessions: [SessionInfo])] {
        let grouped = Dictionary(grouping: sessionStore.sessions, by: \.projectName)
        var result: [(project: String, sessions: [SessionInfo])] = []
        for project in projects {
            result.append((project: project.name, sessions: grouped[project.name] ?? []))
        }
        for key in grouped.keys.sorted() where !projects.contains(where: { $0.name == key }) {
            result.append((project: key, sessions: grouped[key]!))
        }
        return result
    }

    // MARK: - Refresh

    func refreshSessions() {
        sessionStore = SessionStore(projects: projects, db: dbManager?.dbQueue)
        sessionStore.refresh()
    }

    // MARK: - Helpers

    func shouldConfirmDelete(id: String) -> Bool {
        guard let project = projects.first(where: { p in id.hasPrefix("planeai-\(p.name)-") }) else { return false }
        let task = String(id.dropFirst("planeai-\(project.name)-".count))
        let worktreePath = (project.repoPath as NSString).deletingLastPathComponent + "/\(project.name)-\(task)"
        return TmuxManager.hasUnmergedChanges(at: worktreePath)
    }

    // MARK: - MRU Tab Switcher

    var showTabSwitcher = false
    var tabSwitcherIndex = 0

    var mruSessionList: [SessionInfo] {
        let allSessions = groupedSessions.flatMap(\.sessions)
        var ordered: [SessionInfo] = []
        if let currentId = activeSessionId,
           let current = allSessions.first(where: { $0.id == currentId }) {
            ordered.append(current)
        }
        for id in sessionHistory.reversed() {
            if let session = allSessions.first(where: { $0.id == id }),
               !ordered.contains(where: { $0.id == id }) {
                ordered.append(session)
            }
        }
        for session in allSessions where !ordered.contains(where: { $0.id == session.id }) {
            ordered.append(session)
        }
        return ordered
    }

    func switchToPreviousSession() {
        let mru = mruSessionList
        guard mru.count > 1 else { return }
        if showTabSwitcher {
            tabSwitcherIndex = (tabSwitcherIndex + 1) % mru.count
        } else {
            showTabSwitcher = true
            tabSwitcherIndex = 1
        }
    }

    func switchToNextSession() {
        let mru = mruSessionList
        guard mru.count > 1 else { return }
        if showTabSwitcher {
            tabSwitcherIndex = (tabSwitcherIndex - 1 + mru.count) % mru.count
        } else {
            showTabSwitcher = true
            tabSwitcherIndex = mru.count - 1
        }
    }

    func confirmTabSwitch() {
        let mru = mruSessionList
        guard tabSwitcherIndex < mru.count else { return }
        showTabSwitcher = false
        activateSession(mru[tabSwitcherIndex])
    }

    func cancelTabSwitch() {
        showTabSwitcher = false
    }

    // MARK: - Pane Management

    func splitActivePane(direction: PaneDirection) {
        guard let sessionId = activeSessionId else { return }
        let workDir = workingDirectory(for: sessionId)
        DispatchQueue.global(qos: .userInitiated).async {
            let tmux = TmuxManager()
            try? tmux.splitPane(sessionName: sessionId, direction: direction, workingDirectory: workDir)
        }
    }

    func focusAdjacentPane(direction: PaneDirection) {
        guard let sessionId = activeSessionId else { return }
        DispatchQueue.global(qos: .userInitiated).async {
            let tmux = TmuxManager()
            try? tmux.focusPane(sessionName: sessionId, direction: direction)
        }
    }

    func closeActivePane() {
        guard let sessionId = activeSessionId else { return }
        DispatchQueue.global(qos: .userInitiated).async { [self] in
            let tmux = TmuxManager()
            let panes = tmux.listPanes(sessionName: sessionId)
            guard let active = panes.first(where: \.isActive) else { return }

            if active.id == panes.first?.id && panes.count > 1 {
                DispatchQueue.main.async { self.showClosePaneConfirmation = true }
                return
            }

            if panes.count <= 1 {
                try? tmux.killSession(named: sessionId)
                DispatchQueue.main.async { self.removeSession(sessionId) }
            } else {
                try? tmux.closePane(sessionName: sessionId, paneId: active.id)
            }
        }
    }

    func forceCloseActivePane() {
        guard let sessionId = activeSessionId else { return }
        DispatchQueue.global(qos: .userInitiated).async { [self] in
            let tmux = TmuxManager()
            let panes = tmux.listPanes(sessionName: sessionId)
            guard let active = panes.first(where: \.isActive) else { return }

            if panes.count <= 1 {
                try? tmux.killSession(named: sessionId)
                DispatchQueue.main.async { self.removeSession(sessionId) }
            } else {
                try? tmux.closePane(sessionName: sessionId, paneId: active.id)
            }
        }
    }

    func focusSidebar() {
        sidebarVisibility = .all
        DispatchQueue.main.async { [self] in
            guard let window = NSApp.keyWindow else { return }
            func findListView(_ view: NSView) -> NSView? {
                if view is NSOutlineView || view is NSTableView { return view }
                for sub in view.subviews {
                    if let found = findListView(sub) { return found }
                }
                return nil
            }
            if let listView = findListView(window.contentView!),
               window.firstResponder !== listView {
                window.makeFirstResponder(listView)
            } else {
                self.focusToken &+= 1
            }
        }
    }

    // MARK: - Private Helpers

    private func removeSession(_ sessionId: String) {
        completeSession(id: sessionId)
        activatedSessions.removeAll { $0.id == sessionId }
        if activeSessionId == sessionId {
            activeSessionId = nil
            selectedSessionId = nil
        }
        refreshSessions()
    }

    private func workingDirectory(for sessionId: String) -> String {
        if let project = projects.first(where: { p in sessionId.hasPrefix("planeai-\(p.name)-") }) {
            let task = String(sessionId.dropFirst("planeai-\(project.name)-".count))
            let worktreePath = (project.repoPath as NSString).deletingLastPathComponent + "/\(project.name)-\(task)"
            return FileManager.default.fileExists(atPath: worktreePath) ? worktreePath : project.repoPath
        }
        return NSHomeDirectory()
    }

    private func worktreeExists(for id: String) -> Bool {
        guard let project = projects.first(where: { p in id.hasPrefix("planeai-\(p.name)-") }) else { return false }
        let task = String(id.dropFirst("planeai-\(project.name)-".count))
        let worktreePath = (project.repoPath as NSString).deletingLastPathComponent + "/\(project.name)-\(task)"
        return FileManager.default.fileExists(atPath: worktreePath)
    }

    private func removeWorktree(for id: String) {
        guard let project = projects.first(where: { p in id.hasPrefix("planeai-\(p.name)-") }) else { return }
        let task = String(id.dropFirst("planeai-\(project.name)-".count))
        let worktreePath = (project.repoPath as NSString).deletingLastPathComponent + "/\(project.name)-\(task)"
        TmuxManager().removeWorktree(at: worktreePath)
    }
}
