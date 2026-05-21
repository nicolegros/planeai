import Foundation
import Observation

/// Queries tmux for live planeai sessions and maps them to SessionInfo.
@Observable
public final class SessionStore {
    public private(set) var sessions: [SessionInfo] = []
    private let projects: [Project]
    private let tmuxListProvider: () -> String

    public init(projects: [Project], tmuxListProvider: @escaping () -> String = defaultTmuxList) {
        self.projects = projects
    self.tmuxListProvider = tmuxListProvider
    }

    /// Refreshes the session list from tmux.
    public func refresh() {
        let output = tmuxListProvider()
        sessions = output
            .split(separator: "\n")
            .map { String($0).trimmingCharacters(in: .whitespaces) }
            .filter { $0.hasPrefix("planeai-") }
            .compactMap { parseLine($0) }
    }

    /// Sessions grouped by project name, preserving order.
    public var groupedByProject: [String: [SessionInfo]] {
        Dictionary(grouping: sessions, by: \.projectName)
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
