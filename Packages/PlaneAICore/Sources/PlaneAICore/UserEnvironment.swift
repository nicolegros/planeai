import Foundation
import os

private let logger = Logger(subsystem: "ca.nicolegros.planeai", category: "UserEnvironment")

/// Resolves and caches the user's login shell PATH for use in GUI apps.
public enum UserEnvironment {
    /// The user's full PATH from their login shell. Cached after first call.
    public static let path: String = {
        // Try the user's login shell first, fall back to /bin/sh
        let shell: String = {
            if let s = ProcessInfo.processInfo.environment["SHELL"], !s.isEmpty { return s }
            // GUI apps may not have SHELL set — query directory services
            let p = Process()
            let pipe = Pipe()
            p.executableURL = URL(fileURLWithPath: "/usr/bin/dscl")
            p.arguments = [".", "-read", "/Users/\(NSUserName())", "UserShell"]
            p.standardOutput = pipe
            p.standardError = FileHandle.nullDevice
            try? p.run()
            p.waitUntilExit()
            let out = String(data: pipe.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8) ?? ""
            if let path = out.split(separator: " ").last { return String(path).trimmingCharacters(in: .whitespacesAndNewlines) }
            return "/bin/sh"
        }()

        logger.info("Resolved login shell: \(shell)")

        let args: [String]
        if shell.hasSuffix("fish") {
            args = ["-lc", "echo $PATH"]
        } else {
            args = ["-lc", "echo $PATH"]
        }

        let process = Process()
        let pipe = Pipe()
        process.executableURL = URL(fileURLWithPath: shell)
        process.arguments = args
        process.standardOutput = pipe
        process.standardError = FileHandle.nullDevice
        do {
            try process.run()
            process.waitUntilExit()
            let data = pipe.fileHandleForReading.readDataToEndOfFile()
            let raw = String(data: data, encoding: .utf8)?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
            // fish uses spaces as PATH separator, convert to colon-separated
            var result: String
            if shell.hasSuffix("fish") && !raw.contains(":") && raw.contains(" ") {
                result = raw.replacingOccurrences(of: " ", with: ":")
            } else {
                result = raw
            }
            if result.isEmpty {
                result = "/usr/local/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin"
            }
            logger.info("Resolved PATH: \(result)")
            return result
        } catch {
            logger.error("Failed to resolve PATH: \(error.localizedDescription)")
            return "/usr/local/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin"
        }
    }()

    /// Environment dictionary suitable for Process with the user's PATH.
    public static let processEnvironment: [String: String] = {
        var env = ProcessInfo.processInfo.environment
        env["PATH"] = path
        return env
    }()

    /// The user's login shell path.
    public static let shell: String = {
        if let s = ProcessInfo.processInfo.environment["SHELL"], !s.isEmpty { return s }
        return "/bin/zsh"
    }()

    /// Resolves the absolute path of a binary using the user's PATH.
    public static func which(_ binary: String) -> String? {
        let process = Process()
        let pipe = Pipe()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/env")
        process.arguments = ["which", binary]
        process.environment = processEnvironment
        process.standardOutput = pipe
        process.standardError = FileHandle.nullDevice
        do {
            try process.run()
            process.waitUntilExit()
            guard process.terminationStatus == 0 else { return nil }
            let data = pipe.fileHandleForReading.readDataToEndOfFile()
            return String(data: data, encoding: .utf8)?.trimmingCharacters(in: .whitespacesAndNewlines)
        } catch {
            return nil
        }
    }
}
