import SwiftUI
import PlaneAICore

@main
struct PlaneAIApp: App {
    @State private var ghosttyManager = GhosttyAppManager()
    @State private var projectStore = ProjectStoreViewModel()
    @State private var sessionStore = SessionStore(projects: [])
    @State private var showNewSessionPalette = false
    @State private var sidebarVisibility: NavigationSplitViewVisibility = .automatic
    @State private var activeTerminalCommand: String?
    @State private var activeSessionId: String?
    @State private var selectedSessionId: String?
    @State private var sessionHistory: [String] = []  // MRU stack of session IDs
    @State private var activatedSessions: [(id: String, command: String)] = []
    @State private var showTabSwitcher = false
    @State private var tabSwitcherIndex = 0
    @State private var eventMonitor: Any?

    var body: some Scene {
        WindowGroup {
            NavigationSplitView(columnVisibility: $sidebarVisibility) {
                SessionSidebarView(
                    selectedSessionId: $selectedSessionId,
                    groupedSessions: groupedSessions,
                    onSelect: { session in
                        activateSession(session)
                    }
                )
                .toolbar {
                    ToolbarItem(placement: .primaryAction) {
                        Button(action: { showNewSessionPalette = true }) {
                            Label("New Session", systemImage: "plus")
                        }
                    }
                }
                .navigationSplitViewColumnWidth(min: 180, ideal: 220, max: 450)
            } detail: {
                ContentView(ghosttyManager: ghosttyManager, terminalCommand: activeTerminalCommand, sessionId: activeSessionId, activatedSessions: activatedSessions)
            }
            .frame(minWidth: 640, minHeight: 480)
            .overlay {
                if showTabSwitcher {
                    TabSwitcherView(
                        sessions: mruSessionList,
                        selectedIndex: tabSwitcherIndex
                    )
                }
            }
            .sheet(isPresented: $showNewSessionPalette) {
                NewSessionPaletteView(
                    projects: projectStore.projects,
                    selectedProject: projectStore.projects.first(where: { $0.id == projectStore.selectedProjectID }),
                    onCreate: { session in
                        let cmd = TmuxManager().attachCommand(for: session.tmuxSession).joined(separator: " ")
                        DispatchQueue.main.async {
                            activeTerminalCommand = cmd
                            activeSessionId = session.tmuxSession.name
                            selectedSessionId = session.tmuxSession.name
                            refreshSessions()
                        }
                    }
                )
            }
            .onAppear {
                refreshSessions()
                eventMonitor = NSEvent.addLocalMonitorForEvents(matching: [.keyDown, .flagsChanged]) { event in
                    if event.type == .keyDown && event.keyCode == 48 && event.modifierFlags.contains(.control) {
                        switchToPreviousSession()
                        return nil
                    }
                    if event.type == .flagsChanged && showTabSwitcher && !event.modifierFlags.contains(.control) {
                        confirmTabSwitch()
                        return nil
                    }
                    if event.type == .keyDown && showTabSwitcher && event.keyCode == 36 {
                        // Enter key confirms
                        confirmTabSwitch()
                        return nil
                    }
                    if event.type == .keyDown && showTabSwitcher && event.keyCode == 53 {
                        // Escape cancels
                        showTabSwitcher = false
                        return nil
                    }
                    return event
                }
            }
        }
        .commands {
            CommandGroup(replacing: .newItem) {
                Button("New Session") {
                    showNewSessionPalette = true
                }
                .keyboardShortcut("n", modifiers: .command)
            }
            CommandGroup(after: .sidebar) {
                Button("Toggle Sidebar") {
                    withAnimation {
                        sidebarVisibility = sidebarVisibility == .detailOnly ? .automatic : .detailOnly
                    }
                }
                .keyboardShortcut("b", modifiers: .command)

                Button("Previous Session") {
                    switchToPreviousSession()
                }
                .keyboardShortcut(KeyEquivalent("\t"), modifiers: .control)

                Button("Focus Sidebar") {
                    sidebarVisibility = .all
                    DispatchQueue.main.async {
                        guard let window = NSApp.keyWindow else { return }
                        func findListView(_ view: NSView) -> NSView? {
                            if view is NSOutlineView || view is NSTableView { return view }
                            for sub in view.subviews {
                                if let found = findListView(sub) { return found }
                            }
                            return nil
                        }
                        if let listView = findListView(window.contentView!),
                           window.firstResponder !== listView {
                            window.makeFirstResponder(listView)
                        } else {
                            // Already in sidebar — focus the terminal
                            func findSurfaceView(_ view: NSView) -> NSView? {
                                if view is TerminalSurfaceView { return view }
                                for sub in view.subviews.reversed() {
                                    if let found = findSurfaceView(sub) { return found }
                                }
                                return nil
                            }
                            if let termView = findSurfaceView(window.contentView!) {
                                window.makeFirstResponder(termView)
                            }
                        }
                    }
                }
                .keyboardShortcut("0", modifiers: .command)

                Divider()

                // Cmd+1-9 quick jump
                ForEach(1...9, id: \.self) { idx in
                    Button("Session \(idx)") {
                        jumpToSession(at: idx - 1)
                    }
                    .keyboardShortcut(KeyEquivalent(Character("\(idx)")), modifiers: .command)
                }
            }
        }
    }

    private var groupedSessions: [(project: String, sessions: [SessionInfo])] {
        let grouped = Dictionary(grouping: sessionStore.sessions, by: \.projectName)
        // Show all registered projects, even those with no active sessions
        var result: [(project: String, sessions: [SessionInfo])] = []
        for project in projectStore.projects {
            result.append((project: project.name, sessions: grouped[project.name] ?? []))
        }
        // Also include any orphan sessions whose project isn't registered
        for key in grouped.keys.sorted() where !projectStore.projects.contains(where: { $0.name == key }) {
            result.append((project: key, sessions: grouped[key]!))
        }
        return result
    }

    private func refreshSessions() {
        sessionStore = SessionStore(projects: projectStore.projects)
        sessionStore.refresh()
    }

    private func jumpToSession(at index: Int) {
        let allSessions = groupedSessions.flatMap(\.sessions)
        guard index < allSessions.count else { return }
        activateSession(allSessions[index])
    }

    private func activateSession(_ session: SessionInfo) {
        if let current = activeSessionId, current != session.id {
            sessionHistory.removeAll { $0 == current }
            sessionHistory.append(current)
        }
        selectedSessionId = session.id
        let tmux = TmuxSession(name: session.id, workingDirectory: "/")
        let cmd = TmuxManager().attachCommand(for: tmux).joined(separator: " ")
        // Add to activated list if not already there
        if !activatedSessions.contains(where: { $0.id == session.id }) {
            activatedSessions.append((id: session.id, command: cmd))
        }
        activeSessionId = session.id
        activeTerminalCommand = cmd
    }

    private var mruSessionList: [SessionInfo] {
        let allSessions = groupedSessions.flatMap(\.sessions)
        // Current session first, then history (most recent first), then remaining
        var ordered: [SessionInfo] = []
        if let currentId = activeSessionId,
           let current = allSessions.first(where: { $0.id == currentId }) {
            ordered.append(current)
        }
        for id in sessionHistory.reversed() {
            if let session = allSessions.first(where: { $0.id == id }),
               !ordered.contains(where: { $0.id == id }) {
                ordered.append(session)
            }
        }
        for session in allSessions where !ordered.contains(where: { $0.id == session.id }) {
            ordered.append(session)
        }
        return ordered
    }

    private func switchToPreviousSession() {
        let mru = mruSessionList
        guard mru.count > 1 else { return }

        if showTabSwitcher {
            tabSwitcherIndex = (tabSwitcherIndex + 1) % mru.count
        } else {
            showTabSwitcher = true
            tabSwitcherIndex = 1
        }
    }

    private func confirmTabSwitch() {
        let mru = mruSessionList
        guard tabSwitcherIndex < mru.count else { return }
        showTabSwitcher = false
        activateSession(mru[tabSwitcherIndex])
    }
}

struct ContentView: View {
    let ghosttyManager: GhosttyAppManager
    let terminalCommand: String?
    let sessionId: String?
    let activatedSessions: [(id: String, command: String)]

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
                    if activatedSessions.isEmpty {
                        TerminalView(ghosttyApp: app)
                    } else {
                        ForEach(activatedSessions, id: \.id) { session in
                            TerminalView(ghosttyApp: app, command: session.command)
                                .opacity(session.id == sessionId ? 1 : 0)
                                .allowsHitTesting(session.id == sessionId)
                        }
                    }
                }
            }
        }
        .background(.black)
    }
}
