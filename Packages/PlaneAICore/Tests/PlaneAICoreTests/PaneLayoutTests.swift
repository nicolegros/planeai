import Foundation
import Testing
@testable import PlaneAICore

@Suite("PaneLayout")
struct PaneLayoutTests {

    @Test func primaryPaneIsTracked() {
        let layout = PaneLayout(sessionName: "test", primaryPaneId: "%0")
        #expect(layout.isPrimary("%0"))
        #expect(!layout.isPrimary("%1"))
    }

    @Test func secondaryPanesExcludePrimary() {
        let layout = PaneLayout(sessionName: "test", primaryPaneId: "%0")
        layout.addPane("%1")
        layout.addPane("%2")
        #expect(layout.secondaryPaneIds == ["%1", "%2"])
    }

    @Test func removePaneUpdatesIds() {
        let layout = PaneLayout(sessionName: "test", primaryPaneId: "%0")
        layout.addPane("%1")
        layout.removePane("%1")
        #expect(layout.paneIds == ["%0"])
    }

    @Test func addPaneIsIdempotent() {
        let layout = PaneLayout(sessionName: "test", primaryPaneId: "%0")
        layout.addPane("%0")
        #expect(layout.paneIds.count == 1)
    }
}
