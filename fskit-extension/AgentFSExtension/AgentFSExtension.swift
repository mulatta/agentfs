// AgentFSExtension.swift
// Main entry point for the AgentFS FSKit extension.
//
// This is a user-space filesystem extension that exposes AgentFS databases
// as mountable filesystems on macOS 26+ without requiring kernel extensions.

import FSKit
import Foundation
import os

@main
class AgentFSExtensionMain {
    static let logger = Logger(subsystem: "io.turso.agentfs", category: "Extension")

    static func main() {
        logger.info("Starting AgentFS extension")

        // Create the file system and run the extension
        let fileSystem = AgentFileSystem()

        // FSKit extensions run via XPC - the system manages the lifecycle
        RunLoop.main.run()
    }
}
