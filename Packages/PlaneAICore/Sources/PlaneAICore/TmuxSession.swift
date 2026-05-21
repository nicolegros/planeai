import Foundation

/// Manages tmux sessions for planeai. Each session is named `planeai-<project>-<sessionId>`.
public struct TmuxSession: Sendable {
    public let name: String
    public let workingDirectory: String

    public init(project: String, sessionId: String, workingDirectory: String) {
        self.name = "planeai-\(project)-\(sessionId)"
        self.workingDirectory = workingDirectory
    }

    public init(name: String, workingDirectory: String) {
        self.name = name
        self.workingDirectory = workingDirectory
    }
}

/// Errors from tmux operations.
public enum TmuxError: Error, Equatable {
    case tmuxNotFound
    case sessionAlreadyExists(String)
    case sessionNotFound(String)
    case commandFailed(String)
}

/// Manages tmux session lifecycle. All methods shell out to the `tmux` binary.
public final class TmuxManager: Sendable {
    private let tmuxPath: String

    public init(tmuxPath: String = "/usr/bin/env") {
        self.tmuxPath = tmuxPath
    }

    // MARK: - Validation

    /// Validates tmux is available on PATH.
    public func validateTmuxAvailable() throws {
        let result = run(["which", "tmux"])
        if result.status != 0 {
            throw TmuxError.tmuxNotFound
        }
    }

    // MARK: - CRUD

    /// Creates a new tmux session with status bar disabled and prefix key remapped to unreachable combo.
    @discardableResult
    public func createSession(_ session: TmuxSession) throws -> TmuxSession {
        // Check if session already exists
        if hasSession(named: session.name) {
            throw TmuxError.sessionAlreadyExists(session.name)
        }

        let result = run([
            "tmux", "new-session",
            "-d",                          // detached
            "-s", session.name,            // session name
            "-c", session.workingDirectory  // working directory
        ])

        guard result.status == 0 else {
            throw TmuxError.commandFailed(result.stderr)
        }

        // Disable status bar and remap prefix to unreachable combo
        _ = run(["tmux", "set-option", "-t", session.name, "status", "off"])
        _ = run(["tmux", "set-option", "-t", session.name, "prefix", "None"])
        _ = run(["tmux", "set-option", "-t", session.name, "prefix2", "None"])

        return session
    }

    /// Lists all existing planeai-* tmux sessions.
    public func listSessions() -> [TmuxSession] {
        let result = run(["tmux", "list-sessions", "-F", "#{session_name}:#{session_path}"])
        guard result.status == 0 else { return [] }

        return result.stdout
            .split(separator: "\n")
            .compactMap { line -> TmuxSession? in
                let parts = line.split(separator: ":", maxSplits: 1)
                let name = String(parts[0])
                guard name.hasPrefix("planeai-") else { return nil }
                let path = parts.count > 1 ? String(parts[1]) : "/"
                return TmuxSession(name: name, workingDirectory: path)
            }
    }

    /// Returns whether a session with the given name exists.
    public func hasSession(named name: String) -> Bool {
        let result = run(["tmux", "has-session", "-t", name])
        return result.status == 0
    }

    /// Kills/destroys a tmux session.
    public func killSession(named name: String) throws {
        guard hasSession(named: name) else {
            throw TmuxError.sessionNotFound(name)
        }
        let result = run(["tmux", "kill-session", "-t", name])
        guard result.status == 0 else {
            throw TmuxError.commandFailed(result.stderr)
        }
    }

    /// Returns the tmux attach command arguments for a given session (for ghostty surface rendering).
    public func attachCommand(for session: TmuxSession) -> [String] {
        ["tmux", "attach-session", "-t", session.name]
    }

    // MARK: - Private

    private struct RunResult: Sendable {
        let stdout: String
        let stderr: String
        let status: Int32
    }

    private func run(_ arguments: [String]) -> RunResult {
        let process = Process()
        let stdoutPipe = Pipe()
        let stderrPipe = Pipe()

        process.executableURL = URL(fileURLWithPath: "/usr/bin/env")
        process.arguments = arguments
        process.standardOutput = stdoutPipe
        process.standardError = stderrPipe

        do {
            try process.run()
            process.waitUntilExit()
        } catch {
            return RunResult(stdout: "", stderr: error.localizedDescription, status: -1)
        }

        let stdout = String(data: stdoutPipe.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8) ?? ""
        let stderr = String(data: stderrPipe.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8) ?? ""
        return RunResult(stdout: stdout, stderr: stderr, status: process.terminationStatus)
    }
}
