import Foundation

// MARK: - Model

public enum BranchStrategy: String, Codable, Sendable {
    case worktree
    case main
}

public struct Project: Identifiable, Codable, Equatable, Sendable {
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
    private let filePath: String
    public private(set) var projects: [Project]

    public init(configDirectory: String = "~/.config/planeai") {
        let expanded = NSString(string: configDirectory).expandingTildeInPath
        self.filePath = (expanded as NSString).appendingPathComponent("projects.json")
        self.projects = []
        load()
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
        let project = Project(
            name: name,
            repoPath: repoPath,
            defaultProvider: defaultProvider,
            defaultAutoApprove: defaultAutoApprove,
            defaultBranchStrategy: defaultBranchStrategy
        )
        projects.append(project)
        save()
        return project
    }

    public func rename(id: UUID, to newName: String) throws {
        guard let idx = projects.firstIndex(where: { $0.id == id }) else {
            throw ProjectStoreError.projectNotFound
        }
        projects[idx].name = newName
        save()
    }

    public func delete(id: UUID) throws {
        guard let idx = projects.firstIndex(where: { $0.id == id }) else {
            throw ProjectStoreError.projectNotFound
        }
        projects.remove(at: idx)
        save()
    }

    // MARK: - Persistence

    private func load() {
        guard let data = FileManager.default.contents(atPath: filePath),
              let decoded = try? JSONDecoder().decode([Project].self, from: data) else { return }
        projects = decoded
    }

    private func save() {
        let dir = (filePath as NSString).deletingLastPathComponent
        try? FileManager.default.createDirectory(atPath: dir, withIntermediateDirectories: true)
        guard let data = try? JSONEncoder().encode(projects) else { return }
        FileManager.default.createFile(atPath: filePath, contents: data)
    }
}

// MARK: - Git Validation

public enum GitRepoValidator {
    /// Validates that the given path is an existing directory containing a git repository.
    public static func validate(path: String) throws {
        var isDir: ObjCBool = false
        guard FileManager.default.fileExists(atPath: path, isDirectory: &isDir), isDir.boolValue else {
            throw ProjectStoreError.invalidGitRepo(path)
        }
        let gitPath = (path as NSString).appendingPathComponent(".git")
        guard FileManager.default.fileExists(atPath: gitPath) else {
            throw ProjectStoreError.invalidGitRepo(path)
        }
    }
}
