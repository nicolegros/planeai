import Foundation

/// A single registerable command for the palette.
public struct Command: Identifiable, Sendable {
    public let id: String
    public let title: String
    public let icon: String
    public let keywords: [String]
    public let action: @Sendable () -> Void

    public init(id: String, title: String, icon: String, keywords: [String] = [], action: @Sendable @escaping () -> Void) {
        self.id = id
        self.title = title
        self.icon = icon
        self.keywords = keywords
        self.action = action
    }
}

/// Open registry of commands searchable by the palette.
public final class CommandRegistry {
    private var commands: [Command] = []

    public init() {}

    public func register(_ command: Command) {
        commands.removeAll { $0.id == command.id }
        commands.append(command)
    }

    public func register(_ batch: [Command]) {
        for cmd in batch { register(cmd) }
    }

    public func unregister(id: String) {
        commands.removeAll { $0.id == id }
    }

    /// Search commands by fuzzy matching title and keywords.
    public func search(query: String) -> [(command: Command, score: Int)] {
        if query.isEmpty {
            return commands.map { ($0, 1) }
        }
        return commands.compactMap { cmd in
            let targets = [cmd.title] + cmd.keywords
            let best = targets.compactMap { fuzzyMatch(query: query, target: $0) }.max()
            guard let score = best else { return nil }
            return (cmd, score)
        }
        .sorted { $0.score > $1.score }
    }

    public var all: [Command] { commands }
}
