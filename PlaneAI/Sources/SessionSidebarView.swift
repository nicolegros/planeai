import SwiftUI
import PlaneAICore

struct SessionSidebarView: View {
    @Binding var selectedSessionId: String?
    let groupedSessions: [(project: String, sessions: [SessionInfo])]
    var onSelect: ((SessionInfo) -> Void)?

    var body: some View {
        let allSessions = groupedSessions.flatMap(\.sessions)
        List(selection: $selectedSessionId) {
            ForEach(groupedSessions, id: \.project) { group in
                Section(group.project) {
                    if group.sessions.isEmpty {
                        Text("No active sessions")
                            .font(.caption)
                            .foregroundStyle(.tertiary)
                    } else {
                        ForEach(group.sessions) { session in
                            let position = allSessions.firstIndex(where: { $0.id == session.id })
                            SessionRow(session: session, position: position.map { $0 + 1 })
                                .tag(session.id)
                        }
                    }
                }
            }
        }
        .listStyle(.sidebar)
        .onKeyPress(.return) {
            if let id = selectedSessionId,
               let session = allSessions.first(where: { $0.id == id }) {
                onSelect?(session)
                return .handled
            }
            return .ignored
        }
    }
}

struct SessionRow: View {
    let session: SessionInfo
    var position: Int?

    var body: some View {
        HStack(spacing: 8) {
            stateIndicator
            VStack(alignment: .leading, spacing: 2) {
                Text(session.taskName)
                    .font(.body)
                    .lineLimit(1)
                Text(session.branch)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
            Spacer()
            if let position, position <= 9 {
                Text("⌘\(position)")
                    .font(.caption2.monospaced())
                    .foregroundStyle(.tertiary)
            }
            providerIcon
        }
        .padding(.vertical, 2)
        .accessibilityElement(children: .combine)
        .accessibilityLabel("\(session.taskName), \(session.state.rawValue)")
    }

    private var stateIndicator: some View {
        Circle()
            .fill(stateColor)
            .frame(width: 8, height: 8)
            .accessibilityHidden(true)
    }

    private var stateColor: Color {
        switch session.state {
        case .running: .green
        case .completed: .gray
        case .needsAttention: .orange
        }
    }

    private var providerIcon: some View {
        Image(systemName: providerSystemImage)
            .font(.caption)
            .foregroundStyle(.secondary)
            .accessibilityLabel(session.provider)
    }

    private var providerSystemImage: String {
        switch session.provider.lowercased() {
        case "claude": "brain"
        case "kiro-cli": "terminal"
        case "codex": "doc.text"
        default: "cpu"
        }
    }
}
