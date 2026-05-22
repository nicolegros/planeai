import Testing
@testable import PlaneAICore
import Foundation

@Suite("SessionStore")
struct SessionStoreTests {

    @Test("parses tmux list-sessions output into SessionInfo grouped by project")
    func parsesSessionsFromTmuxOutput() {
        let projects = [
            Project(id: UUID(), name: "myapp", repoPath: "/tmp/myapp", defaultProvider: "claude", defaultAutoApprove: false, defaultBranchStrategy: .worktree)
        ]
        let tmuxOutput = """
        planeai-myapp-fix-auth:/tmp/myapp-fix-auth
        planeai-myapp-add-tests:/tmp/myapp-add-tests
        unrelated-session:/tmp/other
        """
        let store = SessionStore(projects: projects, tmuxListProvider: { tmuxOutput })
        store.refresh()

        #expect(store.sessions.count == 2)
        #expect(store.sessions[0].taskName == "fix-auth")
        #expect(store.sessions[0].projectName == "myapp")
        #expect(store.sessions[1].taskName == "add-tests")
    }

    @Test("returns empty when no planeai sessions exist")
    func emptyWhenNoSessions() {
        let store = SessionStore(projects: [], tmuxListProvider: { "" })
        store.refresh()
        #expect(store.sessions.isEmpty)
    }

    @Test("groups sessions by project name")
    func groupsByProject() {
        let projects = [
            Project(id: UUID(), name: "alpha", repoPath: "/tmp/alpha", defaultProvider: "", defaultAutoApprove: false, defaultBranchStrategy: .worktree),
            Project(id: UUID(), name: "beta", repoPath: "/tmp/beta", defaultProvider: "", defaultAutoApprove: false, defaultBranchStrategy: .worktree),
        ]
        let tmuxOutput = """
        planeai-alpha-task1:/tmp/alpha-task1
        planeai-beta-task2:/tmp/beta-task2
        planeai-alpha-task3:/tmp/alpha-task3
        """
        let store = SessionStore(projects: projects, tmuxListProvider: { tmuxOutput })
        store.refresh()

        let grouped = store.groupedByProject
        #expect(grouped["alpha"]?.count == 2)
        #expect(grouped["beta"]?.count == 1)
    }

    @Test("session state defaults to running")
    func defaultStateIsRunning() {
        let projects = [
            Project(id: UUID(), name: "proj", repoPath: "/tmp/proj", defaultProvider: "", defaultAutoApprove: false, defaultBranchStrategy: .worktree)
        ]
        let store = SessionStore(projects: projects, tmuxListProvider: { "planeai-proj-task:/tmp/proj-task" })
        store.refresh()
        #expect(store.sessions.first?.state == .running)
    }
}
