import SwiftUI
import PlaneAICore

struct CommandPaletteView: View {
    let sessions: [SessionInfo]
    let registry: CommandRegistry
    var onActivateSession: ((SessionInfo) -> Void)?

    @Environment(\.dismiss) private var dismiss
    @State private var query = ""
    @State private var selectedIndex = 0

    private enum Result: Identifiable {
        case session(SessionInfo, score: Int)
        case command(Command, score: Int)

        var id: String {
            switch self {
            case .session(let s, _): "s:\(s.id)"
            case .command(let c, _): "c:\(c.id)"
            }
        }
    }

    private var results: [Result] {
        var items: [Result] = []

        for session in sessions {
            let targets = [session.taskName, session.projectName, session.branch]
            let best = targets.compactMap { fuzzyMatch(query: query, target: $0) }.max()
            if let score = best {
                items.append(.session(session, score: score))
            }
        }

        for (cmd, score) in registry.search(query: query) {
            items.append(.command(cmd, score: score))
        }

        return items.sorted { lhs, rhs in
            func score(_ r: Result) -> Int {
                switch r { case .session(_, let s): s; case .command(_, let s): s }
            }
            return score(lhs) > score(rhs)
        }
    }

    var body: some View {
        let currentResults = results
        let clamped = currentResults.isEmpty ? -1 : min(selectedIndex, currentResults.count - 1)

        VStack(spacing: 0) {
            HStack(spacing: 8) {
                Image(systemName: "magnifyingglass")
                    .foregroundStyle(.secondary)
                PaletteTextField(
                    text: $query,
                    onMoveUp: { selectedIndex = max(0, clamped - 1) },
                    onMoveDown: {
                        if !currentResults.isEmpty {
                            selectedIndex = min(clamped + 1, currentResults.count - 1)
                        }
                    },
                    onSubmit: { confirm(currentResults, at: clamped) },
                    onEscape: { dismiss() }
                )
            }
            .padding(12)

            Divider()

            ScrollViewReader { proxy in
                ScrollView {
                    LazyVStack(spacing: 0) {
                        ForEach(Array(currentResults.enumerated()), id: \.element.id) { idx, result in
                            resultRow(result, isSelected: idx == clamped)
                                .id(idx)
                                .contentShape(Rectangle())
                                .onTapGesture { selectedIndex = idx; confirm(currentResults, at: idx) }
                        }
                    }
                    .padding(.vertical, 4)
                }
                .frame(maxHeight: 300)
                .onChange(of: clamped) { _, newValue in
                    if newValue >= 0 { proxy.scrollTo(newValue, anchor: .center) }
                }
            }

            if currentResults.isEmpty && !query.isEmpty {
                Text("No results")
                    .foregroundStyle(.secondary)
                    .padding()
            }
        }
        .frame(width: 480)
        .background(.ultraThinMaterial)
        .clipShape(RoundedRectangle(cornerRadius: 12))
        .shadow(radius: 20)
        .onChange(of: query) { _, _ in selectedIndex = 0 }
    }

    @ViewBuilder
    private func resultRow(_ result: Result, isSelected: Bool) -> some View {
        HStack(spacing: 10) {
            switch result {
            case .session(let session, _):
                Circle()
                    .fill(stateColor(session.state))
                    .frame(width: 8, height: 8)
                VStack(alignment: .leading, spacing: 2) {
                    Text(session.taskName).lineLimit(1)
                    Text(session.projectName)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                Spacer()
                if session.state == .needsAttention {
                    Image(systemName: "exclamationmark.circle.fill")
                        .foregroundStyle(.orange)
                        .font(.caption)
                }
            case .command(let cmd, _):
                Image(systemName: cmd.icon)
                    .foregroundStyle(.secondary)
                    .frame(width: 8)
                Text(cmd.title)
                Spacer()
                Text("Action")
                    .font(.caption)
                    .foregroundStyle(.tertiary)
            }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
        .background(isSelected ? Color.accentColor.opacity(0.2) : Color.clear)
        .cornerRadius(6)
        .padding(.horizontal, 4)
    }

    private func stateColor(_ state: SessionState) -> Color {
        switch state {
        case .running: .green
        case .completed: .gray
        case .needsAttention: .orange
        case .archived: .secondary
        }
    }

    private func confirm(_ results: [Result], at idx: Int) {
        guard idx >= 0, idx < results.count else { return }
        switch results[idx] {
        case .session(let session, _):
            onActivateSession?(session)
        case .command(let cmd, _):
            cmd.action()
        }
        dismiss()
    }
}

// MARK: - PaletteTextField (NSViewRepresentable for arrow key interception)

struct PaletteTextField: NSViewRepresentable {
    @Binding var text: String
    var onMoveUp: () -> Void
    var onMoveDown: () -> Void
    var onSubmit: () -> Void
    var onEscape: () -> Void

    func makeNSView(context: Context) -> NSTextField {
        let field = NSTextField()
        field.delegate = context.coordinator
        field.isBordered = false
        field.backgroundColor = .clear
        field.focusRingType = .none
        field.font = .systemFont(ofSize: NSFont.systemFontSize)
        field.placeholderString = "Search sessions and actions…"
        field.cell?.sendsActionOnEndEditing = false
        DispatchQueue.main.async { field.window?.makeFirstResponder(field) }
        return field
    }

    func updateNSView(_ nsView: NSTextField, context: Context) {
        if nsView.stringValue != text { nsView.stringValue = text }
        context.coordinator.onMoveUp = onMoveUp
        context.coordinator.onMoveDown = onMoveDown
        context.coordinator.onSubmit = onSubmit
        context.coordinator.onEscape = onEscape
    }

    func makeCoordinator() -> Coordinator {
        Coordinator(text: $text, onMoveUp: onMoveUp, onMoveDown: onMoveDown, onSubmit: onSubmit, onEscape: onEscape)
    }

    class Coordinator: NSObject, NSTextFieldDelegate {
        @Binding var text: String
        var onMoveUp: () -> Void
        var onMoveDown: () -> Void
        var onSubmit: () -> Void
        var onEscape: () -> Void

        init(text: Binding<String>, onMoveUp: @escaping () -> Void, onMoveDown: @escaping () -> Void, onSubmit: @escaping () -> Void, onEscape: @escaping () -> Void) {
            _text = text
            self.onMoveUp = onMoveUp
            self.onMoveDown = onMoveDown
            self.onSubmit = onSubmit
            self.onEscape = onEscape
        }

        func controlTextDidChange(_ obj: Notification) {
            guard let field = obj.object as? NSTextField else { return }
            text = field.stringValue
        }

        func control(_ control: NSControl, textView: NSTextView, doCommandBy commandSelector: Selector) -> Bool {
            switch commandSelector {
            case #selector(NSResponder.moveUp(_:)): onMoveUp(); return true
            case #selector(NSResponder.moveDown(_:)): onMoveDown(); return true
            case #selector(NSResponder.insertNewline(_:)): onSubmit(); return true
            case #selector(NSResponder.cancelOperation(_:)): onEscape(); return true
            default: return false
            }
        }
    }
}
