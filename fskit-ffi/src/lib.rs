//! C-compatible FFI layer exposing the FileSystem trait for FSKit Swift extension.
//!
//! This crate provides a stable C ABI for Swift to call into the Rust filesystem
//! implementation. All functions use C-compatible types and follow memory safety
//! conventions for FFI.

use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::Arc;

use agentfs_sdk::{AgentFS, AgentFSOptions, FileSystem, HostFS, OverlayFS};
use tokio::runtime::Runtime;
use turso::Value;

// ============================================================================
// Types
// ============================================================================

/// Opaque handle to a mounted filesystem instance.
///
/// This handle wraps the Rust FileSystem trait object and a Tokio runtime
/// for executing async operations.
pub struct AgentFSHandle {
    fs: Arc<dyn FileSystem>,
    runtime: Runtime,
}

/// File statistics returned to Swift.
///
/// Mirrors the Rust `Stats` struct with C-compatible types.
#[repr(C)]
pub struct FFIStats {
    pub ino: i64,
    pub mode: u32,
    pub nlink: u32,
    pub uid: u32,
    pub gid: u32,
    pub size: i64,
    pub atime: i64,
    pub mtime: i64,
    pub ctime: i64,
}

/// Filesystem statistics for statfs.
#[repr(C)]
pub struct FFIFilesystemStats {
    pub inodes: u64,
    pub bytes_used: u64,
}

/// Result type for FFI operations.
///
/// - `success`: true if the operation succeeded
/// - `error_code`: 0 on success, positive errno value on failure
#[repr(C)]
pub struct FFIResult {
    pub success: bool,
    pub error_code: i32,
}

impl FFIResult {
    fn ok() -> Self {
        FFIResult { success: true, error_code: 0 }
    }

    fn err(errno: i32) -> Self {
        FFIResult { success: false, error_code: errno }
    }

    fn not_found() -> Self {
        Self::err(libc::ENOENT)
    }

    fn io_error() -> Self {
        Self::err(libc::EIO)
    }

    fn invalid_arg() -> Self {
        Self::err(libc::EINVAL)
    }
}

/// Buffer for returning variable-length data.
///
/// The caller is responsible for freeing this buffer using `agentfs_free_buffer`.
#[repr(C)]
pub struct FFIBuffer {
    pub data: *mut u8,
    pub len: usize,
    pub capacity: usize,
}

impl FFIBuffer {
    fn null() -> Self {
        FFIBuffer {
            data: ptr::null_mut(),
            len: 0,
            capacity: 0,
        }
    }

    fn from_vec(v: Vec<u8>) -> Self {
        let mut v = v.into_boxed_slice();
        let len = v.len();
        let data = v.as_mut_ptr();
        std::mem::forget(v);
        FFIBuffer {
            data,
            len,
            capacity: len,
        }
    }
}

// ============================================================================
// Lifecycle Functions
// ============================================================================

/// Open an AgentFS database and return a handle.
///
/// # Arguments
/// * `db_path` - Path to the SQLite database file (null-terminated C string)
///
/// # Returns
/// * Non-null handle on success
/// * Null pointer on failure
///
/// # Safety
/// `db_path` must be a valid null-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn agentfs_open(db_path: *const c_char) -> *mut AgentFSHandle {
    if db_path.is_null() {
        return ptr::null_mut();
    }

    let path = match CStr::from_ptr(db_path).to_str() {
        Ok(s) => s,
        Err(_) => return ptr::null_mut(),
    };

    let runtime = match Runtime::new() {
        Ok(rt) => rt,
        Err(_) => return ptr::null_mut(),
    };

    let opts = match AgentFSOptions::resolve(path) {
        Ok(o) => o,
        Err(_) => return ptr::null_mut(),
    };

    let fs: Arc<dyn FileSystem> = match runtime.block_on(async {
        let agentfs = AgentFS::open(opts).await?;

        // Check for overlay configuration
        let conn = agentfs.get_connection();
        let query = "SELECT value FROM fs_overlay_config WHERE key = 'base_path'";
        let base_path: Option<String> = match conn.query(query, ()).await {
            Ok(mut rows) => {
                if let Ok(Some(row)) = rows.next().await {
                    row.get_value(0).ok().and_then(|v| {
                        if let Value::Text(s) = v {
                            Some(s.clone())
                        } else {
                            None
                        }
                    })
                } else {
                    None
                }
            }
            Err(_) => None,
        };

        if let Some(base_path) = base_path {
            let hostfs = HostFS::new(&base_path)?;
            let overlay = OverlayFS::new(Arc::new(hostfs), agentfs.fs);
            Ok::<Arc<dyn FileSystem>, anyhow::Error>(Arc::new(overlay))
        } else {
            Ok(Arc::new(agentfs.fs) as Arc<dyn FileSystem>)
        }
    }) {
        Ok(fs) => fs,
        Err(_) => return ptr::null_mut(),
    };

    Box::into_raw(Box::new(AgentFSHandle { fs, runtime }))
}

/// Close and free an AgentFS handle.
///
/// # Safety
/// `handle` must be a valid handle returned by `agentfs_open`, or null.
/// After calling this function, the handle must not be used again.
#[no_mangle]
pub unsafe extern "C" fn agentfs_close(handle: *mut AgentFSHandle) {
    if !handle.is_null() {
        let _ = Box::from_raw(handle);
    }
}

// ============================================================================
// File Metadata Operations
// ============================================================================

/// Get file statistics, following symlinks.
///
/// # Safety
/// - `handle` must be a valid handle
/// - `path` must be a valid null-terminated C string
/// - `out_stats` must be a valid pointer to write stats
#[no_mangle]
pub unsafe extern "C" fn agentfs_stat(
    handle: *const AgentFSHandle,
    path: *const c_char,
    out_stats: *mut FFIStats,
) -> FFIResult {
    if handle.is_null() || path.is_null() || out_stats.is_null() {
        return FFIResult::invalid_arg();
    }

    let handle = &*handle;
    let path = match CStr::from_ptr(path).to_str() {
        Ok(s) => s,
        Err(_) => return FFIResult::invalid_arg(),
    };

    match handle.runtime.block_on(handle.fs.stat(path)) {
        Ok(Some(stats)) => {
            *out_stats = FFIStats {
                ino: stats.ino,
                mode: stats.mode,
                nlink: stats.nlink,
                uid: stats.uid,
                gid: stats.gid,
                size: stats.size,
                atime: stats.atime,
                mtime: stats.mtime,
                ctime: stats.ctime,
            };
            FFIResult::ok()
        }
        Ok(None) => FFIResult::not_found(),
        Err(_) => FFIResult::io_error(),
    }
}

/// Get file statistics without following symlinks.
///
/// # Safety
/// Same as `agentfs_stat`.
#[no_mangle]
pub unsafe extern "C" fn agentfs_lstat(
    handle: *const AgentFSHandle,
    path: *const c_char,
    out_stats: *mut FFIStats,
) -> FFIResult {
    if handle.is_null() || path.is_null() || out_stats.is_null() {
        return FFIResult::invalid_arg();
    }

    let handle = &*handle;
    let path = match CStr::from_ptr(path).to_str() {
        Ok(s) => s,
        Err(_) => return FFIResult::invalid_arg(),
    };

    match handle.runtime.block_on(handle.fs.lstat(path)) {
        Ok(Some(stats)) => {
            *out_stats = FFIStats {
                ino: stats.ino,
                mode: stats.mode,
                nlink: stats.nlink,
                uid: stats.uid,
                gid: stats.gid,
                size: stats.size,
                atime: stats.atime,
                mtime: stats.mtime,
                ctime: stats.ctime,
            };
            FFIResult::ok()
        }
        Ok(None) => FFIResult::not_found(),
        Err(_) => FFIResult::io_error(),
    }
}

// ============================================================================
// File I/O Operations
// ============================================================================

/// Read data from a file at offset.
///
/// # Arguments
/// * `handle` - AgentFS handle
/// * `path` - File path
/// * `offset` - Byte offset to start reading
/// * `size` - Maximum bytes to read
/// * `out_buffer` - Output buffer (caller must free with `agentfs_free_buffer`)
///
/// # Safety
/// - All pointers must be valid
/// - `out_buffer` will be filled with allocated data that must be freed
#[no_mangle]
pub unsafe extern "C" fn agentfs_pread(
    handle: *const AgentFSHandle,
    path: *const c_char,
    offset: u64,
    size: u64,
    out_buffer: *mut FFIBuffer,
) -> FFIResult {
    if handle.is_null() || path.is_null() || out_buffer.is_null() {
        return FFIResult::invalid_arg();
    }

    let handle = &*handle;
    let path = match CStr::from_ptr(path).to_str() {
        Ok(s) => s,
        Err(_) => return FFIResult::invalid_arg(),
    };

    match handle.runtime.block_on(handle.fs.pread(path, offset, size)) {
        Ok(Some(data)) => {
            *out_buffer = FFIBuffer::from_vec(data);
            FFIResult::ok()
        }
        Ok(None) => {
            *out_buffer = FFIBuffer::null();
            FFIResult::not_found()
        }
        Err(_) => {
            *out_buffer = FFIBuffer::null();
            FFIResult::io_error()
        }
    }
}

/// Write data to a file at offset.
///
/// Creates the file if it doesn't exist. Extends the file if writing past end.
///
/// # Safety
/// - All pointers must be valid
/// - `data` must point to at least `data_len` bytes
#[no_mangle]
pub unsafe extern "C" fn agentfs_pwrite(
    handle: *const AgentFSHandle,
    path: *const c_char,
    offset: u64,
    data: *const u8,
    data_len: usize,
) -> FFIResult {
    if handle.is_null() || path.is_null() {
        return FFIResult::invalid_arg();
    }

    let handle = &*handle;
    let path = match CStr::from_ptr(path).to_str() {
        Ok(s) => s,
        Err(_) => return FFIResult::invalid_arg(),
    };

    let data_slice = if data.is_null() || data_len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(data, data_len)
    };

    match handle.runtime.block_on(handle.fs.pwrite(path, offset, data_slice)) {
        Ok(()) => FFIResult::ok(),
        Err(_) => FFIResult::io_error(),
    }
}

/// Read entire file contents.
///
/// # Safety
/// Same as `agentfs_pread`.
#[no_mangle]
pub unsafe extern "C" fn agentfs_read_file(
    handle: *const AgentFSHandle,
    path: *const c_char,
    out_buffer: *mut FFIBuffer,
) -> FFIResult {
    if handle.is_null() || path.is_null() || out_buffer.is_null() {
        return FFIResult::invalid_arg();
    }

    let handle = &*handle;
    let path = match CStr::from_ptr(path).to_str() {
        Ok(s) => s,
        Err(_) => return FFIResult::invalid_arg(),
    };

    match handle.runtime.block_on(handle.fs.read_file(path)) {
        Ok(Some(data)) => {
            *out_buffer = FFIBuffer::from_vec(data);
            FFIResult::ok()
        }
        Ok(None) => {
            *out_buffer = FFIBuffer::null();
            FFIResult::not_found()
        }
        Err(_) => {
            *out_buffer = FFIBuffer::null();
            FFIResult::io_error()
        }
    }
}

/// Write entire file contents (creates or overwrites).
///
/// # Safety
/// Same as `agentfs_pwrite`.
#[no_mangle]
pub unsafe extern "C" fn agentfs_write_file(
    handle: *const AgentFSHandle,
    path: *const c_char,
    data: *const u8,
    data_len: usize,
) -> FFIResult {
    if handle.is_null() || path.is_null() {
        return FFIResult::invalid_arg();
    }

    let handle = &*handle;
    let path = match CStr::from_ptr(path).to_str() {
        Ok(s) => s,
        Err(_) => return FFIResult::invalid_arg(),
    };

    let data_slice = if data.is_null() || data_len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(data, data_len)
    };

    match handle.runtime.block_on(handle.fs.write_file(path, data_slice)) {
        Ok(()) => FFIResult::ok(),
        Err(_) => FFIResult::io_error(),
    }
}

/// Truncate a file to a specific size.
///
/// # Safety
/// - `handle` must be valid
/// - `path` must be a valid null-terminated string
#[no_mangle]
pub unsafe extern "C" fn agentfs_truncate(
    handle: *const AgentFSHandle,
    path: *const c_char,
    size: u64,
) -> FFIResult {
    if handle.is_null() || path.is_null() {
        return FFIResult::invalid_arg();
    }

    let handle = &*handle;
    let path = match CStr::from_ptr(path).to_str() {
        Ok(s) => s,
        Err(_) => return FFIResult::invalid_arg(),
    };

    match handle.runtime.block_on(handle.fs.truncate(path, size)) {
        Ok(()) => FFIResult::ok(),
        Err(_) => FFIResult::io_error(),
    }
}

// ============================================================================
// Directory Operations
// ============================================================================

/// Read directory entries.
///
/// Returns entries as a JSON array string: `["file1", "file2", "dir1"]`
///
/// # Safety
/// - `out_entries` will be set to a newly allocated string (free with `agentfs_free_string`)
#[no_mangle]
pub unsafe extern "C" fn agentfs_readdir(
    handle: *const AgentFSHandle,
    path: *const c_char,
    out_entries: *mut *mut c_char,
) -> FFIResult {
    if handle.is_null() || path.is_null() || out_entries.is_null() {
        return FFIResult::invalid_arg();
    }

    let handle = &*handle;
    let path = match CStr::from_ptr(path).to_str() {
        Ok(s) => s,
        Err(_) => return FFIResult::invalid_arg(),
    };

    match handle.runtime.block_on(handle.fs.readdir(path)) {
        Ok(Some(entries)) => {
            // Format as JSON array
            let json = format!(
                "[{}]",
                entries
                    .iter()
                    .map(|s| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")))
                    .collect::<Vec<_>>()
                    .join(",")
            );

            match CString::new(json) {
                Ok(cstr) => {
                    *out_entries = cstr.into_raw();
                    FFIResult::ok()
                }
                Err(_) => {
                    *out_entries = ptr::null_mut();
                    FFIResult::io_error()
                }
            }
        }
        Ok(None) => {
            *out_entries = ptr::null_mut();
            FFIResult::not_found()
        }
        Err(_) => {
            *out_entries = ptr::null_mut();
            FFIResult::io_error()
        }
    }
}

/// Create a directory.
///
/// # Safety
/// Standard pointer validity requirements.
#[no_mangle]
pub unsafe extern "C" fn agentfs_mkdir(
    handle: *const AgentFSHandle,
    path: *const c_char,
) -> FFIResult {
    if handle.is_null() || path.is_null() {
        return FFIResult::invalid_arg();
    }

    let handle = &*handle;
    let path = match CStr::from_ptr(path).to_str() {
        Ok(s) => s,
        Err(_) => return FFIResult::invalid_arg(),
    };

    match handle.runtime.block_on(handle.fs.mkdir(path)) {
        Ok(()) => FFIResult::ok(),
        Err(e) => {
            // Check for specific error types
            if let Some(fs_err) = e.downcast_ref::<agentfs_sdk::FsError>() {
                FFIResult::err(fs_err.to_errno())
            } else {
                FFIResult::io_error()
            }
        }
    }
}

/// Remove a file or empty directory.
///
/// # Safety
/// Standard pointer validity requirements.
#[no_mangle]
pub unsafe extern "C" fn agentfs_remove(
    handle: *const AgentFSHandle,
    path: *const c_char,
) -> FFIResult {
    if handle.is_null() || path.is_null() {
        return FFIResult::invalid_arg();
    }

    let handle = &*handle;
    let path = match CStr::from_ptr(path).to_str() {
        Ok(s) => s,
        Err(_) => return FFIResult::invalid_arg(),
    };

    match handle.runtime.block_on(handle.fs.remove(path)) {
        Ok(()) => FFIResult::ok(),
        Err(e) => {
            if let Some(fs_err) = e.downcast_ref::<agentfs_sdk::FsError>() {
                FFIResult::err(fs_err.to_errno())
            } else {
                FFIResult::io_error()
            }
        }
    }
}

/// Rename/move a file or directory.
///
/// # Safety
/// Standard pointer validity requirements.
#[no_mangle]
pub unsafe extern "C" fn agentfs_rename(
    handle: *const AgentFSHandle,
    from: *const c_char,
    to: *const c_char,
) -> FFIResult {
    if handle.is_null() || from.is_null() || to.is_null() {
        return FFIResult::invalid_arg();
    }

    let handle = &*handle;
    let from_path = match CStr::from_ptr(from).to_str() {
        Ok(s) => s,
        Err(_) => return FFIResult::invalid_arg(),
    };
    let to_path = match CStr::from_ptr(to).to_str() {
        Ok(s) => s,
        Err(_) => return FFIResult::invalid_arg(),
    };

    match handle.runtime.block_on(handle.fs.rename(from_path, to_path)) {
        Ok(()) => FFIResult::ok(),
        Err(e) => {
            if let Some(fs_err) = e.downcast_ref::<agentfs_sdk::FsError>() {
                FFIResult::err(fs_err.to_errno())
            } else {
                FFIResult::io_error()
            }
        }
    }
}

// ============================================================================
// Symlink Operations
// ============================================================================

/// Create a symbolic link.
///
/// # Arguments
/// * `target` - What the symlink points to
/// * `linkpath` - Path where the symlink will be created
///
/// # Safety
/// Standard pointer validity requirements.
#[no_mangle]
pub unsafe extern "C" fn agentfs_symlink(
    handle: *const AgentFSHandle,
    target: *const c_char,
    linkpath: *const c_char,
) -> FFIResult {
    if handle.is_null() || target.is_null() || linkpath.is_null() {
        return FFIResult::invalid_arg();
    }

    let handle = &*handle;
    let target_str = match CStr::from_ptr(target).to_str() {
        Ok(s) => s,
        Err(_) => return FFIResult::invalid_arg(),
    };
    let linkpath_str = match CStr::from_ptr(linkpath).to_str() {
        Ok(s) => s,
        Err(_) => return FFIResult::invalid_arg(),
    };

    match handle.runtime.block_on(handle.fs.symlink(target_str, linkpath_str)) {
        Ok(()) => FFIResult::ok(),
        Err(e) => {
            if let Some(fs_err) = e.downcast_ref::<agentfs_sdk::FsError>() {
                FFIResult::err(fs_err.to_errno())
            } else {
                FFIResult::io_error()
            }
        }
    }
}

/// Read the target of a symbolic link.
///
/// # Safety
/// - `out_target` will be set to a newly allocated string (free with `agentfs_free_string`)
#[no_mangle]
pub unsafe extern "C" fn agentfs_readlink(
    handle: *const AgentFSHandle,
    path: *const c_char,
    out_target: *mut *mut c_char,
) -> FFIResult {
    if handle.is_null() || path.is_null() || out_target.is_null() {
        return FFIResult::invalid_arg();
    }

    let handle = &*handle;
    let path = match CStr::from_ptr(path).to_str() {
        Ok(s) => s,
        Err(_) => return FFIResult::invalid_arg(),
    };

    match handle.runtime.block_on(handle.fs.readlink(path)) {
        Ok(Some(target)) => match CString::new(target) {
            Ok(cstr) => {
                *out_target = cstr.into_raw();
                FFIResult::ok()
            }
            Err(_) => {
                *out_target = ptr::null_mut();
                FFIResult::io_error()
            }
        },
        Ok(None) => {
            *out_target = ptr::null_mut();
            FFIResult::not_found()
        }
        Err(_) => {
            *out_target = ptr::null_mut();
            FFIResult::io_error()
        }
    }
}

// ============================================================================
// Filesystem Operations
// ============================================================================

/// Get filesystem statistics.
///
/// # Safety
/// Standard pointer validity requirements.
#[no_mangle]
pub unsafe extern "C" fn agentfs_statfs(
    handle: *const AgentFSHandle,
    out_stats: *mut FFIFilesystemStats,
) -> FFIResult {
    if handle.is_null() || out_stats.is_null() {
        return FFIResult::invalid_arg();
    }

    let handle = &*handle;

    match handle.runtime.block_on(handle.fs.statfs()) {
        Ok(stats) => {
            *out_stats = FFIFilesystemStats {
                inodes: stats.inodes,
                bytes_used: stats.bytes_used,
            };
            FFIResult::ok()
        }
        Err(_) => FFIResult::io_error(),
    }
}

/// Synchronize file data to persistent storage.
///
/// # Safety
/// Standard pointer validity requirements.
#[no_mangle]
pub unsafe extern "C" fn agentfs_fsync(
    handle: *const AgentFSHandle,
    path: *const c_char,
) -> FFIResult {
    if handle.is_null() || path.is_null() {
        return FFIResult::invalid_arg();
    }

    let handle = &*handle;
    let path = match CStr::from_ptr(path).to_str() {
        Ok(s) => s,
        Err(_) => return FFIResult::invalid_arg(),
    };

    match handle.runtime.block_on(handle.fs.fsync(path)) {
        Ok(()) => FFIResult::ok(),
        Err(_) => FFIResult::io_error(),
    }
}

// ============================================================================
// Memory Management
// ============================================================================

/// Free a string allocated by Rust.
///
/// # Safety
/// `s` must be a string returned by an agentfs_* function, or null.
#[no_mangle]
pub unsafe extern "C" fn agentfs_free_string(s: *mut c_char) {
    if !s.is_null() {
        let _ = CString::from_raw(s);
    }
}

/// Free a buffer allocated by Rust.
///
/// # Safety
/// `buf` must be a buffer returned by an agentfs_* function.
#[no_mangle]
pub unsafe extern "C" fn agentfs_free_buffer(buf: FFIBuffer) {
    if !buf.data.is_null() && buf.capacity > 0 {
        let _ = Vec::from_raw_parts(buf.data, buf.len, buf.capacity);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ffi_result_values() {
        let ok = FFIResult::ok();
        assert!(ok.success);
        assert_eq!(ok.error_code, 0);

        let not_found = FFIResult::not_found();
        assert!(!not_found.success);
        assert_eq!(not_found.error_code, libc::ENOENT);
    }

    #[test]
    fn test_ffi_buffer_from_vec() {
        let data = vec![1u8, 2, 3, 4, 5];
        let buf = FFIBuffer::from_vec(data);
        assert!(!buf.data.is_null());
        assert_eq!(buf.len, 5);
        assert_eq!(buf.capacity, 5);

        // Free the buffer
        unsafe { agentfs_free_buffer(buf) };
    }

    #[test]
    fn test_null_handle_safety() {
        unsafe {
            // All functions should handle null handles gracefully
            let result = agentfs_stat(ptr::null(), c"test".as_ptr(), ptr::null_mut());
            assert!(!result.success);
            assert_eq!(result.error_code, libc::EINVAL);

            agentfs_close(ptr::null_mut()); // Should not crash
        }
    }
}
