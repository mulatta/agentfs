pub mod completions;
pub mod fs;
pub mod init;

// Mount module selection:
// - Linux: always use FUSE (mount.rs)
// - macOS with force-fuse: use FUSE (mount.rs)
// - macOS without force-fuse: use FSKit (mount_fskit.rs)
// - Other platforms: use stub (mount_stub.rs)

#[cfg(target_os = "linux")]
mod mount;

#[cfg(all(target_os = "macos", feature = "force-fuse"))]
mod mount;

#[cfg(all(target_os = "macos", not(feature = "force-fuse")))]
#[path = "mount_fskit.rs"]
mod mount;

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
#[path = "mount_stub.rs"]
mod mount;

// Run module selection:
// - Linux x86_64: use overlay sandbox (run.rs)
// - macOS with force-fuse: use stub (not yet supported with FUSE)
// - macOS without force-fuse: use FSKit (run_fskit.rs)
// - Other platforms: use stub (run_stub.rs)

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod run;

#[cfg(all(target_os = "macos", not(feature = "force-fuse")))]
#[path = "run_fskit.rs"]
mod run;

#[cfg(not(any(
    all(target_os = "linux", target_arch = "x86_64"),
    all(target_os = "macos", not(feature = "force-fuse"))
)))]
#[path = "run_stub.rs"]
mod run;

pub use mount::{mount, MountArgs};
pub use run::handle_run_command;
