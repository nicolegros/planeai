import Foundation
import Testing
@testable import PlaneAICore

@Suite("TmuxSession")
struct TmuxSessionTests {

    @Test func nameFormatIncludesProjectAndSessionId() {
        let session = TmuxSession(project: "myrepo", sessionId: "abc123", workingDirectory: "/tmp")
        #expect(session.name == "planeai-myrepo-abc123")
    }

    @Test func customNamePreserved() {
        let session = TmuxSession(name: "planeai-foo-bar", workingDirectory: "/tmp")
        #expect(session.name == "planeai-foo-bar")
    }

    @Test func attachCommandFormatsCorrectly() {
        let manager = TmuxManager()
        let session = TmuxSession(project: "proj", sessionId: "s1", workingDirectory: "/tmp")
        let cmd = manager.attachCommand(for: session)
        #expect(cmd == ["tmux", "attach-session", "-t", "planeai-proj-s1"])
    }
}

@Suite("TmuxManager Integration", .enabled(if: tmuxAvailable()))
struct TmuxManagerIntegrationTests {
    let manager = TmuxManager()
    let testSession = TmuxSession(project: "test", sessionId: "unit\(ProcessInfo.processInfo.processIdentifier)", workingDirectory: "/tmp")

    @Test func createAndKillSession() throws {
        // Create
        try manager.createSession(testSession)
        #expect(manager.hasSession(named: testSession.name))

        // List includes it
        let sessions = manager.listSessions()
        #expect(sessions.contains { $0.name == testSession.name })

        // Duplicate throws
        #expect(throws: TmuxError.sessionAlreadyExists(testSession.name)) {
            try manager.createSession(testSession)
        }

        // Kill
        try manager.killSession(named: testSession.name)
        #expect(!manager.hasSession(named: testSession.name))
    }

    @Test func killNonexistentSessionThrows() {
        #expect(throws: TmuxError.sessionNotFound("planeai-ghost-none")) {
            try manager.killSession(named: "planeai-ghost-none")
        }
    }

    @Test func validateTmuxAvailable() throws {
        try manager.validateTmuxAvailable()
    }
}

/// Helper to check if tmux is available for integration tests.
private func tmuxAvailable() -> Bool {
    let process = Process()
    process.executableURL = URL(fileURLWithPath: "/usr/bin/env")
    process.arguments = ["which", "tmux"]
    process.standardOutput = FileHandle.nullDevice
    process.standardError = FileHandle.nullDevice
    try? process.run()
    process.waitUntilExit()
    return process.terminationStatus == 0
}
