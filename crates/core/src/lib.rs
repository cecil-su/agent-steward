use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Open,
    InProgress,
    Blocked,
    Closed,
}

impl TaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::InProgress => "in_progress",
            Self::Blocked => "blocked",
            Self::Closed => "closed",
        }
    }
}

impl TryFrom<&str> for TaskStatus {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "open" => Ok(Self::Open),
            "in_progress" => Ok(Self::InProgress),
            "blocked" => Ok(Self::Blocked),
            "closed" => Ok(Self::Closed),
            _ => Err(format!("unknown task status: {value}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TaskView {
    pub id: i64,
    pub task_key: Option<String>,
    pub title: Option<String>,
    pub status: TaskStatus,
    pub version: i64,
    pub goal: Option<String>,
    pub scope: Option<String>,
    pub acceptance_criteria: Option<String>,
    pub next_step: Option<String>,
    pub block_reason: Option<String>,
    pub block_recovery: Option<String>,
    pub current_session_id: Option<String>,
    pub repository_path: Option<String>,
    pub repository_common_dir: Option<String>,
    pub repository_branch: Option<String>,
    pub worktree_path: Option<String>,
    pub latest_checkpoint_id: Option<String>,
    pub closure_outcome: Option<String>,
    pub closure_reason: Option<String>,
    pub closed_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionView {
    pub id: String,
    pub task_id: i64,
    pub source: Option<String>,
    pub external_session_id: Option<String>,
    pub continued_from: Option<String>,
    pub record_path: Option<String>,
    pub started_at: String,
    pub ended_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CheckpointView {
    pub id: String,
    pub task_id: i64,
    pub session_id: String,
    pub summary: String,
    pub completed: Vec<String>,
    pub decisions: Vec<String>,
    pub pending: Vec<String>,
    pub next_step: String,
    pub risks: Vec<String>,
    pub git_head: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TaskNoteView {
    pub id: i64,
    pub task_id: i64,
    pub session_id: Option<String>,
    pub note_type: String,
    pub text: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionImportView {
    pub id: String,
    pub session_id: String,
    pub source_path: String,
    pub media_type: Option<String>,
    pub sha256: String,
    pub size_bytes: i64,
    pub imported_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeStatus {
    pub registered: bool,
    pub repository_path: Option<String>,
    pub repository_common_dir: Option<String>,
    pub path: Option<String>,
    pub exists: bool,
    pub branch: Option<String>,
    pub head: Option<String>,
    pub staged: Option<Vec<String>>,
    pub unstaged: Option<Vec<String>>,
    pub untracked: Option<Vec<String>>,
    pub ignored: Option<Vec<String>>,
    pub observed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: i64,
    pub task_id: i64,
    pub sequence: i64,
    pub change_type: String,
    pub session_id: Option<String>,
    pub occurred_at: String,
    pub summary: String,
    pub payload: Option<Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskCreateInput {
    #[serde(default)]
    pub task_key: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub goal: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub acceptance_criteria: Option<String>,
    #[serde(default)]
    pub next_step: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CheckpointInput {
    pub summary: String,
    pub completed: Vec<String>,
    pub decisions: Vec<String>,
    pub pending: Vec<String>,
    pub next_step: String,
    pub risks: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Warning {
    pub code: String,
    pub message: String,
    pub details: Value,
}

pub fn require_non_empty(field: &str, value: &str) -> Result<String, (String, String)> {
    let value = value.trim();
    if value.is_empty() {
        Err((field.to_owned(), "must be a non-empty string".to_owned()))
    } else {
        Ok(value.to_owned())
    }
}

pub fn validate_string_array(field: &str, values: &[String]) -> Result<(), (String, String)> {
    if values.iter().any(|value| value.trim().is_empty()) {
        Err((
            field.to_owned(),
            "items must be non-empty strings".to_owned(),
        ))
    } else {
        Ok(())
    }
}

pub fn default_data_dir() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .map(|home| home.join("Library/Application Support/agent-steward"))
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("LOCALAPPDATA")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .map(|root| root.join("agent-steward"))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        if let Some(root) = std::env::var_os("XDG_DATA_HOME").filter(|value| !value.is_empty()) {
            return Some(PathBuf::from(root).join("agent-steward"));
        }
        std::env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .map(|home| home.join(".local/share/agent-steward"))
    }
}

pub fn set_private_dir(path: &Path) -> Result<(), std::io::Error> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
    }
    #[cfg(windows)]
    {
        windows_permissions::set_private_acl(path, true)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        Ok(())
    }
}

pub fn set_private_file(path: &Path) -> Result<(), std::io::Error> {
    if !path.try_exists()? {
        return Ok(());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
    }
    #[cfg(windows)]
    {
        windows_permissions::set_private_acl(path, false)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        Ok(())
    }
}

#[cfg(windows)]
pub fn private_acl_is_protected(path: &Path) -> Result<bool, std::io::Error> {
    windows_permissions::private_acl_is_protected(path)
}

#[cfg(windows)]
mod windows_permissions {
    use std::collections::BTreeSet;
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use std::{io, mem, ptr, slice};

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE, LocalFree};
    use windows_sys::Win32::Security::Authorization::{
        ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
        GetNamedSecurityInfoW, SDDL_REVISION_1, SE_FILE_OBJECT, SetNamedSecurityInfoW,
    };
    use windows_sys::Win32::Security::{
        ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, ACL_REVISION, ACL_SIZE_INFORMATION,
        AclSizeInformation, AddAce, DACL_SECURITY_INFORMATION, GetAce, GetAclInformation,
        GetSecurityDescriptorControl, GetSecurityDescriptorDacl, GetTokenInformation,
        InitializeAcl, IsWellKnownSid, LookupAccountSidW, PROTECTED_DACL_SECURITY_INFORMATION,
        PSECURITY_DESCRIPTOR, SE_DACL_PROTECTED, SID_AND_ATTRIBUTES, SID_NAME_USE,
        TOKEN_APPCONTAINER_INFORMATION, TOKEN_GROUPS, TOKEN_INFORMATION_CLASS, TOKEN_QUERY,
        TOKEN_USER, TokenAppContainerSid, TokenRestrictedSids, TokenUser,
        WinBuiltinDeviceOwnersSid,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_ADD_FILE, FILE_FLAG_BACKUP_SEMANTICS, FILE_GENERIC_READ,
        FILE_GENERIC_WRITE, FILE_LIST_DIRECTORY, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::SystemServices::{
        ACCESS_ALLOWED_ACE_TYPE, ACCESS_DENIED_ACE_TYPE, ACCESS_DENIED_CALLBACK_ACE_TYPE,
        ACCESS_DENIED_CALLBACK_OBJECT_ACE_TYPE, ACCESS_DENIED_OBJECT_ACE_TYPE,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    struct OwnedHandle(HANDLE);

    impl Drop for OwnedHandle {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }

    struct LocalAllocation(*mut c_void);

    impl Drop for LocalAllocation {
        fn drop(&mut self) {
            unsafe {
                LocalFree(self.0);
            }
        }
    }

    pub(super) fn set_private_acl(path: &Path, directory: bool) -> io::Result<()> {
        let context = current_token_security_context()?;
        let trusted_restricted_sids =
            trusted_restricted_sids(path, &context.restricted_isolation_sids)?;
        let preserved_aces = current_restricted_aces(path, &trusted_restricted_sids)?
            .into_iter()
            .map(|(_, ace)| ace)
            .collect::<Vec<_>>();
        let flags = if directory { "OICI" } else { "" };
        let mut sddl = format!(
            "D:P(A;{flags};FA;;;{})(A;{flags};FA;;;SY)(A;{flags};FA;;;BA)",
            context.user
        );
        // The package SID is authoritative token metadata. For a non-AppContainer
        // restricted token, retain an object's matching ACE only when its parent
        // ACL independently identifies that SID as host-granted; never infer one
        // from token cardinality or a failed account-name lookup.
        if let Some(sid) = &context.app_container_sid {
            sddl.push_str(&format!("(A;{flags};FA;;;{sid})"));
        }
        apply_dacl_sddl(path, &sddl, &preserved_aces)?;
        verify_current_process_access(path, directory)
    }

    fn apply_dacl_sddl(path: &Path, sddl: &str, additional_aces: &[Vec<u8>]) -> io::Result<()> {
        let sddl_wide = sddl.encode_utf16().chain([0]).collect::<Vec<_>>();
        let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl_wide.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let _descriptor = LocalAllocation(descriptor);
        let path_wide = path
            .as_os_str()
            .encode_wide()
            .chain([0])
            .collect::<Vec<_>>();
        let mut dacl: *mut ACL = ptr::null_mut();
        let mut dacl_present = 0;
        let mut dacl_defaulted = 0;
        if unsafe {
            GetSecurityDescriptorDacl(
                descriptor,
                &mut dacl_present,
                &mut dacl,
                &mut dacl_defaulted,
            )
        } == 0
            || dacl_present == 0
            || dacl.is_null()
        {
            return Err(io::Error::last_os_error());
        }
        let mut merged_dacl = if additional_aces.is_empty() {
            None
        } else {
            Some(merge_dacl(dacl, additional_aces)?)
        };
        let dacl = merged_dacl
            .as_mut()
            .map_or(dacl, |buffer| buffer.as_mut_ptr().cast::<ACL>());
        let error = unsafe {
            SetNamedSecurityInfoW(
                path_wide.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                dacl,
                ptr::null_mut(),
            )
        };
        if error != 0 {
            return Err(io::Error::from_raw_os_error(error as i32));
        }
        Ok(())
    }

    fn trusted_restricted_sids(
        path: &Path,
        restricted_sids: &BTreeSet<String>,
    ) -> io::Result<BTreeSet<String>> {
        let Some(parent) = acl_parent(path) else {
            return Ok(BTreeSet::new());
        };
        Ok(current_restricted_aces(parent, restricted_sids)?
            .into_iter()
            .map(|(sid, _)| sid)
            .collect())
    }

    fn acl_parent(path: &Path) -> Option<&Path> {
        path.parent().map(|parent| {
            if parent.as_os_str().is_empty() {
                Path::new(".")
            } else {
                parent
            }
        })
    }

    fn current_restricted_aces(
        path: &Path,
        restricted_sids: &BTreeSet<String>,
    ) -> io::Result<Vec<(String, Vec<u8>)>> {
        if restricted_sids.is_empty() {
            return Ok(Vec::new());
        }
        let path_wide = path
            .as_os_str()
            .encode_wide()
            .chain([0])
            .collect::<Vec<_>>();
        let mut dacl: *mut ACL = ptr::null_mut();
        let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
        let error = unsafe {
            GetNamedSecurityInfoW(
                path_wide.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                &mut dacl,
                ptr::null_mut(),
                &mut descriptor,
            )
        };
        if error != 0 {
            return Err(io::Error::from_raw_os_error(error as i32));
        }
        let _descriptor = LocalAllocation(descriptor);
        if dacl.is_null() {
            return Ok(Vec::new());
        }
        let mut information = ACL_SIZE_INFORMATION::default();
        if unsafe {
            GetAclInformation(
                dacl,
                ptr::addr_of_mut!(information).cast(),
                mem::size_of::<ACL_SIZE_INFORMATION>() as u32,
                AclSizeInformation,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let mut result = Vec::new();
        for index in 0..information.AceCount {
            let mut ace: *mut c_void = ptr::null_mut();
            if unsafe { GetAce(dacl, index, &mut ace) } == 0 {
                return Err(io::Error::last_os_error());
            }
            let header = unsafe { &*ace.cast::<ACE_HEADER>() };
            if header.AceType as u32 != ACCESS_ALLOWED_ACE_TYPE {
                continue;
            }
            let ace_size = usize::from(header.AceSize);
            if ace_size < mem::size_of::<ACCESS_ALLOWED_ACE>() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid access-allowed ACE",
                ));
            }
            let allowed_ace = ace.cast::<ACCESS_ALLOWED_ACE>();
            let sid_pointer = unsafe { ptr::addr_of!((*allowed_ace).SidStart) }
                .cast_mut()
                .cast::<c_void>();
            let sid = sid_to_string(sid_pointer)?;
            if restricted_sids.contains(&sid) {
                result.push((
                    sid,
                    unsafe { slice::from_raw_parts(ace.cast::<u8>(), ace_size) }.to_vec(),
                ));
            }
        }
        Ok(result)
    }

    fn merge_dacl(base: *const ACL, additional_aces: &[Vec<u8>]) -> io::Result<Vec<usize>> {
        let mut information = ACL_SIZE_INFORMATION::default();
        if unsafe {
            GetAclInformation(
                base,
                ptr::addr_of_mut!(information).cast(),
                mem::size_of::<ACL_SIZE_INFORMATION>() as u32,
                AclSizeInformation,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let additional_size = additional_aces.iter().try_fold(0_usize, |size, ace| {
            size.checked_add(ace.len()).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "private ACL is too large")
            })
        })?;
        let acl_size = (information.AclBytesInUse as usize)
            .checked_add(additional_size)
            .filter(|size| *size <= u16::MAX as usize)
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "private ACL is too large")
            })?;
        let word_size = mem::size_of::<usize>();
        let mut buffer = vec![0_usize; acl_size.div_ceil(word_size)];
        let merged = buffer.as_mut_ptr().cast::<ACL>();
        if unsafe { InitializeAcl(merged, acl_size as u32, ACL_REVISION) } == 0 {
            return Err(io::Error::last_os_error());
        }
        for index in 0..information.AceCount {
            let mut ace: *mut c_void = ptr::null_mut();
            if unsafe { GetAce(base, index, &mut ace) } == 0 {
                return Err(io::Error::last_os_error());
            }
            let ace_size = unsafe { (*ace.cast::<ACE_HEADER>()).AceSize } as u32;
            if unsafe { AddAce(merged, ACL_REVISION, u32::MAX, ace, ace_size) } == 0 {
                return Err(io::Error::last_os_error());
            }
        }
        for ace in additional_aces {
            if unsafe {
                AddAce(
                    merged,
                    ACL_REVISION,
                    u32::MAX,
                    ace.as_ptr().cast(),
                    ace.len() as u32,
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(buffer)
    }

    fn verify_current_process_access(path: &Path, directory: bool) -> io::Result<()> {
        let path_wide = path
            .as_os_str()
            .encode_wide()
            .chain([0])
            .collect::<Vec<_>>();
        let desired_access = if directory {
            FILE_LIST_DIRECTORY | FILE_ADD_FILE
        } else {
            FILE_GENERIC_READ | FILE_GENERIC_WRITE
        };
        let flags = if directory {
            FILE_FLAG_BACKUP_SEMANTICS
        } else {
            0
        };
        let handle = unsafe {
            CreateFileW(
                path_wide.as_ptr(),
                desired_access,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                ptr::null(),
                OPEN_EXISTING,
                flags,
                ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        unsafe {
            CloseHandle(handle);
        }
        Ok(())
    }

    pub(super) fn private_acl_is_protected(path: &Path) -> io::Result<bool> {
        let path_wide = path
            .as_os_str()
            .encode_wide()
            .chain([0])
            .collect::<Vec<_>>();
        let mut dacl: *mut ACL = ptr::null_mut();
        let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
        let error = unsafe {
            GetNamedSecurityInfoW(
                path_wide.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                &mut dacl,
                ptr::null_mut(),
                &mut descriptor,
            )
        };
        if error != 0 {
            return Err(io::Error::from_raw_os_error(error as i32));
        }
        let _descriptor = LocalAllocation(descriptor);
        let mut control = 0_u16;
        let mut revision = 0_u32;
        if unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) } == 0 {
            return Err(io::Error::last_os_error());
        }
        if dacl.is_null() || control & SE_DACL_PROTECTED == 0 {
            return Ok(false);
        }

        let context = current_token_security_context()?;
        let trusted_restricted_sids =
            trusted_restricted_sids(path, &context.restricted_isolation_sids)?;
        dacl_allows_only_approved_principals(
            dacl,
            &context.user,
            context.app_container_sid.as_deref(),
            &trusted_restricted_sids,
        )
    }

    fn dacl_allows_only_approved_principals(
        dacl: *const ACL,
        current_user: &str,
        app_container_sid: Option<&str>,
        restricted_isolation_sids: &BTreeSet<String>,
    ) -> io::Result<bool> {
        let mut information = ACL_SIZE_INFORMATION::default();
        if unsafe {
            GetAclInformation(
                dacl,
                ptr::addr_of_mut!(information).cast(),
                mem::size_of::<ACL_SIZE_INFORMATION>() as u32,
                AclSizeInformation,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        for index in 0..information.AceCount {
            let mut ace: *mut c_void = ptr::null_mut();
            if unsafe { GetAce(dacl, index, &mut ace) } == 0 {
                return Err(io::Error::last_os_error());
            }
            let header = unsafe { &*ace.cast::<ACE_HEADER>() };
            match header.AceType as u32 {
                ACCESS_ALLOWED_ACE_TYPE => {
                    if usize::from(header.AceSize) < mem::size_of::<ACCESS_ALLOWED_ACE>() {
                        return Ok(false);
                    }
                    let allowed_ace = ace.cast::<ACCESS_ALLOWED_ACE>();
                    let sid_pointer = unsafe { ptr::addr_of!((*allowed_ace).SidStart) }
                        .cast_mut()
                        .cast::<c_void>();
                    let sid = sid_to_string(sid_pointer)?;
                    if sid != current_user
                        && sid != "S-1-5-18"
                        && sid != "S-1-5-32-544"
                        && app_container_sid != Some(sid.as_str())
                        && !restricted_isolation_sids.contains(&sid)
                    {
                        return Ok(false);
                    }
                }
                ACCESS_DENIED_ACE_TYPE
                | ACCESS_DENIED_OBJECT_ACE_TYPE
                | ACCESS_DENIED_CALLBACK_ACE_TYPE
                | ACCESS_DENIED_CALLBACK_OBJECT_ACE_TYPE => {}
                _ => return Ok(false),
            }
        }
        Ok(true)
    }

    #[cfg(test)]
    pub(super) fn test_sddl_is_private(sddl: &str) -> io::Result<bool> {
        let sddl_wide = sddl.encode_utf16().chain([0]).collect::<Vec<_>>();
        let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl_wide.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let _descriptor = LocalAllocation(descriptor);
        let mut dacl: *mut ACL = ptr::null_mut();
        let mut dacl_present = 0;
        let mut dacl_defaulted = 0;
        if unsafe {
            GetSecurityDescriptorDacl(
                descriptor,
                &mut dacl_present,
                &mut dacl,
                &mut dacl_defaulted,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let mut control = 0_u16;
        let mut revision = 0_u32;
        if unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) } == 0 {
            return Err(io::Error::last_os_error());
        }
        if dacl_present == 0 || dacl.is_null() || control & SE_DACL_PROTECTED == 0 {
            return Ok(false);
        }
        let context = current_token_security_context()?;
        dacl_allows_only_approved_principals(
            dacl,
            &context.user,
            context.app_container_sid.as_deref(),
            &BTreeSet::new(),
        )
    }

    struct TokenSecurityContext {
        user: String,
        app_container_sid: Option<String>,
        restricted_isolation_sids: BTreeSet<String>,
    }

    fn current_token_security_context() -> io::Result<TokenSecurityContext> {
        let token = current_process_token()?;
        let user = token_user_sid(token.0)?;
        let mut restricted_isolation_sids = token_restricted_isolation_sids(token.0)?
            .into_iter()
            .collect::<BTreeSet<_>>();
        restricted_isolation_sids.remove(&user);
        restricted_isolation_sids.remove("S-1-5-18");
        restricted_isolation_sids.remove("S-1-5-32-544");

        let app_container = token_information(token.0, TokenAppContainerSid)?;
        let mut app_container_sid = None;
        if !app_container.is_empty() {
            if app_container.len() * mem::size_of::<usize>()
                < mem::size_of::<TOKEN_APPCONTAINER_INFORMATION>()
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid app-container token information",
                ));
            }
            let information = unsafe {
                &*app_container
                    .as_ptr()
                    .cast::<TOKEN_APPCONTAINER_INFORMATION>()
            };
            if !information.TokenAppContainer.is_null() {
                app_container_sid = Some(sid_to_string(information.TokenAppContainer)?);
            }
        }
        Ok(TokenSecurityContext {
            user,
            app_container_sid,
            restricted_isolation_sids,
        })
    }

    fn current_process_token() -> io::Result<OwnedHandle> {
        let mut raw_token: HANDLE = ptr::null_mut();
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw_token) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(OwnedHandle(raw_token))
    }

    fn token_information(token: HANDLE, class: TOKEN_INFORMATION_CLASS) -> io::Result<Vec<usize>> {
        const ERROR_INVALID_PARAMETER: i32 = 87;

        let mut required = 0_u32;
        let first = unsafe { GetTokenInformation(token, class, ptr::null_mut(), 0, &mut required) };
        if first == 0 && required == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER) {
                return Ok(Vec::new());
            }
            return Err(error);
        }
        if required == 0 {
            return Ok(Vec::new());
        }
        let word_size = mem::size_of::<usize>();
        let mut buffer = vec![0_usize; (required as usize).div_ceil(word_size)];
        if unsafe {
            GetTokenInformation(
                token,
                class,
                buffer.as_mut_ptr().cast(),
                required,
                &mut required,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(buffer)
    }

    fn token_user_sid(token: HANDLE) -> io::Result<String> {
        let buffer = token_information(token, TokenUser)?;
        if buffer.len() * mem::size_of::<usize>() < mem::size_of::<TOKEN_USER>() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid user token information",
            ));
        }
        let token_user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
        sid_to_string(token_user.User.Sid)
    }

    fn token_restricted_isolation_sids(token: HANDLE) -> io::Result<Vec<String>> {
        let buffer = token_information(token, TokenRestrictedSids)?;
        if buffer.is_empty() {
            return Ok(Vec::new());
        }
        let byte_len = buffer.len() * mem::size_of::<usize>();
        if byte_len < mem::size_of::<u32>() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid group token information",
            ));
        }
        let count = unsafe { *buffer.as_ptr().cast::<u32>() } as usize;
        if count == 0 {
            return Ok(Vec::new());
        }
        let groups_offset = mem::offset_of!(TOKEN_GROUPS, Groups);
        let required = groups_offset
            .checked_add(count.saturating_mul(mem::size_of::<SID_AND_ATTRIBUTES>()))
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "invalid group token count")
            })?;
        if required > byte_len {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "truncated group token information",
            ));
        }
        let groups = unsafe {
            slice::from_raw_parts(
                buffer
                    .as_ptr()
                    .cast::<u8>()
                    .add(groups_offset)
                    .cast::<SID_AND_ATTRIBUTES>(),
                count,
            )
        };
        let mut isolation_sids = Vec::new();
        for entry in groups {
            // Name lookup is rejection-only: an unmapped result does not make a
            // SID authoritative. Authority comes from matching object and parent
            // ACEs (or from TokenAppContainerSid).
            if is_well_known_sid(entry.Sid) || sid_is_mapped(entry.Sid)? {
                continue;
            }
            let sid = sid_to_string(entry.Sid)?;
            if !is_capability_sid(&sid) {
                isolation_sids.push(sid);
            }
        }
        Ok(isolation_sids)
    }

    fn is_well_known_sid(sid: *mut c_void) -> bool {
        (0..=WinBuiltinDeviceOwnersSid).any(|kind| unsafe { IsWellKnownSid(sid, kind) != 0 })
    }

    fn sid_is_mapped(sid: *mut c_void) -> io::Result<bool> {
        const ERROR_INSUFFICIENT_BUFFER: i32 = 122;
        const ERROR_NONE_MAPPED: i32 = 1332;

        let mut name_length = 0_u32;
        let mut domain_length = 0_u32;
        let mut use_type: SID_NAME_USE = 0;
        if unsafe {
            LookupAccountSidW(
                ptr::null(),
                sid,
                ptr::null_mut(),
                &mut name_length,
                ptr::null_mut(),
                &mut domain_length,
                &mut use_type,
            )
        } != 0
        {
            return Ok(true);
        }
        let error = io::Error::last_os_error();
        match error.raw_os_error() {
            Some(ERROR_NONE_MAPPED) => Ok(false),
            Some(ERROR_INSUFFICIENT_BUFFER) => Ok(true),
            _ => Err(error),
        }
    }

    fn is_capability_sid(sid: &str) -> bool {
        sid == "S-1-15-3" || sid.starts_with("S-1-15-3-")
    }

    #[cfg(test)]
    pub(super) fn current_user_sid() -> io::Result<String> {
        let token = current_process_token()?;
        token_user_sid(token.0)
    }

    fn sid_to_string(sid: *mut c_void) -> io::Result<String> {
        let mut sid_string = ptr::null_mut();
        if unsafe { ConvertSidToStringSidW(sid, &mut sid_string) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let _sid_string = LocalAllocation(sid_string.cast());
        let mut length = 0;
        while unsafe { *sid_string.add(length) } != 0 {
            length += 1;
        }
        String::from_utf16(unsafe { slice::from_raw_parts(sid_string, length) })
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn relative_acl_target_uses_the_current_directory_as_parent() {
            assert_eq!(acl_parent(Path::new("state.sqlite")), Some(Path::new(".")));
        }
    }
}

#[cfg(all(test, windows))]
mod windows_permission_tests {
    use super::*;

    #[test]
    fn protected_acl_that_allows_everyone_is_not_private() {
        assert!(!windows_permissions::test_sddl_is_private("D:P(A;OICI;FA;;;WD)").unwrap());
    }

    #[test]
    fn protected_acl_with_an_unknown_user_is_not_private() {
        assert!(
            !windows_permissions::test_sddl_is_private("D:P(A;OICI;FA;;;S-1-5-21-999-999-999-999)")
                .unwrap()
        );
    }

    #[test]
    fn protected_acl_with_a_capability_sid_is_not_private() {
        assert!(!windows_permissions::test_sddl_is_private("D:P(A;OICI;FA;;;S-1-15-3-1)").unwrap());
    }

    #[test]
    fn unprotected_acl_is_not_private() {
        assert!(!windows_permissions::test_sddl_is_private("D:(A;;FA;;;WD)").unwrap());
    }

    #[test]
    fn applied_private_acl_is_private() {
        let user = windows_permissions::current_user_sid().unwrap();
        let sddl = format!("D:P(A;OICI;FA;;;{user})(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)");
        assert!(windows_permissions::test_sddl_is_private(&sddl).unwrap());
    }

    #[test]
    fn applied_private_acl_keeps_current_process_read_write_access() {
        use std::fs::OpenOptions;
        use std::io::{Read, Seek, SeekFrom, Write};

        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("private");
        fs::create_dir(&directory).unwrap();
        set_private_dir(&directory).unwrap();

        let file = directory.join("state.sqlite");
        fs::write(&file, b"before").unwrap();
        set_private_file(&file).unwrap();
        let mut opened = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&file)
            .unwrap();
        opened.seek(SeekFrom::End(0)).unwrap();
        opened.write_all(b"-after").unwrap();
        opened.seek(SeekFrom::Start(0)).unwrap();
        let mut contents = String::new();
        opened.read_to_string(&mut contents).unwrap();
        assert_eq!(contents, "before-after");
    }
}
