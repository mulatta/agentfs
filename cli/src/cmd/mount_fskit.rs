//! FSKit-based mount implementation for macOS 26+.
//!
//! This module is only compiled on macOS when the `force-fuse` feature is NOT enabled.
//! It uses Apple's FSKit framework for user-space filesystem mounting without kernel extensions.

#![cfg(all(target_os = "macos", not(feature = "force-fuse")))]

use agentfs_sdk::AgentFSOptions;
use anyhow::Result;
use std::path::PathBuf;
use std::process::Command;

/// Arguments for the mount command.
#[derive(Debug, Clone)]
pub struct MountArgs {
    /// The agent filesystem ID or path.
    pub id_or_path: String,
    /// The mountpoint path.
    pub mountpoint: PathBuf,
    /// Automatically unmount when the process exits.
    pub auto_unmount: bool,
    /// Allow root to access the mount.
    pub allow_root: bool,
    /// Run in foreground (don't daemonize).
    pub foreground: bool,
    /// User ID to report for all files (defaults to current user).
    pub uid: Option<u32>,
    /// Group ID to report for all files (defaults to current group).
    pub gid: Option<u32>,
}

/// Mount the agent filesystem using FSKit.
///
/// This requires:
/// - macOS 26 or later
/// - The AgentFS FSKit extension to be installed and enabled
pub fn mount(args: MountArgs) -> Result<()> {
    // Check macOS version
    if !supports_fskit()? {
        anyhow::bail!(
            "FSKit requires macOS 26 or later.\n\
             You can use the `--features force-fuse` flag to use macFUSE instead."
        );
    }

    // Check if extension is installed
    if !is_extension_installed() {
        anyhow::bail!(
            "AgentFS FSKit extension is not installed.\n\
             \n\
             To install:\n\
             1. Build the extension: cd fskit-ffi && make build-extension\n\
             2. Install the extension app bundle\n\
             3. Enable it via: System Settings > General > Login Items & Extensions\n\
                > File System Extensions > AgentFS\n\
             \n\
             Alternatively, use macFUSE with: cargo build --features force-fuse"
        );
    }

    // Resolve the database path
    let db_path = resolve_db_path(&args.id_or_path)?;

    // Validate mountpoint exists
    if !args.mountpoint.exists() {
        anyhow::bail!("Mountpoint does not exist: {}", args.mountpoint.display());
    }

    // Mount using FSKit's mount command
    // FSKit filesystems are mounted via: mount -t <fstype> <resource> <mountpoint>
    // The resource is a URL for FSGenericURLResource
    let resource_url = format!("file://{}", db_path);

    eprintln!("Mounting {} at {}", db_path, args.mountpoint.display());

    let mut cmd = Command::new("/sbin/mount");
    cmd.arg("-t").arg("agentfs")
        .arg(&resource_url)
        .arg(&args.mountpoint);

    let output = cmd.output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "Mount failed: {}\n\
             \n\
             Make sure the AgentFS FSKit extension is installed and enabled.",
            stderr.trim()
        );
    }

    eprintln!("Mounted successfully!");

    if args.foreground {
        // Wait for unmount signal
        wait_for_unmount(&args.mountpoint)?;
    }

    Ok(())
}

/// Resolve the database path from an ID or path.
fn resolve_db_path(id_or_path: &str) -> Result<String> {
    let opts = AgentFSOptions::resolve(id_or_path)?;

    if let Some(path) = opts.path {
        Ok(std::fs::canonicalize(&path)?.to_string_lossy().to_string())
    } else {
        anyhow::bail!("Cannot mount ephemeral filesystem")
    }
}

/// Check if macOS version supports FSKit (26+).
fn supports_fskit() -> Result<bool> {
    let output = Command::new("sw_vers")
        .arg("-productVersion")
        .output()?;

    if !output.status.success() {
        return Ok(false);
    }

    let version = String::from_utf8_lossy(&output.stdout);
    let major: u32 = version
        .trim()
        .split('.')
        .next()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    // FSKit with FSGenericURLResource requires macOS 26+
    Ok(major >= 26)
}

/// Check if the AgentFS FSKit extension is installed.
fn is_extension_installed() -> bool {
    let output = Command::new("systemextensionsctl")
        .arg("list")
        .output()
        .ok();

    if let Some(output) = output {
        let stdout = String::from_utf8_lossy(&output.stdout);
        // Check for our bundle identifier
        stdout.contains("io.turso.agentfs") || stdout.contains("AgentFS")
    } else {
        false
    }
}

/// Wait for the filesystem to be unmounted.
fn wait_for_unmount(mountpoint: &PathBuf) -> Result<()> {
    use std::os::unix::fs::MetadataExt;

    eprintln!("Running in foreground. Press Ctrl+C to unmount.");

    // Get the device ID of the mounted filesystem
    let mounted_dev = std::fs::metadata(mountpoint)?.dev();

    // Poll for unmount
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));

        match std::fs::metadata(mountpoint) {
            Ok(meta) => {
                // Check if device ID changed (unmounted)
                if meta.dev() != mounted_dev {
                    break;
                }
            }
            Err(_) => break, // Mountpoint gone or inaccessible
        }
    }

    eprintln!("Unmounted.");
    Ok(())
}
