//! Windows: the `TERMSRV/<host>` credential `mstsc` signs in with, written with `CredWriteW` so the
//! password never appears on a command line (the Swift-era plan used `cmdkey /pass:`), and deleted with
//! `CredDeleteW` once the connection has started.
//!
//! These are Win32 C calls, hence the `unsafe` blocks; each one says why it is sound.
#![allow(unsafe_code)]

use std::io;

use windows_sys::Win32::Security::Credentials::{
    CRED_PERSIST_SESSION, CRED_TYPE_DOMAIN_PASSWORD, CREDENTIALW, CredDeleteW, CredWriteW,
};
use zeroize::Zeroizing;

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Stores `user` / `password` for `target` as a domain password credential for this logon session.
///
/// # Errors
/// The Win32 error when the credential can't be written.
pub fn write(target: &str, user: &str, password: &str) -> io::Result<()> {
    let mut target = wide(target);
    let mut user = wide(user);
    // The blob is the password in UTF-16LE without the terminator.
    let mut blob: Zeroizing<Vec<u8>> =
        Zeroizing::new(password.encode_utf16().flat_map(u16::to_le_bytes).collect());
    let size = u32::try_from(blob.len()).map_err(|_| io::Error::other("password too long"))?;
    let credential = CREDENTIALW {
        Type: CRED_TYPE_DOMAIN_PASSWORD,
        TargetName: target.as_mut_ptr(),
        CredentialBlobSize: size,
        CredentialBlob: blob.as_mut_ptr(),
        Persist: CRED_PERSIST_SESSION,
        UserName: user.as_mut_ptr(),
        ..CREDENTIALW::default()
    };
    // SAFETY: every pointer in `credential` points into a live, NUL-terminated (strings) or
    // `CredentialBlobSize`-long (blob) buffer owned by this frame; the call copies them.
    if unsafe { CredWriteW(&raw const credential, 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Deletes the domain password credential for `target`.
///
/// # Errors
/// The Win32 error, e.g. when it doesn't exist.
pub fn delete(target: &str) -> io::Result<()> {
    let target = wide(target);
    // SAFETY: `target` is a live NUL-terminated UTF-16 string.
    if unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_DOMAIN_PASSWORD, 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_strings_are_terminated() {
        assert_eq!(wide("ab"), [97, 98, 0]);
    }

    /// Writes and deletes a real credential; run by hand on Windows with `-- --ignored`.
    #[test]
    #[ignore = "writes to the Windows credential store"]
    fn writes_and_deletes() {
        let target = "TERMSRV/rd-platform-test.example";
        write(target, "EXAMPLE\\user", "secret").unwrap();
        delete(target).unwrap();
        assert!(delete(target).is_err());
    }
}
