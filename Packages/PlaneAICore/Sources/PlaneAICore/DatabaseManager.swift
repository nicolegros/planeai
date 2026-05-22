import Foundation
import GRDB

public enum DatabaseStorage {
    case inMemory
    case onDisk(path: String)
}

public final class DatabaseManager: Sendable {
    public let dbQueue: DatabaseQueue

    public init(storage: DatabaseStorage = .onDisk(path: DatabaseManager.defaultPath)) throws {
        switch storage {
        case .inMemory:
            dbQueue = try DatabaseQueue()
        case .onDisk(let path):
            let dir = (path as NSString).deletingLastPathComponent
            try FileManager.default.createDirectory(atPath: dir, withIntermediateDirectories: true)
            dbQueue = try DatabaseQueue(path: path)
        }
        try migrate(dbQueue)
    }

    public static var defaultPath: String {
        let appSupport = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        return appSupport.appendingPathComponent("planeai/planeai.db").path
    }
}

private func migrate(_ db: DatabaseQueue) throws {
    var migrator = DatabaseMigrator()
    migrator.eraseDatabaseOnSchemaChange = false
    migrator = migrator.disablingDeferredForeignKeyChecks()

    migrator.registerMigration("v1") { db in
        try db.create(table: "project", ifNotExists: true) { t in
            t.column("id", .text).primaryKey()
            t.column("name", .text).notNull().unique()
            t.column("repoPath", .text).notNull()
            t.column("defaultProvider", .text).notNull()
            t.column("defaultAutoApprove", .boolean).notNull()
            t.column("defaultBranchStrategy", .text).notNull()
        }

        try db.create(table: "session", ifNotExists: true) { t in
            t.column("id", .text).primaryKey()
            t.column("taskName", .text).notNull()
            t.column("branch", .text).notNull()
            t.column("provider", .text).notNull()
            t.column("state", .text).notNull()
            t.column("projectId", .text).references("project", onDelete: .setNull)
            t.column("projectName", .text).notNull()
            t.column("createdAt", .datetime).notNull()
            t.column("completedAt", .datetime)
            t.column("archivedAt", .datetime)
        }
    }

    try migrator.migrate(db)
}
