import Foundation
import Testing
@testable import PlaneAICore

@Suite("TmuxManager Pane Operations", .enabled(if: tmuxPaneTestsAvailable()), .serialized)
struct TmuxPaneTests {
    let manager = TmuxManager()
    let sessionName = "planeai-test-pane\(ProcessInfo.processInfo.processIdentifier)"

    init() {
        // Ensure clean state
        try? manager.killSession(named: sessionName)
    }

    private func createTestSession() throws {
        let session = TmuxSession(name: sessionName, workingDirectory: "/tmp")
        try manager.createSession(session)
    }

    private func cleanup() {
        try? manager.killSession(named: sessionName)
    }

    @Test func listPanesReturnsSinglePaneAfterCreate() throws {
        try createTestSession()
        defer { cleanup() }

        let panes = manager.listPanes(sessionName: sessionName)
        #expect(panes.count == 1)
        #expect(panes.first != nil)
    }

    @Test func splitRightCreatesTwoPanes() throws {
        try createTestSession()
        defer { cleanup() }

        try manager.splitPane(sessionName: sessionName, direction: .right, workingDirectory: "/tmp")
        let panes = manager.listPanes(sessionName: sessionName)
        #expect(panes.count == 2)
    }

    @Test func splitDownCreatesTwoPanes() throws {
        try createTestSession()
        defer { cleanup() }

        try manager.splitPane(sessionName: sessionName, direction: .down, workingDirectory: "/tmp")
        let panes = manager.listPanes(sessionName: sessionName)
        #expect(panes.count == 2)
    }

    @Test func focusPaneChangesActivePane() throws {
        try createTestSession()
        defer { cleanup() }

        try manager.splitPane(sessionName: sessionName, direction: .right, workingDirectory: "/tmp")
        let panes = manager.listPanes(sessionName: sessionName)
        guard panes.count == 2 else {
            Issue.record("Expected 2 panes")
            return
        }

        try manager.focusPane(sessionName: sessionName, direction: .left)
        let active = manager.activePaneId(sessionName: sessionName)
        #expect(active == panes[0].id)
    }

    @Test func closePaneReducesCount() throws {
        try createTestSession()
        defer { cleanup() }

        try manager.splitPane(sessionName: sessionName, direction: .right, workingDirectory: "/tmp")
        let panes = manager.listPanes(sessionName: sessionName)
        guard panes.count == 2 else {
            Issue.record("Expected 2 panes")
            return
        }

        try manager.closePane(sessionName: sessionName, paneId: panes[1].id)
        let remaining = manager.listPanes(sessionName: sessionName)
        #expect(remaining.count == 1)
    }

    @Test func closingLastPaneKillsSession() throws {
        try createTestSession()
        defer { cleanup() }

        let panes = manager.listPanes(sessionName: sessionName)
        guard let pane = panes.first else {
            Issue.record("Expected at least 1 pane")
            return
        }

        try manager.closePane(sessionName: sessionName, paneId: pane.id)
        #expect(!manager.hasSession(named: sessionName))
    }

    @Test func paneCountForNonexistentSessionIsZero() {
        let panes = manager.listPanes(sessionName: "planeai-nonexistent-xyz")
        #expect(panes.isEmpty)
    }
}

private func tmuxPaneTestsAvailable() -> Bool {
    let process = Process()
    process.executableURL = URL(fileURLWithPath: "/usr/bin/env")
    process.arguments = ["which", "tmux"]
    process.standardOutput = FileHandle.nullDevice
    process.standardError = FileHandle.nullDevice
    try? process.run()
    process.waitUntilExit()
    return process.terminationStatus == 0
}
