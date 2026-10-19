//! Windows only: the control channel's token file is made owner-only, and checked to be, through
//! the Win32 security API (wavcraft#8, Hyperion's check of CX-W5, 2026-10-19). The owner is the
//! user in this process's token, by SID, never the USERNAME variables. This module is the app's
//! one exception to "no `unsafe`" (README): each call below is a plain Win32 call with the
//! pointers it documents, and every buffer the system allocates is freed here.
#![allow(unsafe_code)]

use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows_sys::Win32::Foundation::{CloseHandle, ERROR_SUCCESS, GENERIC_ALL, HANDLE, LocalFree};
use windows_sys::Win32::Security::Authorization::{
    EXPLICIT_ACCESS_W, GetNamedSecurityInfoW, NO_MULTIPLE_TRUSTEE, SE_FILE_OBJECT, SET_ACCESS, SetEntriesInAclW, SetNamedSecurityInfoW,
    TRUSTEE_IS_SID, TRUSTEE_IS_USER, TRUSTEE_W,
};
use windows_sys::Win32::Security::{
    ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, ACL_SIZE_INFORMATION, AclSizeInformation, DACL_SECURITY_INFORMATION, EqualSid, GetAce, GetAclInformation,
    GetSecurityDescriptorControl, GetTokenInformation, NO_INHERITANCE, PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID,
    SE_DACL_PROTECTED, TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

/// `ACCESS_ALLOWED_ACE_TYPE` (winnt.h).
const ACCESS_ALLOWED: u8 = 0;

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
}

fn win32(code: u32, what: &str) -> io::Result<()> {
    if code == ERROR_SUCCESS { Ok(()) } else { Err(io::Error::other(format!("{what} failed: {}", io::Error::from_raw_os_error(code as i32)))) }
}

/// This process's user, as the bytes of a `TOKEN_USER` (its SID points inside them).
struct ProcessUser {
    buf: Vec<u64>,
}

impl ProcessUser {
    fn get() -> io::Result<ProcessUser> {
        let mut token: HANDLE = std::ptr::null_mut();
        // SAFETY: GetCurrentProcess returns a pseudo-handle that needs no closing; `token` is a
        // valid out-pointer for the opened token handle.
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut len = 0u32;
        // SAFETY: a size query (null buffer, length 0); `len` receives the size needed.
        unsafe { GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut len) };
        // u64 words keep the TOKEN_USER (and the SID inside it) suitably aligned.
        let mut buf = vec![0u64; (len as usize).div_ceil(8).max(1)];
        // SAFETY: `buf` holds at least `len` bytes; the call writes a TOKEN_USER there.
        let ok = unsafe { GetTokenInformation(token, TokenUser, buf.as_mut_ptr().cast(), (buf.len() * 8) as u32, &mut len) };
        let err = io::Error::last_os_error();
        // SAFETY: `token` was opened above and is closed once.
        unsafe { CloseHandle(token) };
        if ok == 0 {
            return Err(err);
        }
        Ok(ProcessUser { buf })
    }

    fn sid(&self) -> PSID {
        // SAFETY: GetTokenInformation(TokenUser) filled `buf` with a TOKEN_USER; reading its SID
        // pointer (into the same buffer) is valid while `self` lives.
        unsafe { (*self.buf.as_ptr().cast::<TOKEN_USER>()).User.Sid }
    }
}

/// Give `path` a protected DACL (nothing inherited) with one entry: full control for this
/// process's user.
pub fn set_owner_only(path: &Path) -> io::Result<()> {
    let user = ProcessUser::get()?;
    let access = EXPLICIT_ACCESS_W {
        grfAccessPermissions: GENERIC_ALL,
        grfAccessMode: SET_ACCESS,
        grfInheritance: NO_INHERITANCE,
        Trustee: TRUSTEE_W {
            pMultipleTrustee: std::ptr::null_mut(),
            MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
            TrusteeForm: TRUSTEE_IS_SID,
            TrusteeType: TRUSTEE_IS_USER,
            ptstrName: user.sid().cast(),
        },
    };
    let mut acl: *mut ACL = std::ptr::null_mut();
    // SAFETY: one EXPLICIT_ACCESS_W whose trustee SID lives in `user`; no old ACL; `acl`
    // receives a LocalAlloc'd ACL that is freed below.
    win32(unsafe { SetEntriesInAclW(1, &access, std::ptr::null(), &mut acl) }, "SetEntriesInAclW")?;
    let name = wide(path);
    // SAFETY: `name` is a NUL-terminated path; `acl` is the valid ACL made above; owner, group
    // and SACL are left alone (null).
    let set = unsafe {
        SetNamedSecurityInfoW(
            name.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            acl,
            std::ptr::null(),
        )
    };
    // SAFETY: `acl` came from SetEntriesInAclW (LocalAlloc) and is freed once.
    unsafe { LocalFree(acl.cast()) };
    win32(set, "SetNamedSecurityInfoW")
}

/// None when `path`'s DACL is protected and every allow entry is for this process's user; else
/// what is wrong, for the refusal.
pub fn others_can_read(path: &Path) -> io::Result<Option<String>> {
    let user = ProcessUser::get()?;
    let name = wide(path);
    let (mut dacl, mut sd): (*mut ACL, PSECURITY_DESCRIPTOR) = (std::ptr::null_mut(), std::ptr::null_mut());
    // SAFETY: `name` is NUL-terminated; only the DACL is asked for; `dacl` points into `sd`,
    // which the system allocates and is freed below.
    win32(
        unsafe {
            GetNamedSecurityInfoW(
                name.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut dacl,
                std::ptr::null_mut(),
                &mut sd,
            )
        },
        "GetNamedSecurityInfoW",
    )?;
    let verdict = (|| {
        let (mut control, mut revision) = (0u16, 0u32);
        // SAFETY: `sd` is the descriptor returned above.
        if unsafe { GetSecurityDescriptorControl(sd, &mut control, &mut revision) } == 0 {
            return Err(io::Error::last_os_error());
        }
        if dacl.is_null() {
            return Ok(Some("no DACL: everyone has access".to_string()));
        }
        if control & SE_DACL_PROTECTED == 0 {
            return Ok(Some("inherited entries".to_string()));
        }
        let mut info = ACL_SIZE_INFORMATION { AceCount: 0, AclBytesInUse: 0, AclBytesFree: 0 };
        // SAFETY: `dacl` is a valid ACL inside `sd`; `info` is an ACL_SIZE_INFORMATION.
        if unsafe {
            GetAclInformation(dacl, (&mut info as *mut ACL_SIZE_INFORMATION).cast(), size_of::<ACL_SIZE_INFORMATION>() as u32, AclSizeInformation)
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let mut others = 0;
        for i in 0..info.AceCount {
            let mut ace: *mut core::ffi::c_void = std::ptr::null_mut();
            // SAFETY: `i` < AceCount; `ace` receives a pointer into the ACL.
            if unsafe { GetAce(dacl, i, &mut ace) } == 0 {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: every ACE starts with an ACE_HEADER.
            let header = unsafe { *ace.cast::<ACE_HEADER>() };
            if header.AceType != ACCESS_ALLOWED {
                continue; // deny entries take access away
            }
            // SAFETY: an ACCESS_ALLOWED ACE: its SID starts at SidStart, inside the ACE.
            let sid: PSID = unsafe { std::ptr::addr_of_mut!((*ace.cast::<ACCESS_ALLOWED_ACE>()).SidStart).cast() };
            // SAFETY: both are valid SIDs.
            if unsafe { EqualSid(sid, user.sid()) } == 0 {
                others += 1;
            }
        }
        Ok((others > 0).then(|| format!("{others} entr{} for other accounts", if others == 1 { "y" } else { "ies" })))
    })();
    // SAFETY: `sd` came from GetNamedSecurityInfoW (LocalAlloc) and is freed once.
    unsafe { LocalFree(sd) };
    verdict
}
