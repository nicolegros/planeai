import Foundation
import GRDB

// MARK: - Model

public enum BranchStrategy: String, Codable, Sendable {
    case worktree
    case main
}

public struct Project: Identifiable, Codable, Equatable, Hashable, Sendable, FetchableRecord, PersistableRecord {
    public static let databaseTableName = "project"

    public let id: UUID
    public var name: String
    public let repoPath: String
    public var defaultProvider: String
    public var defaultAutoApprove: Bool
    public var defaultBranchStrategy: BranchStrategy

    public init(
        id: UUID = UUID(),
        name: String,
        repoPath: String,
        defaultProvider: String,
        defaultAutoApprove: Bool,
        defaultBranchStrategy: BranchStrategy
    ) {
        self.id = id
        self.name = name
        self.repoPath = repoPath
        self.defaultProvider = defaultProvider
        self.defaultAutoApprove = defaultAutoApprove
        self.defaultBranchStrategy = defaultBranchStrategy
    }
}

// MARK: - Errors

public enum ProjectStoreError: Error, Equatable {
    case projectNotFound
    case duplicateName(String)
    case invalidGitRepo(String)
}

// MARK: - Store

public final class ProjectStore {
    private let db: DatabaseQueue
    public private(set) var projects: [Project]

    public init(db: DatabaseQueue) {
        self.db = db
        self.projects = (try? db.read { db in try Project.fetchAll(db) }) ?? []
    }

    @discardableResult
    public func add(
        name: String,
        repoPath: String,
        defaultProvider: String,
        defaultAutoApprove: Bool,
        defaultBranchStrategy: BranchStrategy
    ) throws -> Project {
        guard !projects.contains(where: { $0.name == name }) else {
            throw ProjectStoreError.duplicateName(name)
        }
        let expandedPath = NSString(string: repoPath).expandingTildeInPath
        let project = Project(
            name: name,
            repoPath: expandedPath,
            defaultProvider: defaultProvider,
            defaultAutoApprove: defaultAutoApprove,
            defaultBranchStrategy: defaultBranchStrategy
        )
        try db.write { db in try project.insert(db) }
        projects.append(project)
        return project
    }

    public func rename(id: UUID, to newName: String) throws {
        guard var project = projects.first(where: { $0.id == id }) else {
            throw ProjectStoreError.projectNotFound
        }
        project.name = newName
        try db.write { db in try project.update(db) }
        if let idx = projects.firstIndex(where: { $0.id == id }) {
            projects[idx] = project
        }
    }

    public func delete(id: UUID) throws {
        guard let project = projects.first(where: { $0.id == id }) else {
            throw ProjectStoreError.projectNotFound
        }
        _ = try db.write { db in try project.delete(db) }
        projects.removeAll { $0.id == id }
    }
}

// MARK: - Git Validation

public enum GitRepoValidator {
    public static func validate(path: String) throws {
        let expanded = NSString(string: path).expandingTildeInPath
        var isDir: ObjCBool = false
        guard FileManager.default.fileExists(atPath: expanded, isDirectory: &isDir), isDir.boolValue else {
            throw ProjectStoreError.invalidGitRepo(path)
        }
        let gitPath = (expanded as NSString).appendingPathComponent(".git")
        guard FileManager.default.fileExists(atPath: gitPath) else {
            throw ProjectStoreError.invalidGitRepo(path)
        }
    }
}
