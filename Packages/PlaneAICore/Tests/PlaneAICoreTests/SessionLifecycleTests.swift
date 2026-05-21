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
}
