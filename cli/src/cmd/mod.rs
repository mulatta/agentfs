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

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod run;
#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
#[path = "run_stub.rs"]
mod run;

pub use mount::{mount, MountArgs};
pub use run::handle_run_command;
