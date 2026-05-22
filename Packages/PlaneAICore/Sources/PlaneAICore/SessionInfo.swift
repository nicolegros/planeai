import Foundation
import GRDB

public enum SessionState: String, Sendable, Equatable, Codable {
    case running
    case completed
    case needsAttention
    case archived
}

public struct SessionInfo: Identifiable, Sendable, Equatable, Codable, FetchableRecord, PersistableRecord {
    public static let databaseTableName = "session"

    public let id: String          // tmux session name
    public let taskName: String
    public let branch: String
    public let provider: String
    public var state: SessionState
    public let projectId: UUID?
    public let projectName: String
    public var createdAt: Date
    public var completedAt: Date?
    public var archivedAt: Date?

    public init(
        id: String,
        taskName: String,
        branch: String,
        provider: String,
        state: SessionState,
        projectId: UUID?,
        projectName: String,
        createdAt: Date = Date(),
        completedAt: Date? = nil,
        archivedAt: Date? = nil
    ) {
        self.id = id
        self.taskName = taskName
        self.branch = branch
        self.provider = provider
        self.state = state
        self.projectId = projectId
        self.projectName = projectName
        self.createdAt = createdAt
        self.completedAt = completedAt
        self.archivedAt = archivedAt
    }
}
