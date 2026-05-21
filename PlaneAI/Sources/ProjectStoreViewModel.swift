import SwiftUI
import PlaneAICore

@Observable
final class ProjectStoreViewModel {
    let store: ProjectStore
    var projects: [Project]
    var selectedProjectID: UUID?

    init(store: ProjectStore = ProjectStore()) {
        self.store = store
        self.projects = store.projects
    }

    func reload() {
        projects = store.projects
    }

    func delete(id: UUID) {
        try? store.delete(id: id)
        reload()
        if selectedProjectID == id { selectedProjectID = nil }
    }

    func rename(id: UUID, to newName: String) {
        try? store.rename(id: id, to: newName)
        reload()
    }
}
