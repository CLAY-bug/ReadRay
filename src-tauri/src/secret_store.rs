const DEEPSEEK_KEY_TARGET: &str = "ReadRay/DeepSeekApiKey";
const DEEPSEEK_DISABLED_TARGET: &str = "ReadRay/DeepSeekApiKeyDisabled";
const DISABLED_MARKER: &[u8] = b"disabled-v1";

pub(crate) enum ApiKeyState {
    Credential(String),
    Environment(String),
    Missing,
}

impl ApiKeyState {
    pub(crate) fn configured(&self) -> bool {
        !matches!(self, Self::Missing)
    }

    pub(crate) fn source(&self) -> &'static str {
        match self {
            Self::Credential(_) => "credential",
            Self::Environment(_) => "environment",
            Self::Missing => "none",
        }
    }

    pub(crate) fn into_key(self) -> Option<String> {
        match self {
            Self::Credential(value) | Self::Environment(value) => Some(value),
            Self::Missing => None,
        }
    }
}

fn normalized_environment_key(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn resolve_api_key_state(
    stored_key: Option<String>,
    environment_disabled: bool,
    environment_key: Option<String>,
) -> ApiKeyState {
    if let Some(stored_key) = stored_key.filter(|value| !value.trim().is_empty()) {
        return ApiKeyState::Credential(stored_key);
    }
    if environment_disabled {
        return ApiKeyState::Missing;
    }
    normalized_environment_key(environment_key)
        .map(ApiKeyState::Environment)
        .unwrap_or(ApiKeyState::Missing)
}

pub(crate) fn deepseek_api_key_state() -> Result<ApiKeyState, String> {
    Ok(resolve_api_key_state(
        read_credential(DEEPSEEK_KEY_TARGET)?,
        read_credential(DEEPSEEK_DISABLED_TARGET)?.is_some(),
        std::env::var("DEEPSEEK_API_KEY").ok(),
    ))
}

pub(crate) fn save_deepseek_api_key(api_key: &str) -> Result<(), String> {
    write_credential(DEEPSEEK_KEY_TARGET, api_key.as_bytes())
}

pub(crate) fn clear_deepseek_api_key() -> Result<(), String> {
    // 先写入非敏感禁用标记，确保清除后不会重新回退到开发环境中的 Key。
    write_credential(DEEPSEEK_DISABLED_TARGET, DISABLED_MARKER)?;
    delete_credential(DEEPSEEK_KEY_TARGET)
}

#[cfg(target_os = "windows")]
fn read_credential(target: &str) -> Result<Option<String>, String> {
    use std::ffi::c_void;
    use std::ptr;
    use windows::core::{HRESULT, PCWSTR};
    use windows::Win32::Foundation::ERROR_NOT_FOUND;
    use windows::Win32::Security::Credentials::{
        CredFree, CredReadW, CREDENTIALW, CRED_TYPE_GENERIC,
    };

    struct CredentialBuffer(*mut CREDENTIALW);

    impl Drop for CredentialBuffer {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { CredFree(self.0.cast::<c_void>()) };
            }
        }
    }

    let target = target.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let mut raw = ptr::null_mut();
    if let Err(error) =
        unsafe { CredReadW(PCWSTR(target.as_ptr()), CRED_TYPE_GENERIC, None, &mut raw) }
    {
        if error.code() == HRESULT::from_win32(ERROR_NOT_FOUND.0) {
            return Ok(None);
        }
        return Err(format!("无法读取 Windows 凭据管理器：{error}"));
    }

    let buffer = CredentialBuffer(raw);
    let credential = unsafe {
        buffer
            .0
            .as_ref()
            .ok_or_else(|| "Windows 凭据管理器返回了空结果。".to_string())?
    };
    let size = usize::try_from(credential.CredentialBlobSize)
        .map_err(|_| "Windows 凭据长度无效。".to_string())?;
    if size == 0 {
        return Ok(None);
    }
    if credential.CredentialBlob.is_null() {
        return Err("Windows 凭据内容缺失。".to_string());
    }
    let bytes = unsafe { std::slice::from_raw_parts(credential.CredentialBlob, size) };
    String::from_utf8(bytes.to_vec())
        .map(Some)
        .map_err(|_| "Windows 凭据内容编码无效。".to_string())
}

#[cfg(target_os = "windows")]
fn write_credential(target: &str, value: &[u8]) -> Result<(), String> {
    use windows::core::PWSTR;
    use windows::Win32::Security::Credentials::{
        CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
    };

    let mut target = target.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let mut username = "ReadRay".encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let mut blob = value.to_vec();
    let blob_size = u32::try_from(blob.len())
        .map_err(|_| "API Key 长度超过 Windows 凭据管理器限制。".to_string())?;
    let credential = CREDENTIALW {
        Type: CRED_TYPE_GENERIC,
        TargetName: PWSTR(target.as_mut_ptr()),
        CredentialBlobSize: blob_size,
        CredentialBlob: blob.as_mut_ptr(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        UserName: PWSTR(username.as_mut_ptr()),
        ..Default::default()
    };

    let result = unsafe { CredWriteW(&credential, 0) }
        .map_err(|error| format!("无法写入 Windows 凭据管理器：{error}"));
    blob.fill(0);
    result
}

#[cfg(target_os = "windows")]
fn delete_credential(target: &str) -> Result<(), String> {
    use windows::core::{HRESULT, PCWSTR};
    use windows::Win32::Foundation::ERROR_NOT_FOUND;
    use windows::Win32::Security::Credentials::{CredDeleteW, CRED_TYPE_GENERIC};

    let target = target.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    match unsafe { CredDeleteW(PCWSTR(target.as_ptr()), CRED_TYPE_GENERIC, None) } {
        Ok(()) => Ok(()),
        Err(error) if error.code() == HRESULT::from_win32(ERROR_NOT_FOUND.0) => Ok(()),
        Err(error) => Err(format!("无法从 Windows 凭据管理器清除 API Key：{error}")),
    }
}

#[cfg(not(target_os = "windows"))]
fn read_credential(_target: &str) -> Result<Option<String>, String> {
    Ok(None)
}

#[cfg(not(target_os = "windows"))]
fn write_credential(_target: &str, _value: &[u8]) -> Result<(), String> {
    Err("ReadRay 当前仅支持在 Windows 上安全保存 API Key。".to_string())
}

#[cfg(not(target_os = "windows"))]
fn delete_credential(_target: &str) -> Result<(), String> {
    Err("ReadRay 当前仅支持在 Windows 上清除已保存的 API Key。".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_key_has_priority_over_environment_and_disabled_marker() {
        let state = resolve_api_key_state(
            Some("stored-secret".to_string()),
            true,
            Some("environment-secret".to_string()),
        );
        assert_eq!(state.source(), "credential");
        assert_eq!(state.into_key().as_deref(), Some("stored-secret"));
    }

    #[test]
    fn disabled_marker_suppresses_environment_fallback_after_clear() {
        let state = resolve_api_key_state(None, true, Some("environment-secret".to_string()));
        assert!(!state.configured());
        assert_eq!(state.source(), "none");
    }

    #[test]
    fn environment_key_is_only_used_when_no_persisted_decision_exists() {
        let state = resolve_api_key_state(None, false, Some("  environment-secret  ".to_string()));
        assert_eq!(state.source(), "environment");
        assert_eq!(state.into_key().as_deref(), Some("environment-secret"));
    }
}
