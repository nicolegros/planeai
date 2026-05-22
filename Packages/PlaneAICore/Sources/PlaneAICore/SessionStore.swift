import Foundation
import Observation
import GRDB

@Observable
public final class SessionStore {
    public private(set) var sessions: [SessionInfo] = []
    public private(set) var archivedSessions: [SessionInfo] = []
    private let projects: [Project]
    private let tmuxListProvider: () -> String
    private let db: DatabaseQueue?

    public init(projects: [Project], db: DatabaseQueue? = nil, tmuxListProvider: @escaping () -> String = defaultTmuxList) {
        self.projects = projects
        self.db = db
        self.tmuxListProvider = tmuxListProvider
        loadArchivedSessions()
        loadCompletedSessions()
    }

    // MARK: - Refresh from tmux

    public func refresh() {
        let output = tmuxListProvider()
        var refreshed = output
            .split(separator: "\n")
            .map { String($0).trimmingCharacters(in: .whitespaces) }
            .filter { $0.hasPrefix("planeai-") }
            .compactMap { parseLine($0) }
            .filter { session in !archivedSessions.contains(where: { $0.id == session.id }) }

        // Restore completed state from DB
        let completedIds = loadCompletedSessionIds()
        for idx in refreshed.indices where completedIds.contains(refreshed[idx].id) {
            refreshed[idx].state = .completed
            refreshed[idx].completedAt = completedSessions[refreshed[idx].id]
        }

        sessions = refreshed
    }

    public var groupedByProject: [String: [SessionInfo]] {
        Dictionary(grouping: sessions, by: \.projectName)
    }

    // MARK: - Lifecycle

    public func complete(sessionId: String) {
        guard let idx = sessions.firstIndex(where: { $0.id == sessionId }) else { return }
        var s = sessions[idx]
        if s.state == .completed {
            s.state = .running
            s.completedAt = nil
            completedSessions.removeValue(forKey: sessionId)
            deleteSession(sessionId)
        } else {
            s.state = .completed
            s.completedAt = Date()
            completedSessions[sessionId] = s.completedAt!
            saveSession(s)
        }
        sessions[idx] = s
    }

    public func archive(sessionId: String) {
        if let idx = sessions.firstIndex(where: { $0.id == sessionId }) {
            var s = sessions.remove(at: idx)
            s.state = .archived
            s.archivedAt = Date()
            if s.completedAt == nil { s.completedAt = Date() }
            archivedSessions.append(s)
            saveSession(s)
        }
    }

    public func delete(sessionId: String) {
        sessions.removeAll { $0.id == sessionId }
        let hadArchived = archivedSessions.contains { $0.id == sessionId }
        archivedSessions.removeAll { $0.id == sessionId }
        if hadArchived { deleteSession(sessionId) }
    }

    public func restore(sessionId: String) {
        guard let idx = archivedSessions.firstIndex(where: { $0.id == sessionId }) else { return }
        var s = archivedSessions.remove(at: idx)
        s.state = .completed
        s.archivedAt = nil
        sessions.append(s)
        deleteSession(sessionId)
    }

    // MARK: - DB persistence (archived sessions only)

    private func saveSession(_ session: SessionInfo) {
        guard let db else { return }
        do {
            try db.write { db in
                let record = SessionInfo(
                    id: session.id, taskName: session.taskName, branch: session.branch,
                    provider: session.provider, state: session.state, projectId: nil,
                    projectName: session.projectName, createdAt: session.createdAt,
                    completedAt: session.completedAt, archivedAt: session.archivedAt
                )
                try record.save(db)
            }
        } catch {
            NSLog("PlaneAI: saveSession failed for \(session.id): \(error)")
        }
    }

    private func deleteSession(_ id: String) {
        guard let db else { return }
        _ = try? db.write { db in try SessionInfo.deleteOne(db, key: id) }
    }

    private func loadArchivedSessions() {
        guard let db else { return }
        archivedSessions = (try? db.read { db in
            try SessionInfo.filter(Column("state") == SessionState.archived.rawValue).fetchAll(db)
        }) ?? []
    }

    private var completedSessions: [String: Date] = [:]

    private func loadCompletedSessions() {
        guard let db else { return }
        let rows = (try? db.read { db in
            try SessionInfo.filter(Column("state") == SessionState.completed.rawValue).fetchAll(db)
        }) ?? []
        completedSessions = Dictionary(uniqueKeysWithValues: rows.map { ($0.id, $0.completedAt ?? Date()) })
    }

    private func loadCompletedSessionIds() -> Set<String> {
        Set(completedSessions.keys)
    }

    // MARK: - Pane exit detection

    public func pollForExitedSessions(tmuxManager: TmuxManager = TmuxManager()) {
        for (idx, session) in sessions.enumerated() where session.state == .running {
            if !tmuxManager.isPaneAlive(sessionName: session.id) {
                var s = session
                s.state = .completed
                s.completedAt = Date()
                sessions[idx] = s
                completedSessions[s.id] = s.completedAt!
                saveSession(s)
            }
        }
    }

    // MARK: - Scrollback

    public static var scrollbackDirectory: URL {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("planeai", isDirectory: true)
    }

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
