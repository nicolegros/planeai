import SwiftUI
import PlaneAICore

@main
struct PlaneAIApp: App {
    @State private var ghosttyManager = GhosttyAppManager()
    @State private var projectStore = ProjectStoreViewModel()
    @State private var showNewSessionPalette = false
    @State private var activeTerminalCommand: String?
    @State private var activeSessionId: String?

    var body: some Scene {
        WindowGroup {
            NavigationSplitView {
                ProjectSidebarView(store: projectStore)
            } detail: {
                ContentView(ghosttyManager: ghosttyManager, terminalCommand: activeTerminalCommand, sessionId: activeSessionId)
            }
            .frame(minWidth: 640, minHeight: 480)
            .sheet(isPresented: $showNewSessionPalette) {
                NewSessionPaletteView(
                    projects: projectStore.projects,
                    selectedProject: projectStore.projects.first(where: { $0.id == projectStore.selectedProjectID }),
                    onCreate: { session in
                        let cmd = TmuxManager().attachCommand(for: session.tmuxSession).joined(separator: " ")
                        DispatchQueue.main.async {
                            activeTerminalCommand = cmd
                            activeSessionId = session.tmuxSession.name
                        }
                    }
                )
            }
        }
        .commands {
            CommandGroup(replacing: .newItem) {
                Button("New Session") {
                    showNewSessionPalette = true
                }
                .keyboardShortcut("n", modifiers: .command)
            }
        }
    }
}

struct ContentView: View {
    let ghosttyManager: GhosttyAppManager
    let terminalCommand: String?
    let sessionId: String?

    var body: some View {
        Group {
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
                    if let command = terminalCommand {
                        TerminalView(ghosttyApp: app, command: command)
                            .id(sessionId)
                    } else {
                        TerminalView(ghosttyApp: app)
                    }
                }
            }
        }
    }
}
