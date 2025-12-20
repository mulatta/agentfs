// AgentVolume.swift
// FSVolume implementation for AgentFS.
//
// This class handles all filesystem operations by calling into the Rust FFI layer.

import FSKit
import Foundation
import os

final class AgentVolume: FSVolume {

    private let logger = Logger(subsystem: "io.turso.agentfs", category: "Volume")
    private let handle: OpaquePointer
    private let dbPath: String
    private var _rootItem: AgentItem?

    init(handle: OpaquePointer, dbPath: String) {
        self.handle = handle
        self.dbPath = dbPath

        let volumeID = FSVolume.Identifier(uuid: UUID())
        let volumeName = FSFileName(string: "AgentFS")

        super.init(volumeID: volumeID, volumeName: volumeName)

        // Create root item
        self._rootItem = AgentItem(volume: self, path: "/", isDirectory: true)
    }

    deinit {
        logger.info("Closing AgentFS handle")
        // The handle is an OpaquePointer which can be passed directly to the FFI function
        // that expects UnsafeMutablePointer<AgentFSHandle>?
        agentfs_close(handle)
    }

    // MARK: - Internal helpers

    func getHandle() -> OpaquePointer {
        return handle
    }
}

// MARK: - FSVolume.Operations
extension AgentVolume: FSVolume.Operations {

    // MARK: Required properties

    var supportedVolumeCapabilities: FSVolume.SupportedCapabilities {
        let caps = FSVolume.SupportedCapabilities()
        caps.supportsHardLinks = false
        caps.supportsSymbolicLinks = true
        caps.supportsPersistentObjectIDs = true
        caps.supports64BitObjectIDs = true
        return caps
    }

    var volumeStatistics: FSStatFSResult {
        let result = FSStatFSResult(fileSystemTypeName: "agentfs")

        var ffiStats = FFIFilesystemStats()
        let ok = agentfs_statfs(handle, &ffiStats)
        if ok.success {
            // Use sensible defaults for block-based stats
            let blockSize: Int = 4096
            result.blockSize = blockSize
            result.ioSize = blockSize

            // Calculate blocks from bytes
            let totalBytes = ffiStats.bytes_used + (1024 * 1024 * 1024)  // Assume 1GB capacity
            result.totalBlocks = UInt64(totalBytes) / UInt64(blockSize)
            result.freeBlocks = UInt64(1024 * 1024 * 1024 - Int64(ffiStats.bytes_used)) / UInt64(blockSize)
            result.availableBlocks = result.freeBlocks
            result.totalBytes = totalBytes
            result.usedBytes = ffiStats.bytes_used
            result.freeBytes = UInt64(1024 * 1024 * 1024) - ffiStats.bytes_used
            result.availableBytes = result.freeBytes

            result.totalFiles = ffiStats.inodes + 1000  // Some headroom
            result.freeFiles = 1000
        } else {
            // Defaults if statfs fails
            result.blockSize = 4096
            result.ioSize = 4096
            result.totalBlocks = 1024 * 1024  // 4GB
            result.freeBlocks = 1024 * 1024
            result.availableBlocks = 1024 * 1024
        }

        return result
    }

    // MARK: PathConfOperations properties (required by Operations)

    var maximumLinkCount: Int { 1 }
    var maximumNameLength: Int { 255 }
    var restrictsOwnershipChanges: Bool { true }
    var truncatesLongNames: Bool { false }

    // MARK: Operations methods

    func activate(options: FSTaskOptions) async throws -> FSItem {
        logger.info("Activating volume: \(self.dbPath)")
        guard let root = _rootItem else {
            throw posixError(EIO)
        }
        return root
    }

    func deactivate(options: FSDeactivateOptions) async throws {
        logger.info("Deactivating volume: \(self.dbPath)")
    }

    func mount(options: FSTaskOptions) async throws {
        logger.info("Mounting volume: \(self.dbPath)")
    }

    func unmount() async {
        logger.info("Unmounting volume: \(self.dbPath)")
    }

    func synchronize(flags: FSSyncFlags) async throws {
        logger.debug("Synchronizing volume")
        let result = "/".withCString { agentfs_fsync(handle, $0) }
        if !result.success {
            throw posixError(EIO)
        }
    }

    func attributes(_ request: FSItem.GetAttributesRequest, of item: FSItem) async throws -> FSItem.Attributes {
        guard let agentItem = item as? AgentItem else {
            throw posixError(EINVAL)
        }

        var stats = FFIStats()
        let result = agentItem.path.withCString { agentfs_stat(handle, $0, &stats) }

        guard result.success else {
            throw posixError(result.error_code)
        }

        return agentItem.makeAttributes(from: stats)
    }

    func setAttributes(_ request: FSItem.SetAttributesRequest, on item: FSItem) async throws -> FSItem.Attributes {
        guard let agentItem = item as? AgentItem else {
            throw posixError(EINVAL)
        }

        // Handle truncate via setAttributes
        if request.isValid(.size) {
            let result = agentItem.path.withCString { agentfs_truncate(handle, $0, request.size) }
            if !result.success {
                throw posixError(result.error_code)
            }
            request.consumedAttributes.insert(.size)
        }

        // Return updated attributes
        var stats = FFIStats()
        guard agentItem.path.withCString({ agentfs_stat(handle, $0, &stats) }).success else {
            throw posixError(EIO)
        }
        return agentItem.makeAttributes(from: stats)
    }

    func lookupItem(named name: FSFileName, inDirectory directory: FSItem) async throws -> (FSItem, FSFileName) {
        guard let parentItem = directory as? AgentItem,
              let nameString = name.string else {
            throw posixError(EINVAL)
        }

        let childPath: String
        if parentItem.path == "/" {
            childPath = "/\(nameString)"
        } else {
            childPath = "\(parentItem.path)/\(nameString)"
        }

        var stats = FFIStats()
        let result = childPath.withCString { agentfs_stat(handle, $0, &stats) }

        guard result.success else {
            throw posixError(result.error_code)
        }

        let isDir = (stats.mode & 0o170000) == 0o040000
        let item = AgentItem(volume: self, path: childPath, isDirectory: isDir, stats: stats)
        return (item, name)
    }

    func reclaimItem(_ item: FSItem) async throws {
        // Nothing to do - items are stateless
    }

    func readSymbolicLink(_ item: FSItem) async throws -> FSFileName {
        guard let agentItem = item as? AgentItem else {
            throw posixError(EINVAL)
        }

        var targetPtr: UnsafeMutablePointer<CChar>?
        let result = agentItem.path.withCString { agentfs_readlink(handle, $0, &targetPtr) }

        guard result.success, let target = targetPtr else {
            throw posixError(result.error_code)
        }

        defer { agentfs_free_string(targetPtr) }
        return FSFileName(string: String(cString: target))
    }

    func createItem(
        named name: FSFileName,
        type: FSItem.ItemType,
        inDirectory directory: FSItem,
        attributes: FSItem.SetAttributesRequest
    ) async throws -> (FSItem, FSFileName) {
        guard let parentItem = directory as? AgentItem,
              let nameString = name.string else {
            throw posixError(EINVAL)
        }

        let newPath: String
        if parentItem.path == "/" {
            newPath = "/\(nameString)"
        } else {
            newPath = "\(parentItem.path)/\(nameString)"
        }

        let result: FFIResult
        switch type {
        case .directory:
            result = newPath.withCString { agentfs_mkdir(handle, $0) }
        case .file:
            // Create empty file via pwrite with 0 bytes
            result = newPath.withCString { agentfs_pwrite(handle, $0, 0, nil, 0) }
        default:
            throw posixError(ENOTSUP)
        }

        guard result.success else {
            throw posixError(result.error_code)
        }

        var stats = FFIStats()
        guard newPath.withCString({ agentfs_stat(handle, $0, &stats) }).success else {
            throw posixError(EIO)
        }

        let item = AgentItem(volume: self, path: newPath, isDirectory: type == .directory, stats: stats)
        return (item, name)
    }

    func createSymbolicLink(
        named name: FSFileName,
        inDirectory directory: FSItem,
        attributes: FSItem.SetAttributesRequest,
        linkContents: FSFileName
    ) async throws -> (FSItem, FSFileName) {
        guard let parentItem = directory as? AgentItem,
              let nameString = name.string,
              let targetString = linkContents.string else {
            throw posixError(EINVAL)
        }

        let linkPath: String
        if parentItem.path == "/" {
            linkPath = "/\(nameString)"
        } else {
            linkPath = "\(parentItem.path)/\(nameString)"
        }

        let result = targetString.withCString { targetCStr in
            linkPath.withCString { linkCStr in
                agentfs_symlink(handle, targetCStr, linkCStr)
            }
        }

        guard result.success else {
            throw posixError(result.error_code)
        }

        var stats = FFIStats()
        guard linkPath.withCString({ agentfs_lstat(handle, $0, &stats) }).success else {
            throw posixError(EIO)
        }

        let item = AgentItem(volume: self, path: linkPath, isDirectory: false, stats: stats)
        return (item, name)
    }

    func createLink(to item: FSItem, named name: FSFileName, inDirectory directory: FSItem) async throws -> FSFileName {
        // AgentFS doesn't support hard links
        throw posixError(ENOTSUP)
    }

    func removeItem(_ item: FSItem, named name: FSFileName, fromDirectory directory: FSItem) async throws {
        guard let agentItem = item as? AgentItem else {
            throw posixError(EINVAL)
        }

        let result = agentItem.path.withCString { agentfs_remove(handle, $0) }
        guard result.success else {
            throw posixError(result.error_code)
        }
    }

    func renameItem(
        _ item: FSItem,
        inDirectory sourceDirectory: FSItem,
        named sourceName: FSFileName,
        to destinationName: FSFileName,
        inDirectory destinationDirectory: FSItem,
        overItem: FSItem?
    ) async throws -> FSFileName {
        guard let srcItem = item as? AgentItem,
              let dstDir = destinationDirectory as? AgentItem,
              let destNameString = destinationName.string else {
            throw posixError(EINVAL)
        }

        let newPath: String
        if dstDir.path == "/" {
            newPath = "/\(destNameString)"
        } else {
            newPath = "\(dstDir.path)/\(destNameString)"
        }

        let result = srcItem.path.withCString { fromCStr in
            newPath.withCString { toCStr in
                agentfs_rename(handle, fromCStr, toCStr)
            }
        }

        guard result.success else {
            throw posixError(result.error_code)
        }

        return destinationName
    }

    func enumerateDirectory(
        _ directory: FSItem,
        startingAt cookie: FSDirectoryCookie,
        verifier: FSDirectoryVerifier,
        attributes: FSItem.GetAttributesRequest?,
        packer: FSDirectoryEntryPacker
    ) async throws -> FSDirectoryVerifier {
        guard let dirItem = directory as? AgentItem else {
            throw posixError(EINVAL)
        }

        var entriesPtr: UnsafeMutablePointer<CChar>?
        let result = dirItem.path.withCString { agentfs_readdir(handle, $0, &entriesPtr) }

        guard result.success, let json = entriesPtr else {
            throw posixError(result.error_code)
        }

        defer { agentfs_free_string(entriesPtr) }

        // Parse JSON array of entry names
        let jsonString = String(cString: json)
        guard let jsonData = jsonString.data(using: .utf8),
              let entries = try? JSONDecoder().decode([String].self, from: jsonData) else {
            throw posixError(EIO)
        }

        // If no attributes requested, include . and .. entries
        let cookieValue = cookie.rawValue
        if attributes == nil {
            if cookie == .initial {
                // Pack "." entry
                let dotName = FSFileName(string: ".")
                let packed = packer.packEntry(
                    name: dotName,
                    itemType: .directory,
                    itemID: .rootDirectory,
                    nextCookie: FSDirectoryCookie(rawValue: 1),
                    attributes: nil
                )
                if !packed {
                    return FSDirectoryVerifier(rawValue: 1)
                }
            }
            if cookieValue <= 1 {
                // Pack ".." entry
                let dotDotName = FSFileName(string: "..")
                let packed = packer.packEntry(
                    name: dotDotName,
                    itemType: .directory,
                    itemID: .parentOfRoot,
                    nextCookie: FSDirectoryCookie(rawValue: 2),
                    attributes: nil
                )
                if !packed {
                    return FSDirectoryVerifier(rawValue: 1)
                }
            }
        }

        let startIndex = attributes == nil ? Int(cookieValue) - 2 : Int(cookieValue)

        for (index, name) in entries.enumerated() {
            guard index >= startIndex else { continue }

            let childPath: String
            if dirItem.path == "/" {
                childPath = "/\(name)"
            } else {
                childPath = "\(dirItem.path)/\(name)"
            }

            var stats = FFIStats()
            guard childPath.withCString({ agentfs_stat(handle, $0, &stats) }).success else {
                continue
            }

            let isDir = (stats.mode & 0o170000) == 0o040000
            let item = AgentItem(volume: self, path: childPath, isDirectory: isDir, stats: stats)

            let itemType: FSItem.ItemType = isDir ? .directory : .file
            let nextCookieValue: UInt64 = attributes == nil ? UInt64(index + 3) : UInt64(index + 1)
            let nextCookie = FSDirectoryCookie(rawValue: nextCookieValue)

            let fileName = FSFileName(string: name)

            let itemID = FSItem.Identifier(rawValue: UInt64(bitPattern: stats.ino))!

            let packed = packer.packEntry(
                name: fileName,
                itemType: itemType,
                itemID: itemID,
                nextCookie: nextCookie,
                attributes: attributes != nil ? item.currentAttributes : nil
            )

            if !packed { break }
        }

        return FSDirectoryVerifier(rawValue: 1)
    }
}

// MARK: - FSVolume.OpenCloseOperations
extension AgentVolume: FSVolume.OpenCloseOperations {

    func openItem(_ item: FSItem, modes: FSVolume.OpenModes) async throws {
        // AgentFS handles files statelessly via path - nothing to do
        logger.debug("Opening item: \((item as? AgentItem)?.path ?? "unknown")")
    }

    func closeItem(_ item: FSItem, modes: FSVolume.OpenModes) async throws {
        // Stateless - nothing to do
        logger.debug("Closing item: \((item as? AgentItem)?.path ?? "unknown")")
    }
}

// MARK: - FSVolume.ReadWriteOperations
extension AgentVolume: FSVolume.ReadWriteOperations {

    func read(
        from item: FSItem,
        at offset: off_t,
        length: Int,
        into buffer: FSMutableFileDataBuffer
    ) async throws -> Int {
        guard let agentItem = item as? AgentItem else {
            throw posixError(EINVAL)
        }

        var ffiBuffer = FFIBuffer()
        let result = agentItem.path.withCString {
            agentfs_pread(handle, $0, UInt64(offset), UInt64(length), &ffiBuffer)
        }

        guard result.success else {
            throw posixError(result.error_code)
        }

        defer { agentfs_free_buffer(ffiBuffer) }

        let bytesToCopy = min(Int(ffiBuffer.len), length, Int(buffer.length))
        if bytesToCopy > 0, let sourceData = ffiBuffer.data {
            // Access mutableBytes via Obj-C selector since Swift API is unavailable
            let selector = NSSelectorFromString("mutableBytes")
            if buffer.responds(to: selector),
               let destRawPtr = buffer.perform(selector)?.toOpaque() {
                let destPtr = UnsafeMutableRawPointer(destRawPtr)
                destPtr.copyMemory(from: sourceData, byteCount: bytesToCopy)
            }
        }

        return bytesToCopy
    }

    func write(
        contents: Data,
        to item: FSItem,
        at offset: off_t
    ) async throws -> Int {
        guard let agentItem = item as? AgentItem else {
            throw posixError(EINVAL)
        }

        let result = contents.withUnsafeBytes { (bytes: UnsafeRawBufferPointer) -> FFIResult in
            agentItem.path.withCString { pathCStr in
                agentfs_pwrite(
                    handle,
                    pathCStr,
                    UInt64(offset),
                    bytes.baseAddress?.assumingMemoryBound(to: UInt8.self),
                    contents.count
                )
            }
        }

        guard result.success else {
            throw posixError(result.error_code)
        }

        return contents.count
    }
}

// MARK: - Helper Functions

private func posixError(_ errno: Int32) -> NSError {
    return NSError(domain: NSPOSIXErrorDomain, code: Int(errno))
}
