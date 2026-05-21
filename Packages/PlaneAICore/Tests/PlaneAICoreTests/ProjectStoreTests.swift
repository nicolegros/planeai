import Foundation
import Testing
@testable import PlaneAICore

@Suite("Project Model")
struct ProjectModelTests {

    @Test func projectStoresAllFields() {
        let project = Project(
            name: "my-app",
            repoPath: "/Users/me/repos/my-app",
            defaultProvider: "claude-code",
            defaultAutoApprove: true,
            defaultBranchStrategy: .worktree
        )
        #expect(project.name == "my-app")
        #expect(project.repoPath == "/Users/me/repos/my-app")
        #expect(project.defaultProvider == "claude-code")
        #expect(project.defaultAutoApprove == true)
        #expect(project.defaultBranchStrategy == .worktree)
        #expect(project.id != UUID())
    }

    @Test func projectEncodesAndDecodes() throws {
        let project = Project(
            name: "test",
            repoPath: "/tmp/repo",
            defaultProvider: "kiro",
            defaultAutoApprove: false,
            defaultBranchStrategy: .main
        )
        let data = try JSONEncoder().encode(project)
        let decoded = try JSONDecoder().decode(Project.self, from: data)
        #expect(decoded == project)
    }

    @Test func branchStrategyValues() {
        #expect(BranchStrategy.worktree.rawValue == "worktree")
        #expect(BranchStrategy.main.rawValue == "main")
    }
}

@Suite("ProjectStore")
struct ProjectStoreTests {
    let tempDir: String

    init() {
        tempDir = NSTemporaryDirectory() + "planeai-test-\(UUID().uuidString)"
        try? FileManager.default.createDirectory(atPath: tempDir, withIntermediateDirectories: true)
    }

    private func makeStore() -> ProjectStore {
        ProjectStore(configDirectory: tempDir)
    }

    @Test func startsEmpty() {
        let store = makeStore()
        #expect(store.projects.isEmpty)
    }

    @Test func addProject() throws {
        let store = makeStore()
        let project = try store.add(
            name: "my-app",
            repoPath: "/tmp/repo",
            defaultProvider: "claude-code",
            defaultAutoApprove: false,
            defaultBranchStrategy: .worktree
        )
        #expect(store.projects.count == 1)
        #expect(store.projects[0] == project)
    }

    @Test func renameProject() throws {
        let store = makeStore()
        let project = try store.add(
            name: "old-name",
            repoPath: "/tmp/repo",
            defaultProvider: "claude-code",
            defaultAutoApprove: false,
            defaultBranchStrategy: .main
        )
        try store.rename(id: project.id, to: "new-name")
        #expect(store.projects[0].name == "new-name")
    }

    @Test func deleteProject() throws {
        let store = makeStore()
        let project = try store.add(
            name: "to-delete",
            repoPath: "/tmp/repo",
            defaultProvider: "claude-code",
            defaultAutoApprove: false,
            defaultBranchStrategy: .main
        )
        try store.delete(id: project.id)
        #expect(store.projects.isEmpty)
    }

    @Test func persistsAcrossInstances() throws {
        let store1 = makeStore()
        try store1.add(
            name: "persisted",
            repoPath: "/tmp/repo",
            defaultProvider: "kiro",
            defaultAutoApprove: true,
            defaultBranchStrategy: .worktree
        )

        let store2 = makeStore()
        #expect(store2.projects.count == 1)
        #expect(store2.projects[0].name == "persisted")
    }

    @Test func deleteNonexistentThrows() {
        let store = makeStore()
        #expect(throws: ProjectStoreError.projectNotFound) {
            try store.delete(id: UUID())
        }
    }

    @Test func renameNonexistentThrows() {
        let store = makeStore()
        #expect(throws: ProjectStoreError.projectNotFound) {
            try store.rename(id: UUID(), to: "x")
        }
    }

    @Test func duplicateNameThrows() throws {
        let store = makeStore()
        try store.add(
            name: "dup",
            repoPath: "/tmp/repo1",
            defaultProvider: "claude-code",
            defaultAutoApprove: false,
            defaultBranchStrategy: .main
        )
        #expect(throws: ProjectStoreError.duplicateName("dup")) {
            try store.add(
                name: "dup",
                repoPath: "/tmp/repo2",
                defaultProvider: "claude-code",
                defaultAutoApprove: false,
                defaultBranchStrategy: .main
            )
        }
    }
}

@Suite("Git Repo Validation")
struct GitRepoValidationTests {

    @Test func rejectsNonexistentPath() {
        #expect(throws: ProjectStoreError.invalidGitRepo("/nonexistent/path/xyz")) {
            try GitRepoValidator.validate(path: "/nonexistent/path/xyz")
        }
    }

    @Test func rejectsNonGitDirectory() throws {
        let tmpDir = NSTemporaryDirectory() + "planeai-nogit-\(UUID().uuidString)"
        try FileManager.default.createDirectory(atPath: tmpDir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(atPath: tmpDir) }

        #expect(throws: ProjectStoreError.invalidGitRepo(tmpDir)) {
            try GitRepoValidator.validate(path: tmpDir)
        }
    }

    @Test func acceptsGitDirectory() throws {
        let tmpDir = NSTemporaryDirectory() + "planeai-git-\(UUID().uuidString)"
        try FileManager.default.createDirectory(atPath: tmpDir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(atPath: tmpDir) }

        // Initialize a git repo
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/env")
        process.arguments = ["git", "init", tmpDir]
        process.standardOutput = FileHandle.nullDevice
        process.standardError = FileHandle.nullDevice
        try process.run()
        process.waitUntilExit()

        // Should not throw
        try GitRepoValidator.validate(path: tmpDir)
    }
}
