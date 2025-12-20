// AgentFileSystem.swift
// FSUnaryFileSystem implementation for AgentFS.
//
// This class handles probing and loading AgentFS database resources.

import FSKit
import os

final class AgentFileSystem: FSUnaryFileSystem, FSUnaryFileSystemOperations {

    private let logger = Logger(subsystem: "io.turso.agentfs", category: "FileSystem")

    // MARK: - FSUnaryFileSystemOperations

    func probeResource(
        resource: FSResource,
        replyHandler: @escaping (FSProbeResult?, Error?) -> Void
    ) {
        logger.info("Probing resource: \(String(describing: resource))")

        // For FSGenericURLResource, extract the database path from URL
        guard let urlResource = resource as? FSGenericURLResource else {
            logger.error("Resource is not FSGenericURLResource")
            replyHandler(nil, NSError(domain: NSPOSIXErrorDomain, code: Int(EINVAL)))
            return
        }

        let url = urlResource.url
        let dbPath = url.path

        logger.info("Probing database at: \(dbPath)")

        // Validate the database exists
        guard FileManager.default.fileExists(atPath: dbPath) else {
            logger.error("Database file does not exist: \(dbPath)")
            replyHandler(nil, NSError(domain: NSPOSIXErrorDomain, code: Int(ENOENT)))
            return
        }

        // Return probe result indicating this is a usable AgentFS database
        let containerID = FSContainerIdentifier(uuid: UUID())
        let result = FSProbeResult.usable(name: "AgentFS", containerID: containerID)

        logger.info("Probe successful for: \(dbPath)")
        replyHandler(result, nil)
    }

    func loadResource(
        resource: FSResource,
        options: FSTaskOptions,
        replyHandler: @escaping (FSVolume?, Error?) -> Void
    ) {
        logger.info("Loading resource")

        guard let urlResource = resource as? FSGenericURLResource else {
            logger.error("Resource is not FSGenericURLResource")
            replyHandler(nil, NSError(domain: NSPOSIXErrorDomain, code: Int(EINVAL)))
            return
        }

        let dbPath = urlResource.url.path
        logger.info("Loading database: \(dbPath)")

        // Open the AgentFS database via FFI
        guard let handle = dbPath.withCString({ agentfs_open($0) }) else {
            logger.error("Failed to open AgentFS database at \(dbPath)")
            replyHandler(nil, NSError(domain: NSPOSIXErrorDomain, code: Int(EIO)))
            return
        }

        logger.info("Database opened successfully")

        // Create and return the volume
        let volume = AgentVolume(handle: handle, dbPath: dbPath)
        replyHandler(volume, nil)
    }

    func unloadResource(
        resource: FSResource,
        options: FSTaskOptions,
        replyHandler: @escaping (Error?) -> Void
    ) {
        logger.info("Unloading resource")
        replyHandler(nil)
    }

    func didFinishLoading() {
        logger.info("Extension finished loading")
    }
}
