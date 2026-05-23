import SwiftUI
import PlaneAICore

struct ProjectSidebarView: View {
    @Bindable var coordinator: AppCoordinator
    @State private var showingAddSheet = false
    @State private var renamingProject: Project?
    @State private var renameText = ""

    var body: some View {
        List(selection: $coordinator.selectedProjectID) {
            Section("Projects") {
                ForEach(coordinator.projects) { project in
                    Text(project.name)
                        .tag(project.id)
                        .contextMenu {
                            Button("Rename…") {
                                renameText = project.name
                                renamingProject = project
                            }
                            Divider()
                            Button("Delete", role: .destructive) {
                                try? coordinator.deleteProject(id: project.id)
                            }
                        }
                }
            }
        }
        .listStyle(.sidebar)
        .frame(minWidth: 180)
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Button(action: { showingAddSheet = true }) {
                    Label("Add Project", systemImage: "plus")
                }
                .keyboardShortcut("n", modifiers: [.command, .shift])
            }
        }
        .sheet(isPresented: $showingAddSheet) {
            AddProjectSheet(coordinator: coordinator)
        }
        .alert("Rename Project", isPresented: .init(
            get: { renamingProject != nil },
            set: { if !$0 { renamingProject = nil } }
        )) {
            TextField("Name", text: $renameText)
            Button("Cancel", role: .cancel) { renamingProject = nil }
            Button("Rename") {
                if let project = renamingProject {
                    try? coordinator.renameProject(id: project.id, to: renameText)
                }
                renamingProject = nil
            }
        }
    }
}

struct AddProjectSheet: View {
    @Bindable var coordinator: AppCoordinator
    @Environment(\.dismiss) private var dismiss
    @State private var name = ""
    @State private var repoPath = ""
    @State private var errorMessage: String?

    var body: some View {
        VStack(spacing: 16) {
            Text("Add Project").font(.headline)

            TextField("Project Name", text: $name)
                .textFieldStyle(.roundedBorder)

            HStack {
                TextField("Repository Path", text: $repoPath)
                    .textFieldStyle(.roundedBorder)
                Button("Browse…") { browseForRepo() }
            }

            if let errorMessage {
                Text(errorMessage)
                    .foregroundStyle(.red)
                    .font(.caption)
            }

            HStack {
                Button("Cancel", role: .cancel) { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button("Add") { addProject() }
                    .keyboardShortcut(.defaultAction)
                    .disabled(name.isEmpty || repoPath.isEmpty)
            }
        }
        .padding()
        .frame(width: 400)
    }

    private func browseForRepo() {
        let panel = NSOpenPanel()
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.allowsMultipleSelection = false
        panel.message = "Select a git repository"
        if panel.runModal() == .OK, let url = panel.url {
            repoPath = url.path
            if name.isEmpty {
                name = url.lastPathComponent
            }
        }
    }

    private func addProject() {
        do {
            try GitRepoValidator.validate(path: repoPath)
            try coordinator.addProject(
                name: name,
                repoPath: repoPath,
                defaultProvider: "",
                defaultAutoApprove: false,
                defaultBranchStrategy: .worktree
            )
            dismiss()
        } catch ProjectStoreError.invalidGitRepo(let path) {
            errorMessage = "Not a git repository: \(path)"
        } catch ProjectStoreError.duplicateName(let n) {
            errorMessage = "Project '\(n)' already exists"
        } catch {
            errorMessage = error.localizedDescription
        }
    }
}
