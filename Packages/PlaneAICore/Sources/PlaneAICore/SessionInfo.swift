import Foundation

/// The state of a running agent session.
public enum SessionState: String, Sendable, Equatable, Codable {
    case running
    case completed
    case needsAttention
    case archived
}

/// A snapshot of a live session for display in the sidebar.
public struct SessionInfo: Identifiable, Sendable, Equatable, Codable {
    public let id: String          // tmux session name
    public let taskName: String
    public let branch: String
    public let provider: String
    public let state: SessionState
    public let projectId: UUID?
    public let projectName: String

    public init(
        id: String,
        taskName: String,
        branch: String,
        provider: String,
        state: SessionState,
        projectId: UUID?,
        projectName: String
    ) {
        self.id = id
        self.taskName = taskName
        self.branch = branch
        self.provider = provider
        self.state = state
        self.projectId = projectId
        self.projectName = projectName
    }
}
