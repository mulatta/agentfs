// AgentItem.swift
// FSItem implementation for AgentFS.
//
// Wraps a filesystem path and cached statistics.

import FSKit
import Foundation

final class AgentItem: FSItem {

    let volume: AgentVolume
    let path: String
    let isDirectory: Bool
    private var cachedStats: FFIStats?

    init(volume: AgentVolume, path: String, isDirectory: Bool, stats: FFIStats? = nil) {
        self.volume = volume
        self.path = path
        self.isDirectory = isDirectory
        self.cachedStats = stats
        super.init()
    }

    var currentAttributes: FSItem.Attributes {
        if let stats = cachedStats {
            return makeAttributes(from: stats)
        }
        // Return minimal attributes if no stats cached
        let attrs = FSItem.Attributes()
        attrs.type = isDirectory ? .directory : .file
        return attrs
    }

    func makeAttributes(from stats: FFIStats) -> FSItem.Attributes {
        let attrs = FSItem.Attributes()

        // Determine item type from mode
        let fileType = stats.mode & 0o170000
        switch fileType {
        case 0o040000:
            attrs.type = .directory
        case 0o120000:
            attrs.type = .symlink
        default:
            attrs.type = .file
        }

        // File permissions (lower 12 bits)
        attrs.mode = stats.mode & 0o7777

        // Link count
        attrs.linkCount = UInt32(stats.nlink)

        // Ownership
        attrs.uid = stats.uid
        attrs.gid = stats.gid

        // Size
        attrs.size = UInt64(stats.size)

        // Allocated size (round up to block size)
        let blockSize: Int64 = 4096
        let blocks = (stats.size + blockSize - 1) / blockSize
        attrs.allocSize = UInt64(blocks * blockSize)

        // Timestamps (using timespec)
        attrs.accessTime = timespec(tv_sec: Int(stats.atime), tv_nsec: 0)
        attrs.modifyTime = timespec(tv_sec: Int(stats.mtime), tv_nsec: 0)
        attrs.changeTime = timespec(tv_sec: Int(stats.ctime), tv_nsec: 0)
        attrs.birthTime = timespec(tv_sec: Int(stats.ctime), tv_nsec: 0)

        // Inode / file ID
        if let fileID = FSItem.Identifier(rawValue: UInt64(bitPattern: stats.ino)) {
            attrs.fileID = fileID
        }

        return attrs
    }

    func updateStats(_ stats: FFIStats) {
        self.cachedStats = stats
    }
}
