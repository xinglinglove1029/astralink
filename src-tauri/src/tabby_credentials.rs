/// Exact keytar Windows target lookup. This module never enumerates credentials.
pub trait CredentialReader {
    fn read_utf8(&self, target: &str) -> Result<Option<String>, String>;
}
pub struct WindowsCredentialReader;
pub fn target(host: &str, port: Option<u16>, account: &str) -> Result<String, String> {
    if host.is_empty()
        || account.is_empty()
        || host.chars().chain(account.chars()).any(char::is_control)
        || host.len() > 253
        || account.len() > 256
        || port == Some(0)
    {
        return Err("Tabby 凭据目标无效".into());
    }
    Ok(match port {
        Some(port) => format!("ssh@{host}:{port}/{account}"),
        None => format!("ssh@{host}/{account}"),
    })
}
#[cfg(windows)]
impl CredentialReader for WindowsCredentialReader {
    fn read_utf8(&self, target: &str) -> Result<Option<String>, String> {
        use windows_sys::Win32::{
            Foundation::{GetLastError, ERROR_NOT_FOUND},
            Security::Credentials::{CredFree, CredReadW, CREDENTIALW, CRED_TYPE_GENERIC},
        };
        if target.contains('\0') || target.len() > 1024 {
            return Err("Tabby 凭据目标无效".into());
        }
        let target: Vec<u16> = target.encode_utf16().chain(Some(0)).collect();
        let mut pointer = std::ptr::null_mut::<CREDENTIALW>();
        // CredReadW returns one owned allocation for this exact target; UTF-16 is only the target name.
        if unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut pointer) } == 0 {
            return if unsafe { GetLastError() } == ERROR_NOT_FOUND {
                Ok(None)
            } else {
                Err("无法读取 Tabby 系统凭据".into())
            };
        }
        if pointer.is_null() {
            return Err("Tabby 系统凭据响应无效".into());
        }
        struct OwnedCredential(*mut CREDENTIALW);
        impl Drop for OwnedCredential {
            fn drop(&mut self) {
                unsafe { CredFree(self.0.cast()) }
            }
        }
        let owned = OwnedCredential(pointer);
        // keytar stores password bytes as UTF-8, not keyring's UTF-16 password convention.
        let credential = unsafe { &*owned.0 };
        let size = credential.CredentialBlobSize as usize;
        if size == 0 {
            return Ok(None);
        }
        if size > 16384 || credential.CredentialBlob.is_null() {
            return Err("Tabby 凭据数据无效".into());
        }
        let bytes = unsafe { std::slice::from_raw_parts(credential.CredentialBlob, size) };
        let password = std::str::from_utf8(bytes).map_err(|_| "Tabby 凭据不是 UTF-8")?;
        if password.contains('\0') {
            return Err("Tabby 凭据数据无效".into());
        }
        Ok(Some(password.into()))
    }
}
#[cfg(not(windows))]
impl CredentialReader for WindowsCredentialReader {
    fn read_utf8(&self, _: &str) -> Result<Option<String>, String> {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keytar_target_is_service_slash_account() {
        assert_eq!(
            target("db.test", Some(2222), "运维").unwrap(),
            "ssh@db.test:2222/运维"
        );
        assert_eq!(target("db.test", None, "ops").unwrap(), "ssh@db.test/ops");
        assert_eq!(target("::1", Some(22), "ops").unwrap(), "ssh@::1:22/ops");
        assert!(target("host", Some(0), "ops").is_err());
        assert!(target("host", Some(22), "bad\0account").is_err());
    }
}
