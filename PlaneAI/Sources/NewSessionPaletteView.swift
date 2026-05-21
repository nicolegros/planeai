import SwiftUI
import PlaneAICore

struct NewSessionPaletteView: View {
    let projects: [Project]
    @Environment(\.dismiss) private var dismiss

    @State private var selectedProject: Project?
    @State private var providers: [ProviderSpec] = []
    @State private var selectedProvider: ProviderSpec?
    @State private var taskName = ""
    @State private var branchStrategy: BranchStrategy = .worktree
    @State private var autoApprove = false
    @State private var errorMessage: String?
    @State private var isCreating = false
    @State private var step: Step = .project
    @State private var highlightedIndex = 0
    @FocusState private var taskFieldFocused: Bool
    @FocusState private var paletteFocused: Bool

    var onCreate: ((Session) -> Void)?

    enum Step: Int, CaseIterable {
        case project, provider, task, branch, approve
    }

    init(projects: [Project], selectedProject: Project? = nil, onCreate: ((Session) -> Void)? = nil) {
        self.projects = projects
        self.onCreate = onCreate
        _selectedProject = State(initialValue: selectedProject)
        _step = State(initialValue: selectedProject != nil ? .provider : .project)
    }

    var body: some View {
        VStack(spacing: 20) {
            Text("New Session").font(.headline)
            Text(stepLabel).font(.subheadline).foregroundStyle(.secondary)

            Group {
                switch step {
                case .project: listStep(items: projects.map { $0.name }, details: nil)
                case .provider: listStep(items: providers.map { $0.name }, details: providers.map { $0.command })
                case .task: taskStep
                case .branch: listStep(items: ["Worktree — isolated branch", "Main branch — repo root"], details: nil)
                case .approve: approveStep
                }
            }
            .frame(maxWidth: .infinity)

            if let errorMessage {
                Text(errorMessage).foregroundStyle(.red).font(.caption)
            }

            HStack {
                Text("↑↓ navigate  ⏎ select  ⎋ cancel")
                    .font(.caption2).foregroundStyle(.tertiary)
                Spacer()
                if step.rawValue > 0 && step != .approve {
                    Text("⌫ back").font(.caption2).foregroundStyle(.tertiary)
                }
            }
        }
        .padding(24)
        .frame(width: 400)
        .focusable()
        .focusEffectDisabled()
        .focused($paletteFocused)
        .onKeyPress(.upArrow) { moveHighlight(-1); return .handled }
        .onKeyPress(.downArrow) { moveHighlight(1); return .handled }
        .onKeyPress(.return) { confirmSelection(); return .handled }
        .onKeyPress(.escape) { dismiss(); return .handled }
        .onKeyPress(.delete) { goBack(); return .handled }
        .onKeyPress(characters: .decimalDigits) { press in
            if step == .task { return .ignored }
            handleDigit(press.characters)
            return .handled
        }
        .onKeyPress(characters: CharacterSet(charactersIn: "aA")) { _ in
            if step == .approve { autoApprove.toggle(); return .handled }
            return .ignored
        }
        .onAppear {
            providers = ProviderDetector.detect()
            if let project = selectedProject {
                applyDefaults(for: project)
            }
            paletteFocused = true
        }
        .onChange(of: step) {
            if step != .task { paletteFocused = true }
        }
        .onChange(of: autoApprove) {
            paletteFocused = true
        }
    }

    // MARK: - Key Handler (removed separate view)

    // MARK: - Steps

    private func listStep(items: [String], details: [String]?) -> some View {
        VStack(spacing: 4) {
            ForEach(Array(items.enumerated()), id: \.offset) { idx, item in
                HStack {
                    Text("\(idx + 1)")
                        .font(.caption.monospaced())
                        .foregroundStyle(.secondary)
                        .frame(width: 20)
                    Text(item)
                    Spacer()
                    if let details, idx < details.count {
                        Text(details[idx]).font(.caption).foregroundStyle(.secondary)
                    }
                }
                .padding(.vertical, 6)
                .padding(.horizontal, 12)
                .background(idx == highlightedIndex ? Color.accentColor.opacity(0.2) : Color.clear)
                .cornerRadius(6)
                .contentShape(Rectangle())
                .onTapGesture { selectItem(at: idx) }
            }
        }
    }

    private var taskStep: some View {
        TextField("Task name (e.g. fix-auth-bug)", text: $taskName)
            .textFieldStyle(.roundedBorder)
            .focused($taskFieldFocused)
            .onAppear { taskFieldFocused = true }
            .onSubmit { if !taskName.isEmpty { advance() } }
    }

    private var approveStep: some View {
        VStack(spacing: 12) {
            summaryRow("Project", selectedProject?.name ?? "—")
            summaryRow("Provider", selectedProvider?.name ?? "—")
            summaryRow("Task", taskName)
            summaryRow("Branch", branchStrategy == .worktree ? "Worktree" : "Main")

            HStack {
                Text("Auto-approve")
                Spacer()
                Toggle("", isOn: $autoApprove).labelsHidden()
            }
            .padding(.horizontal, 12)

            Text("Press Return to create · A to toggle auto-approve")
                .font(.caption).foregroundStyle(.secondary)
        }
    }

    private func summaryRow(_ label: String, _ value: String) -> some View {
        HStack {
            Text(label).foregroundStyle(.secondary)
            Spacer()
            Text(value).fontWeight(.medium)
        }
        .padding(.horizontal, 12)
    }

    private var stepLabel: String {
        switch step {
        case .project: "1/5 — Select project"
        case .provider: "2/5 — Select provider"
        case .task: "3/5 — Task name"
        case .branch: "4/5 — Branch strategy"
        case .approve: "5/5 — Confirm"
        }
    }

    // MARK: - Navigation Logic

    private var currentItemCount: Int {
        switch step {
        case .project: projects.count
        case .provider: providers.count
        case .branch: 2
        case .task, .approve: 0
        }
    }

    private func moveHighlight(_ delta: Int) {
        let count = currentItemCount
        guard count > 0 else { return }
        highlightedIndex = (highlightedIndex + delta + count) % count
    }

    private func confirmSelection() {
        switch step {
        case .task:
            if !taskName.isEmpty { advance() }
        case .approve:
            createSession()
        default:
            selectItem(at: highlightedIndex)
        }
    }

    private func selectItem(at index: Int) {
        switch step {
        case .project:
            guard index < projects.count else { return }
            selectedProject = projects[index]
            applyDefaults(for: projects[index])
            advance()
        case .provider:
            guard index < providers.count else { return }
            selectedProvider = providers[index]
            advance()
        case .branch:
            branchStrategy = index == 0 ? .worktree : .main
            advance()
        default:
            break
        }
    }

    private func handleDigit(_ chars: String) {
        if step == .approve, chars.lowercased() == "a" { return }
        guard let digit = Int(chars), digit >= 1, digit <= currentItemCount else { return }
        selectItem(at: digit - 1)
    }

    private func advance() {
        guard let next = Step(rawValue: step.rawValue + 1) else { return }
        highlightedIndex = 0
        step = next
    }

    private func goBack() {
        guard let prev = Step(rawValue: step.rawValue - 1) else { return }
        highlightedIndex = 0
        step = prev
    }

    // MARK: - Defaults & Create

    private func applyDefaults(for project: Project) {
        branchStrategy = project.defaultBranchStrategy
        autoApprove = project.defaultAutoApprove
        selectedProvider = providers.first { $0.command == project.defaultProvider } ?? providers.first
    }

    private func createSession() {
        guard let project = selectedProject, let provider = selectedProvider else {
            NSLog("PlaneAI: createSession guard failed — project=\(selectedProject?.name ?? "nil") provider=\(selectedProvider?.name ?? "nil")")
            return
        }
        isCreating = true
        errorMessage = nil

        do {
            let factory = SessionFactory(tmux: TmuxManager())
            let session = try factory.create(
                project: project,
                taskName: taskName,
                provider: provider,
                branchStrategy: branchStrategy,
                autoApprove: autoApprove
            )
            NSLog("PlaneAI: Session created: \(session.tmuxSession.name)")
            onCreate?(session)
            dismiss()
        } catch {
            NSLog("PlaneAI: Session creation failed: \(error)")
            errorMessage = error.localizedDescription
            isCreating = false
        }
    }
}
