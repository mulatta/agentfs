// AgentFS Host App
// This minimal app exists to install and manage the AgentFS FSKit extension.

import Cocoa

struct AgentFSApp {
    static func main() {
        let app = NSApplication.shared
        app.setActivationPolicy(.regular)

        // Create a simple window
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 400, height: 200),
            styleMask: [.titled, .closable, .miniaturizable],
            backing: .buffered,
            defer: false
        )
        window.title = "AgentFS"
        window.center()

        // Create content view with instructions
        let textView = NSTextView(frame: NSRect(x: 20, y: 20, width: 360, height: 160))
        textView.isEditable = false
        textView.backgroundColor = .clear
        textView.string = """
        AgentFS Extension

        The AgentFS file system extension is now installed.

        To enable it:
        1. Open System Settings
        2. Go to General > Login Items & Extensions
        3. Enable "AgentFS" under File System Extensions

        Once enabled, you can mount AgentFS databases using:
            agentfs mount <database> <mountpoint>
        """

        window.contentView = textView
        window.makeKeyAndOrderFront(nil)

        app.run()
    }
}

// Entry point
AgentFSApp.main()
