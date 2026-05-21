import SwiftUI
import PlaneAICore

struct TabSwitcherView: View {
    let sessions: [SessionInfo]
    let selectedIndex: Int

    var body: some View {
        VStack(spacing: 4) {
            Text("Switch Session").font(.caption).foregroundStyle(.secondary)
            ForEach(Array(sessions.prefix(9).enumerated()), id: \.element.id) { idx, session in
                HStack(spacing: 8) {
                    Circle()
                        .fill(session.state == .running ? .green : .gray)
                        .frame(width: 6, height: 6)
                    Text(session.taskName)
                        .lineLimit(1)
                    Spacer()
                    Text(session.projectName)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                .padding(.horizontal, 12)
                .padding(.vertical, 6)
                .background(idx == selectedIndex ? Color.accentColor.opacity(0.3) : Color.clear)
                .cornerRadius(4)
            }
        }
        .padding(12)
        .frame(width: 300)
        .background(Color(nsColor: .windowBackgroundColor), in: RoundedRectangle(cornerRadius: 10))
        .overlay(RoundedRectangle(cornerRadius: 10).stroke(Color.secondary.opacity(0.3)))
        .shadow(radius: 20)
    }
}
