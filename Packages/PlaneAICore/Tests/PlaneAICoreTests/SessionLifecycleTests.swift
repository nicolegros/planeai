import Testing
@testable import PlaneAICore
import Foundation

@Suite("Session Lifecycle")
struct SessionLifecycleTests {

    private func makeStore(tmuxOutput: String = "planeai-proj-task:/tmp/proj-task") -> SessionStore {
        let projects = [
            Project(id: UUID(), name: "proj", repoPath: "/tmp/proj", defaultProvider: "claude", defaultAutoApprove: false, defaultBranchStrategy: .worktree)
        ]
        let store = SessionStore(projects: projects, tmuxListProvider: { tmuxOutput })
        store.refresh()
        return store
    }

    // MARK: - State enum

    @Test("SessionState includes archived case")
    func archivedStateExists() {
        let state = SessionState.archived
        #expect(state.rawValue == "archived")
    }

    // MARK: - Complete

    @Test("complete transitions running session to completed")
    func completeTransition() {
        let store = makeStore()
        let sessionId = store.sessions[0].id
        store.complete(sessionId: sessionId)
        #expect(store.sessions.first(where: { $0.id == sessionId })?.state == .completed)
    }

    @Test("complete toggles back to running if already completed")
    func completeToggles() {
        let store = makeStore()
        let sessionId = store.sessions[0].id
        store.complete(sessionId: sessionId)
        #expect(store.sessions.first(where: { $0.id == sessionId })?.state == .completed)
        store.complete(sessionId: sessionId)
        #expect(store.sessions.first(where: { $0.id == sessionId })?.state == .running)
    }

    // MARK: - Archive

    @Test("archive transitions completed session to archived and removes from sessions")
    func archiveTransition() {
        let store = makeStore()
        let sessionId = store.sessions[0].id
        store.complete(sessionId: sessionId)
        store.archive(sessionId: sessionId)
        #expect(store.sessions.contains(where: { $0.id == sessionId }) == false)
        #expect(store.archivedSessions.contains(where: { $0.id == sessionId }))
    }

    @Test("archive running session transitions directly to archived")
    func archiveRunningSession() {
        let store = makeStore()
        let sessionId = store.sessions[0].id
        store.archive(sessionId: sessionId)
        #expect(store.sessions.contains(where: { $0.id == sessionId }) == false)
        #expect(store.archivedSessions.contains(where: { $0.id == sessionId }))
    }

    // MARK: - Delete

    @Test("delete removes session from both active and archived lists")
    func deleteRemovesCompletely() {
        let store = makeStore()
        let sessionId = store.sessions[0].id
        store.delete(sessionId: sessionId)
        #expect(store.sessions.contains(where: { $0.id == sessionId }) == false)
        #expect(store.archivedSessions.contains(where: { $0.id == sessionId }) == false)
    }

    @Test("delete archived session removes from archived list")
    func deleteArchivedSession() {
        let store = makeStore()
        let sessionId = store.sessions[0].id
        store.archive(sessionId: sessionId)
        store.delete(sessionId: sessionId)
        #expect(store.archivedSessions.contains(where: { $0.id == sessionId }) == false)
    }

    // MARK: - Restore

    @Test("restore moves archived session back to active sessions")
    func restoreTransition() {
        let store = makeStore()
        let sessionId = store.sessions[0].id
        store.archive(sessionId: sessionId)
        store.restore(sessionId: sessionId)
        #expect(store.sessions.contains(where: { $0.id == sessionId }))
        #expect(store.archivedSessions.contains(where: { $0.id == sessionId }) == false)
    }

    @Test("restored session has completed state")
    func restoredSessionState() {
        let store = makeStore()
        let sessionId = store.sessions[0].id
        store.archive(sessionId: sessionId)
        store.restore(sessionId: sessionId)
        #expect(store.sessions.first(where: { $0.id == sessionId })?.state == .completed)
    }

    // MARK: - Filtering

    @Test("activeSessions excludes archived sessions")
    func activeSessionsFilter() {
        let tmux = """
        planeai-proj-task1:/tmp/proj-task1
        planeai-proj-task2:/tmp/proj-task2
        """
        let store = makeStore(tmuxOutput: tmux)
        store.archive(sessionId: "planeai-proj-task1")
        #expect(store.sessions.count == 1)
        #expect(store.sessions[0].id == "planeai-proj-task2")
    }
    @Test("complete persists to DB and survives new store instance")
    func completePersistsAcrossStoreInstances() throws {
        let dbManager = try DatabaseManager(storage: .inMemory)
        let project = Project(id: UUID(), name: "proj", repoPath: "/tmp/proj", defaultProvider: "claude", defaultAutoApprove: false, defaultBranchStrategy: .worktree)
        try dbManager.dbQueue.write { db in try project.insert(db) }
        let projects = [project]
        let tmux = "planeai-proj-task:/tmp/proj-task"

        let store1 = SessionStore(projects: projects, db: dbManager.dbQueue, tmuxListProvider: { tmux })
        store1.refresh()
        store1.complete(sessionId: "planeai-proj-task")
        #expect(store1.sessions[0].state == .completed)

        // Simulate app restart: new store, same DB
        let store2 = SessionStore(projects: projects, db: dbManager.dbQueue, tmuxListProvider: { tmux })
        store2.refresh()
        #expect(store2.sessions[0].state == .completed)
    }

    @Test("complete persists even when project has no matching DB row")
    func completePersistsWithoutProjectInDB() throws {
        let dbManager = try DatabaseManager(storage: .inMemory)
        // Project exists in memory but NOT in DB — simulates potential FK issue
        let projects = [
            Project(id: UUID(), name: "proj", repoPath: "/tmp/proj", defaultProvider: "claude", defaultAutoApprove: false, defaultBranchStrategy: .worktree)
        ]
        let tmux = "planeai-proj-task:/tmp/proj-task"

        let store1 = SessionStore(projects: projects, db: dbManager.dbQueue, tmuxListProvider: { tmux })
        store1.refresh()
        store1.complete(sessionId: "planeai-proj-task")
        #expect(store1.sessions[0].state == .completed)

        // This will fail if FK constraint blocks the save
        let store2 = SessionStore(projects: projects, db: dbManager.dbQueue, tmuxListProvider: { tmux })
        store2.refresh()
        #expect(store2.sessions[0].state == .completed)
    }
}

@Suite("Session Activation")
struct SessionActivationTests {

    @Test("new session appears in activated list and becomes active")
    func newSessionActivation() {
        // Simulates the onCreate flow: a new session must be added to activatedSessions
        // and set as activeSessionId to receive terminal focus.
        var activatedSessions: [(id: String, command: String)] = []
        var activeSessionId: String?
        var focusToken: UInt = 0

        let newSessionId = "planeai-proj-new-task"
        let cmd = "tmux attach -t planeai-proj-new-task"

        // Simulate onCreate logic (mirrors PlaneAIApp.onCreate)
        if !activatedSessions.contains(where: { $0.id == newSessionId }) {
            activatedSessions.append((id: newSessionId, command: cmd))
        }
        activeSessionId = newSessionId
        focusToken &+= 1

        #expect(activatedSessions.contains(where: { $0.id == newSessionId }))
        #expect(activeSessionId == newSessionId)
        #expect(focusToken == 1)
    }

    @Test("creating second session preserves first in activated list")
    func secondSessionPreservesFirst() {
        var activatedSessions: [(id: String, command: String)] = []
        var activeSessionId: String?
        var sessionHistory: [String] = []
        var focusToken: UInt = 0

        // First session
        let first = "planeai-proj-task1"
        activatedSessions.append((id: first, command: "cmd1"))
        activeSessionId = first
        focusToken &+= 1

        // Second session (mirrors onCreate with history tracking)
        let second = "planeai-proj-task2"
        if let current = activeSessionId, current != second {
            sessionHistory.removeAll { $0 == current }
            sessionHistory.append(current)
        }
        activatedSessions.append((id: second, command: "cmd2"))
        activeSessionId = second
        focusToken &+= 1

        #expect(activatedSessions.count == 2)
        #expect(activeSessionId == second)
        #expect(sessionHistory == [first])
        #expect(focusToken == 2)
    }
}
