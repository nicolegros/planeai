import SwiftUI
import PlaneAICore

struct ActivatedSession: Identifiable, Equatable {
    let id: String
    let command: String
}

@main
struct PlaneAIApp: App {
    @State private var ghosttyManager = GhosttyAppManager()
    @State private var coordinator: AppCoordinator
    @State private var keyboardRouter: KeyboardRouter?

    init() {
        let db = try? DatabaseManager()
        let store = db.map { ProjectStore(db: $0.dbQueue) } ?? ProjectStore(db: try! DatabaseManager(storage: .inMemory).dbQueue)
        let sessionStore = SessionStore(projects: store.projects, db: db?.dbQueue)
        _coordinator = State(initialValue: AppCoordinator(projectStore: store, sessionStore: sessionStore, dbManager: db))
    }

    var body: some Scene {
        WindowGroup {
            NavigationSplitView(columnVisibility: $coordinator.sidebarVisibility) {
                SessionSidebarView(
                    selectedSessionId: $coordinator.selectedSessionId,
                    groupedSessions: coordinator.groupedSessions,
                    onSelect: { coordinator.activateSession($0) },
                    onComplete: { coordinator.completeSession(id: $0) },
                    onArchive: { coordinator.archiveSession(id: $0) },
                    onDelete: { coordinator.deleteSession(id: $0) },
                    onRestore: { coordinator.restoreSession(id: $0) },
                    shouldConfirmDelete: { coordinator.shouldConfirmDelete(id: $0) },
                    archivedSessions: coordinator.sessionStore.archivedSessions
                )
                .toolbar {
                    ToolbarItem(placement: .primaryAction) {
                        Button(action: { coordinator.showNewSessionPalette = true }) {
                            Label("New Session", systemImage: "plus")
                        }
                    }
                }
                .navigationSplitViewColumnWidth(min: 180, ideal: 220, max: 450)
            } detail: {
                ContentView(ghosttyManager: ghosttyManager, terminalCommand: activeTerminalCommand, sessionId: coordinator.activeSessionId, activatedSessions: coordinator.activatedSessions, focusToken: coordinator.focusToken)
            }
            .frame(minWidth: 640, minHeight: 480)
            .overlay {
                if coordinator.showTabSwitcher {
                    TabSwitcherView(
                        sessions: coordinator.mruSessionList,
                        selectedIndex: coordinator.tabSwitcherIndex
                    )
                }
            }
            .sheet(isPresented: $coordinator.showNewSessionPalette) {
                NewSessionPaletteView(
                    projects: coordinator.projects,
                    selectedProject: coordinator.projects.first(where: { $0.id == coordinator.selectedProjectID }),
                    onCreate: { coordinator.sessionCreated($0) }
                )
            }
            .sheet(isPresented: $coordinator.showCommandPalette) {
                CommandPaletteView(
                    sessions: coordinator.sessionStore.sessions,
                    projects: coordinator.projects,
                    onActivateSession: { coordinator.activateSession($0) },
                    onAction: { action in
                        switch action {
                        case .newSession:
                            coordinator.showNewSessionPalette = true
                        case .archiveSession:
                            if let id = coordinator.activeSessionId {
                                coordinator.archiveSession(id: id)
                            }
                        case .toggleSidebar:
                            coordinator.toggleSidebar()
                        }
                    }
                )
            }
            .alert("Archive Session", isPresented: $coordinator.showArchiveWorktreePrompt) {
                Button("Keep Worktree") { coordinator.confirmArchive(removeWorktree: false) }
                Button("Remove Worktree", role: .destructive) { coordinator.confirmArchive(removeWorktree: true) }
                Button("Cancel", role: .cancel) { coordinator.cancelArchive() }
            } message: {
                Text("Do you want to keep or remove the worktree for this session?")
            }
            .alert("Close Agent Pane", isPresented: $coordinator.showClosePaneConfirmation) {
                Button("Close", role: .destructive) { coordinator.forceCloseActivePane() }
                Button("Cancel", role: .cancel) {}
            } message: {
                Text("This is the primary agent pane. Closing it will end the agent process. Continue?")
            }
            .onAppear {
                coordinator.refreshSessions()
                let router = KeyboardRouter(coordinator: coordinator)
                router.start()
                keyboardRouter = router
            }
            .onDisappear {
                keyboardRouter?.stop()
                keyboardRouter = nil
            }
        }
        .commands {
            CommandGroup(replacing: .newItem) {
                Button("New Session") { coordinator.showNewSessionPalette = true }
                    .keyboardShortcut("n", modifiers: .command)
                Button("Command Palette") { coordinator.showCommandPalette.toggle() }
                    .keyboardShortcut("k", modifiers: .command)
            }
            CommandGroup(after: .sidebar) {
                Button("Toggle Sidebar") { coordinator.toggleSidebar() }
                    .keyboardShortcut("b", modifiers: .command)
                Button("Previous Session") { coordinator.switchToPreviousSession() }
                    .keyboardShortcut(KeyEquivalent("\t"), modifiers: .control)
                Button("Focus Sidebar") { coordinator.focusSidebar() }
                    .keyboardShortcut("0", modifiers: .command)

                Divider()

                ForEach(1...9, id: \.self) { idx in
                    Button("Session \(idx)") { coordinator.jumpToSession(at: idx - 1) }
                        .keyboardShortcut(KeyEquivalent(Character("\(idx)")), modifiers: .command)
                }
            }
            CommandGroup(after: .windowArrangement) {
                Button("Split Right") { coordinator.splitActivePane(direction: .right) }
                    .keyboardShortcut("d", modifiers: .command)
                Button("Split Down") { coordinator.splitActivePane(direction: .down) }
                    .keyboardShortcut("d", modifiers: [.command, .shift])

                Divider()

                Button("Focus Pane Left") { coordinator.focusAdjacentPane(direction: .left) }
                    .keyboardShortcut(.leftArrow, modifiers: [.command, .option])
                Button("Focus Pane Right") { coordinator.focusAdjacentPane(direction: .right) }
                    .keyboardShortcut(.rightArrow, modifiers: [.command, .option])
                Button("Focus Pane Up") { coordinator.focusAdjacentPane(direction: .up) }
                    .keyboardShortcut(.upArrow, modifiers: [.command, .option])
                Button("Focus Pane Down") { coordinator.focusAdjacentPane(direction: .down) }
                    .keyboardShortcut(.downArrow, modifiers: [.command, .option])

                Divider()

                Button("Close Pane ⌘W") { coordinator.closeActivePane() }
            }
        }
    }

    private var activeTerminalCommand: String? {
        coordinator.activatedSessions.first(where: { $0.id == coordinator.activeSessionId })?.command
    }
}

struct ContentView: View {
    let ghosttyManager: GhosttyAppManager
    let terminalCommand: String?
    let sessionId: String?
    let activatedSessions: [ActivatedSession]
    var focusToken: UInt = 0

    var body: some View {
        ZStack {
            switch ghosttyManager.state {
            case .loading:
                ProgressView("Initializing terminal…")
            case .error:
                VStack(spacing: 12) {
                    Image(systemName: "exclamationmark.triangle")
                        .font(.largeTitle)
                    Text("Failed to initialize ghostty")
                    Text("Ensure the GhosttyKit framework is built and linked.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            case .ready:
                if let app = ghosttyManager.app {
                    ForEach(activatedSessions, id: \.id) { session in
                        TerminalView(ghosttyApp: app, command: session.command, isActive: session.id == sessionId, focusToken: session.id == sessionId ? focusToken : 0, appManager: ghosttyManager)
                    }
                }
            }
        }
        .background(.black)
    }
}
