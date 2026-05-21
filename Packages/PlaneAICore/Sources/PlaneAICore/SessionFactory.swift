import Foundation
import os

private let logger = Logger(subsystem: "ca.nicolegros.planeai", category: "SessionFactory")

/// A running agent session backed by a tmux session.
public struct Session: Sendable {
    public let tmuxSession: TmuxSession
    public let agentCommand: [String]
    public let projectId: UUID
    public let taskName: String
}

/// Creates and launches agent sessions.
public final class SessionFactory: Sendable {
    private let tmux: TmuxManager

    public init(tmux: TmuxManager) {
        self.tmux = tmux
    }

    public func create(
        project: Project,
        taskName: String,
        provider: ProviderSpec,
        branchStrategy: BranchStrategy,
        autoApprove: Bool
    ) throws -> Session {
        let sanitizedTaskName = taskName.replacingOccurrences(of: " ", with: "-")
        let workingDirectory: String
        switch branchStrategy {
        case .main:
            workingDirectory = project.repoPath
        case .worktree:
            workingDirectory = try createWorktree(project: project, branch: sanitizedTaskName)
        }

        let sessionName = "planeai-\(project.name)-\(sanitizedTaskName)"
        let tmuxSession = TmuxSession(name: sessionName, workingDirectory: workingDirectory)
        try tmux.createSession(tmuxSession)

        var command = [provider.command] + provider.arguments
        if autoApprove {
            command.append(provider.autoApproveFlag)
        }

        // Send the agent command into the tmux session
        let cmdString = command.joined(separator: " ")
        sendKeys(session: sessionName, keys: cmdString)

        return Session(
            tmuxSession: tmuxSession,
            agentCommand: command,
            projectId: project.id,
            taskName: taskName
        )
    }

    private func createWorktree(project: Project, branch: String) throws -> String {
        let parentDir = (project.repoPath as NSString).deletingLastPathComponent
        let worktreePath = "\(parentDir)/\(project.name)-\(branch)"

        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/env")
        process.arguments = ["git", "-C", project.repoPath, "worktree", "add", "-b", branch, worktreePath]
        process.environment = UserEnvironment.processEnvironment
        process.standardOutput = FileHandle.nullDevice
        process.standardError = Pipe()
        try process.run()
        process.waitUntilExit()

        guard process.terminationStatus == 0 else {
            let stderr = String(data: (process.standardError as! Pipe).fileHandleForReading.readDataToEndOfFile(), encoding: .utf8) ?? ""
            throw TmuxError.commandFailed("git worktree add failed: \(stderr)")
        }
        return worktreePath
    }

    private func sendKeys(session: String, keys: String) {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/env")
        process.arguments = ["tmux", "send-keys", "-t", session, keys, "Enter"]
        process.environment = UserEnvironment.processEnvironment
        process.standardOutput = FileHandle.nullDevice
        process.standardError = FileHandle.nullDevice
        try? process.run()
        process.waitUntilExit()
    }
}
