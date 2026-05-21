import SwiftUI
import PlaneAICore

struct SessionSidebarView: View {
    @Binding var selectedSessionId: String?
    let groupedSessions: [(project: String, sessions: [SessionInfo])]
    var onSelect: ((SessionInfo) -> Void)?
    var onComplete: ((String) -> Void)?
    var onArchive: ((String) -> Void)?
    var onDelete: ((String) -> Void)?
    var onRestore: ((String) -> Void)?
    var shouldConfirmDelete: ((String) -> Bool) = { _ in false }
    var archivedSessions: [SessionInfo] = []
    @State private var showArchived = false
    @State private var showDeleteConfirmation = false
    @State private var pendingDeleteId: String?

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

            if !archivedSessions.isEmpty {
                Section {
                    HStack {
                        Image(systemName: showArchived ? "chevron.down" : "chevron.right")
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                        Text("Archived")
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                        Spacer()
                        Text("\(archivedSessions.count)")
                            .font(.caption2)
                            .foregroundStyle(.tertiary)
                    }
                    .tag("__archived_header__")

                    if showArchived {
                        ForEach(archivedSessions) { session in
                            SessionRow(session: session, position: nil)
                                .tag(session.id)
                        }
                    }
                }
            }
        }
        .listStyle(.sidebar)
        .onKeyPress(.leftArrow) {
            guard selectedSessionId == "__archived_header__" && showArchived else { return .ignored }
            showArchived = false
            return .handled
        }
        .onKeyPress(.rightArrow) {
            guard selectedSessionId == "__archived_header__" && !showArchived else { return .ignored }
            showArchived = true
            return .handled
        }
        .onKeyPress(.return) {
            guard let id = selectedSessionId, id != "__archived_header__" else { return .ignored }
            if let session = allSessions.first(where: { $0.id == id }) {
                onSelect?(session)
            }
            return .handled
        }
        .onKeyPress(characters: CharacterSet(charactersIn: "c")) { _ in
            guard let id = selectedSessionId else { return .ignored }
            onComplete?(id)
            return .handled
        }
        .onKeyPress(characters: CharacterSet(charactersIn: "a")) { _ in
            guard let id = selectedSessionId else { return .ignored }
            onArchive?(id)
            return .handled
        }
        .onKeyPress(characters: CharacterSet(charactersIn: "d")) { _ in
            guard let id = selectedSessionId else { return .ignored }
            if shouldConfirmDelete(id) {
                pendingDeleteId = id
                showDeleteConfirmation = true
            } else {
                onDelete?(id)
                if selectedSessionId == id { selectedSessionId = nil }
            }
            return .handled
        }
        .onKeyPress(characters: CharacterSet(charactersIn: "r")) { _ in
            guard let id = selectedSessionId else { return .ignored }
            onRestore?(id)
            return .handled
        }
        .alert("Delete Session?", isPresented: $showDeleteConfirmation) {
            Button("Delete", role: .destructive) {
                if let id = pendingDeleteId {
                    onDelete?(id)
                    if selectedSessionId == id { selectedSessionId = nil }
                }
                pendingDeleteId = nil
            }
            Button("Cancel", role: .cancel) {
                pendingDeleteId = nil
            }
        } message: {
            Text("This will remove the session, scrollback, and worktree. This cannot be undone.")
        }
    }
}

struct SessionRow: View {
    let session: SessionInfo
    var position: Int?

    private var isDimmed: Bool {
        session.state == .completed || session.state == .archived
    }

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
        .opacity(isDimmed ? 0.5 : 1.0)
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
        case .archived: .gray
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
