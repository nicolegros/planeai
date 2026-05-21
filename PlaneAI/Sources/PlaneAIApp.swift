import SwiftUI

@main
struct PlaneAIApp: App {
    @State private var ghosttyManager = GhosttyAppManager()

    var body: some Scene {
        WindowGroup {
            ContentView(ghosttyManager: ghosttyManager)
                .frame(minWidth: 640, minHeight: 480)
        }
        .commands {
            // Cmd+Q is handled by default SwiftUI app lifecycle
            CommandGroup(replacing: .newItem) {} // Remove Cmd+N for now
        }
    }
}

struct ContentView: View {
    let ghosttyManager: GhosttyAppManager

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
                    TerminalView(ghosttyApp: app)
                }
            }
        }
    }
}
