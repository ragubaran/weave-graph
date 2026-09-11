use std::path::{Path, PathBuf};

/// `statfs`-based network-filesystem detection (`plan.md` §1.4): WAL mode
/// needs POSIX shared memory, which NFS/SMB/AFP mounts don't reliably
/// provide. Fails open (returns `false`) on any lookup failure or on a
/// platform without a `statfs`-family syscall — this exists to catch the
/// common, well-known network cases, not to be an exhaustive classifier.
pub fn is_network_filesystem(path: &Path) -> bool {
    match nearest_existing_ancestor(path) {
        Some(existing) => raw_is_network_filesystem(&existing),
        None => false,
    }
}

fn nearest_existing_ancestor(path: &Path) -> Option<PathBuf> {
    let mut current = path;
    loop {
        if current.exists() {
            return Some(current.to_path_buf());
        }
        current = current.parent()?;
    }
}

#[cfg(target_os = "macos")]
const NETWORK_FS_NAMES: &[&str] = &["nfs", "smbfs", "afpfs", "webdav", "cifs"];

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn raw_is_network_filesystem(path: &Path) -> bool {
    use std::ffi::CString;
    use std::mem::MaybeUninit;

    let Some(path_str) = path.to_str() else {
        return false;
    };
    let Ok(c_path) = CString::new(path_str) else {
        return false;
    };
    let mut stat = MaybeUninit::<libc::statfs>::uninit();
    // Safety: `c_path` is a valid NUL-terminated C string that outlives this
    // call, and `stat` is a correctly-sized out-pointer per `statfs(2)`. A
    // non-zero return means the call failed, so `stat` is never read then.
    let ret = unsafe { libc::statfs(c_path.as_ptr(), stat.as_mut_ptr()) };
    if ret != 0 {
        return false;
    }
    // Safety: `ret == 0` means the kernel fully populated `stat`.
    let stat = unsafe { stat.assume_init() };
    let name_bytes: Vec<u8> = stat
        .f_fstypename
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c as u8)
        .collect();
    let name = String::from_utf8_lossy(&name_bytes).to_lowercase();
    NETWORK_FS_NAMES.contains(&name.as_str())
}

#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
fn raw_is_network_filesystem(path: &Path) -> bool {
    use std::ffi::CString;
    use std::mem::MaybeUninit;

    // Magic numbers from linux/magic.h: NFS, SMB1, and both CIFS/SMB2 client
    // filesystem ids. Best-effort — not verified on this session's macOS host.
    const NFS_SUPER_MAGIC: i64 = 0x6969;
    const SMB_SUPER_MAGIC: i64 = 0x517B;
    const CIFS_MAGIC_NUMBER: i64 = 0xFF53_4D42u32 as i32 as i64;
    const SMB2_MAGIC_NUMBER: i64 = 0xFE53_4D42u32 as i32 as i64;

    let Some(path_str) = path.to_str() else {
        return false;
    };
    let Ok(c_path) = CString::new(path_str) else {
        return false;
    };
    let mut stat = MaybeUninit::<libc::statfs>::uninit();
    // Safety: same contract as the macOS variant above.
    let ret = unsafe { libc::statfs(c_path.as_ptr(), stat.as_mut_ptr()) };
    if ret != 0 {
        return false;
    }
    // Safety: `ret == 0` means the kernel fully populated `stat`.
    let stat = unsafe { stat.assume_init() };
    // `f_type`'s type varies by libc: signed `i64` on glibc x86_64/aarch64,
    // unsigned `c_ulong` (u64) on musl — `as i64` is required on musl and a
    // same-type no-op on glibc, so clippy's lint is a false positive here.
    #[allow(clippy::unnecessary_cast)]
    let f_type = stat.f_type as i64;
    matches!(
        f_type,
        NFS_SUPER_MAGIC | SMB_SUPER_MAGIC | CIFS_MAGIC_NUMBER | SMB2_MAGIC_NUMBER
    )
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn raw_is_network_filesystem(_path: &Path) -> bool {
    false
}

#[cfg(test)]
mod tests;
