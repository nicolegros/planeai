import Foundation
import Observation

/// Queries tmux for live planeai sessions and maps them to SessionInfo.
@Observable
public final class SessionStore {
    public private(set) var sessions: [SessionInfo] = []
    public private(set) var archivedSessions: [SessionInfo] = []
    private let projects: [Project]
    private let tmuxListProvider: () -> String

    private static var archiveFileURL: URL {
        scrollbackDirectory.appendingPathComponent("archived-sessions.json")
    }

    private let persistsArchive: Bool

    public init(projects: [Project], tmuxListProvider: @escaping () -> String = defaultTmuxList, persistsArchive: Bool = true) {
        self.projects = projects
        self.tmuxListProvider = tmuxListProvider
        self.persistsArchive = persistsArchive
        if persistsArchive { loadArchivedSessions() }
    }

    /// Refreshes the session list from tmux.
    public func refresh() {
        let output = tmuxListProvider()
        sessions = output
            .split(separator: "\n")
            .map { String($0).trimmingCharacters(in: .whitespaces) }
            .filter { $0.hasPrefix("planeai-") }
            .compactMap { parseLine($0) }
            .filter { session in !archivedSessions.contains(where: { $0.id == session.id }) }
    }

    /// Sessions grouped by project name, preserving order.
    public var groupedByProject: [String: [SessionInfo]] {
        Dictionary(grouping: sessions, by: \.projectName)
    }

    // MARK: - Lifecycle

    /// Toggles a session between running and completed state.
    public func complete(sessionId: String) {
        guard let idx = sessions.firstIndex(where: { $0.id == sessionId }) else { return }
        let s = sessions[idx]
        let newState: SessionState = s.state == .completed ? .running : .completed
        sessions[idx] = SessionInfo(id: s.id, taskName: s.taskName, branch: s.branch, provider: s.provider, state: newState, projectId: s.projectId, projectName: s.projectName)
    }

    /// Archives a session: removes from active list, adds to archived list.
    public func archive(sessionId: String) {
        if let idx = sessions.firstIndex(where: { $0.id == sessionId }) {
            let s = sessions.remove(at: idx)
            archivedSessions.append(SessionInfo(id: s.id, taskName: s.taskName, branch: s.branch, provider: s.provider, state: .archived, projectId: s.projectId, projectName: s.projectName))
            saveArchivedSessions()
        }
    }

    /// Hard-deletes a session from both active and archived lists.
    public func delete(sessionId: String) {
        sessions.removeAll { $0.id == sessionId }
        let hadArchived = archivedSessions.contains { $0.id == sessionId }
        archivedSessions.removeAll { $0.id == sessionId }
        if hadArchived { saveArchivedSessions() }
    }

    /// Restores an archived session back to the active list.
    public func restore(sessionId: String) {
        guard let idx = archivedSessions.firstIndex(where: { $0.id == sessionId }) else { return }
        let s = archivedSessions.remove(at: idx)
        sessions.append(SessionInfo(id: s.id, taskName: s.taskName, branch: s.branch, provider: s.provider, state: .completed, projectId: s.projectId, projectName: s.projectName))
        saveArchivedSessions()
    }

    // MARK: - Archive persistence

    private func saveArchivedSessions() {
        guard persistsArchive else { return }
        let dir = Self.scrollbackDirectory
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        if let data = try? JSONEncoder().encode(archivedSessions) {
            try? data.write(to: Self.archiveFileURL)
        }
    }

    private func loadArchivedSessions() {
        guard let data = try? Data(contentsOf: Self.archiveFileURL),
              let loaded = try? JSONDecoder().decode([SessionInfo].self, from: data) else { return }
        archivedSessions = loaded
    }

    // MARK: - Pane exit detection

    /// Checks all running sessions for dead panes and auto-completes them.
    public func pollForExitedSessions(tmuxManager: TmuxManager = TmuxManager()) {
        for (idx, session) in sessions.enumerated() where session.state == .running {
            if !tmuxManager.isPaneAlive(sessionName: session.id) {
                sessions[idx] = SessionInfo(id: session.id, taskName: session.taskName, branch: session.branch, provider: session.provider, state: .completed, projectId: session.projectId, projectName: session.projectName)
            }
        }
    }

    // MARK: - Scrollback persistence

    /// The directory where archived scrollback is stored.
    public static var scrollbackDirectory: URL {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("planeai", isDirectory: true)
    }

    /// Persists scrollback for a session to Application Support.
    public func persistScrollback(sessionId: String, tmuxManager: TmuxManager = TmuxManager()) {
        let dir = Self.scrollbackDirectory
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let path = dir.appendingPathComponent("\(sessionId).txt").path
        tmuxManager.captureScrollback(sessionName: sessionId, to: path)
    }

    // MARK: - Parsing

    private func parseLine(_ line: String) -> SessionInfo? {
        let parts = line.split(separator: ":", maxSplits: 1)
        let name = String(parts[0])

        // Format: planeai-<project>-<task>
        let withoutPrefix = String(name.dropFirst("planeai-".count))
        guard let dashIdx = withoutPrefix.firstIndex(of: "-") else { return nil }
        let projectName = String(withoutPrefix[..<dashIdx])
        let taskName = String(withoutPrefix[withoutPrefix.index(after: dashIdx)...])

        let project = projects.first { $0.name == projectName }

        return SessionInfo(
            id: name,
            taskName: taskName,
            branch: taskName,
            provider: project?.defaultProvider ?? "",
            state: .running,
            projectId: project?.id,
            projectName: projectName
        )
    }

    // MARK: - Default tmux provider

    public static func defaultTmuxList() -> String {
        let process = Process()
        let pipe = Pipe()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/env")
        process.arguments = ["tmux", "list-sessions", "-F", "#{session_name}:#{session_path}"]
        process.environment = UserEnvironment.processEnvironment
        process.standardOutput = pipe
        process.standardError = FileHandle.nullDevice
        do {
            try process.run()
            process.waitUntilExit()
        } catch { return "" }
        return String(data: pipe.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8) ?? ""
    }
}
