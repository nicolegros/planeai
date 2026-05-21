import Foundation
import Testing
@testable import PlaneAICore

@Suite("SessionFactory")
struct SessionFactoryTests {

    // MARK: - Provider Detection

    @Test func detectsProvidersOnPATH() throws {
        // "echo" is always on PATH — use it as a stand-in for a known CLI
        let providers = ProviderDetector.detect(knownProviders: [
            ProviderSpec(name: "echo", command: "echo", autoApproveFlag: "--yes"),
            ProviderSpec(name: "nonexistent_xyz_fake", command: "nonexistent_xyz_fake_binary", autoApproveFlag: "--y"),
        ])
        #expect(providers.count == 1)
        #expect(providers[0].name == "echo")
    }

    // MARK: - Session Creation (main branch)

    @Test func createSessionMainBranchLaunchesAgentInRepoRoot() throws {
        let factory = SessionFactory(tmux: TmuxManager())
        let provider = ProviderSpec(name: "echo", command: "echo", autoApproveFlag: "--yes")
        let project = Project(
            name: "myproject",
            repoPath: "/tmp",
            defaultProvider: "echo",
            defaultAutoApprove: false,
            defaultBranchStrategy: .main
        )

        let session = try factory.create(
            project: project,
            taskName: "fix-bug",
            provider: provider,
            branchStrategy: .main,
            autoApprove: false
        )

        // Session name follows convention
        #expect(session.tmuxSession.name == "planeai-myproject-fix-bug")
        // Working directory is the repo root
        #expect(session.tmuxSession.workingDirectory == "/tmp")
        // Agent command is the provider command
        #expect(session.agentCommand == ["echo"])

        // Cleanup
        try? TmuxManager().killSession(named: session.tmuxSession.name)
    }

    // MARK: - Session Creation (worktree)

    @Test func createSessionWorktreeCreatesWorktreeAndTmuxSession() throws {
        // Set up a temporary git repo
        let tmpDir = FileManager.default.temporaryDirectory.appendingPathComponent("planeai-test-\(UUID().uuidString)").path
        try FileManager.default.createDirectory(atPath: tmpDir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(atPath: tmpDir) }

        // Init git repo with an initial commit
        shell("git -C \(tmpDir) init")
        shell("git -C \(tmpDir) commit --allow-empty -m init")

        let factory = SessionFactory(tmux: TmuxManager())
        let provider = ProviderSpec(name: "echo", command: "echo", autoApproveFlag: "--yes")
        let project = Project(
            name: "testrepo",
            repoPath: tmpDir,
            defaultProvider: "echo",
            defaultAutoApprove: false,
            defaultBranchStrategy: .worktree
        )

        let session = try factory.create(
            project: project,
            taskName: "new-feature",
            provider: provider,
            branchStrategy: .worktree,
            autoApprove: false
        )

        // Worktree path is sibling to repo
        let expectedWorktree = (tmpDir as NSString).deletingLastPathComponent + "/testrepo-new-feature"
        #expect(session.tmuxSession.workingDirectory == expectedWorktree)
        #expect(FileManager.default.fileExists(atPath: expectedWorktree))
        #expect(session.tmuxSession.name == "planeai-testrepo-new-feature")

        // Cleanup
        try? TmuxManager().killSession(named: session.tmuxSession.name)
        shell("git -C \(tmpDir) worktree remove \(expectedWorktree)")
    }

    private func shell(_ cmd: String) {
        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/bin/sh")
        p.arguments = ["-c", cmd]
        p.standardOutput = FileHandle.nullDevice
        p.standardError = FileHandle.nullDevice
        try? p.run()
        p.waitUntilExit()
    }

    // MARK: - Auto-approve

    @Test func autoApproveFlagIncludedInAgentCommand() throws {
        let factory = SessionFactory(tmux: TmuxManager())
        let provider = ProviderSpec(name: "echo", command: "echo", autoApproveFlag: "--dangerously-skip-permissions")
        let project = Project(
            name: "proj",
            repoPath: "/tmp",
            defaultProvider: "echo",
            defaultAutoApprove: true,
            defaultBranchStrategy: .main
        )

        let session = try factory.create(
            project: project,
            taskName: "auto-task",
            provider: provider,
            branchStrategy: .main,
            autoApprove: true
        )

        #expect(session.agentCommand == ["echo", "--dangerously-skip-permissions"])

        // Cleanup
        try? TmuxManager().killSession(named: session.tmuxSession.name)
    }

    @Test func autoApproveDisabledOmitsFlag() throws {
        let factory = SessionFactory(tmux: TmuxManager())
        let provider = ProviderSpec(name: "echo", command: "echo", autoApproveFlag: "--dangerously-skip-permissions")
        let project = Project(
            name: "proj2",
            repoPath: "/tmp",
            defaultProvider: "echo",
            defaultAutoApprove: false,
            defaultBranchStrategy: .main
        )

        let session = try factory.create(
            project: project,
            taskName: "no-auto",
            provider: provider,
            branchStrategy: .main,
            autoApprove: false
        )

        #expect(session.agentCommand == ["echo"])

        // Cleanup
        try? TmuxManager().killSession(named: session.tmuxSession.name)
    }

    // MARK: - Task Name Naming

    @Test func taskNameDrivesSessionNameAndWorktreeBranch() throws {
        let tmpDir = FileManager.default.temporaryDirectory.appendingPathComponent("planeai-name-\(UUID().uuidString)").path
        try FileManager.default.createDirectory(atPath: tmpDir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(atPath: tmpDir) }

        shell("git -C \(tmpDir) init")
        shell("git -C \(tmpDir) commit --allow-empty -m init")

        let factory = SessionFactory(tmux: TmuxManager())
        let provider = ProviderSpec(name: "echo", command: "echo", autoApproveFlag: "--yes")
        let project = Project(
            name: "naming",
            repoPath: tmpDir,
            defaultProvider: "echo",
            defaultAutoApprove: false,
            defaultBranchStrategy: .worktree
        )

        let session = try factory.create(
            project: project,
            taskName: "add-auth",
            provider: provider,
            branchStrategy: .worktree,
            autoApprove: false
        )

        // tmux session name uses task name
        #expect(session.tmuxSession.name == "planeai-naming-add-auth")
        // Session stores the task name
        #expect(session.taskName == "add-auth")
        // Worktree path uses task name as branch
        let expectedPath = (tmpDir as NSString).deletingLastPathComponent + "/naming-add-auth"
        #expect(session.tmuxSession.workingDirectory == expectedPath)

        // Cleanup
        try? TmuxManager().killSession(named: session.tmuxSession.name)
        shell("git -C \(tmpDir) worktree remove \(expectedPath)")
    }
}
