import Foundation

/// Specification of a known agent CLI provider.
public struct ProviderSpec: Sendable, Equatable, Hashable {
    public let name: String
    public let command: String
    public let arguments: [String]
    public let autoApproveFlag: String

    public init(name: String, command: String, arguments: [String] = [], autoApproveFlag: String) {
        self.name = name
        self.command = command
        self.arguments = arguments
        self.autoApproveFlag = autoApproveFlag
    }
}

/// Well-known providers shipped with planeai.
extension ProviderSpec {
    public static let builtIn: [ProviderSpec] = [
        ProviderSpec(name: "Claude Code", command: "claude", autoApproveFlag: "--dangerously-skip-permissions"),
        ProviderSpec(name: "Kiro", command: "kiro-cli", arguments: ["chat"], autoApproveFlag: "--trust-all-tools"),
        ProviderSpec(name: "Codex", command: "codex", autoApproveFlag: "--auto-approve"),
    ]
}

/// Detects which providers are available on the system PATH.
public enum ProviderDetector {
    public static func detect(knownProviders: [ProviderSpec] = ProviderSpec.builtIn) -> [ProviderSpec] {
        knownProviders.filter { isOnPATH($0.command) }
    }

    private static func isOnPATH(_ command: String) -> Bool {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/env")
        process.arguments = ["which", command]
        process.environment = UserEnvironment.processEnvironment
        process.standardOutput = FileHandle.nullDevice
        process.standardError = FileHandle.nullDevice
        do {
            try process.run()
            process.waitUntilExit()
            return process.terminationStatus == 0
        } catch {
            return false
        }
    }
}
