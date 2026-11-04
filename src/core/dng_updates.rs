use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{path::Path, time::Duration};

pub const DOWNLOAD: &str = "https://adobe.com/go/dng_converter_win/";
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct UpdateCache {
    pub date: String,
    pub latest: Option<String>,
    pub error: Option<String>,
}
pub fn version(value: &str) -> Result<[u32; 4]> {
    let parts = value.split('.').collect::<Vec<_>>();
    if !(2..=4).contains(&parts.len()) {
        bail!("Unbekannte Versionsnummer");
    }
    let mut result = [0; 4];
    for (index, part) in parts.iter().enumerate() {
        if part.is_empty() || !part.chars().all(|c| c.is_ascii_digit()) {
            bail!("Ungültige Versionsnummer");
        }
        result[index] = part.parse()?;
    }
    Ok(result)
}
pub fn from_download_url(text: &str) -> Result<String> {
    let url = reqwest::Url::parse(text)?;
    if url.scheme() != "https" || url.host_str() != Some("download.adobe.com") {
        bail!("Unerwartetes Adobe-Downloadziel");
    }
    let file = url
        .path()
        .strip_prefix("/pub/adobe/dng/win/AdobeDNGConverter_")
        .context("Adobe-Downloadformat hat sich geändert")?;
    let number = file
        .strip_prefix("x64_")
        .or_else(|| file.strip_prefix("arm64_"))
        .and_then(|s| s.strip_suffix(".exe"))
        .context("Adobe-Downloadformat hat sich geändert")?
        .replace('_', ".");
    version(&number)?;
    Ok(number)
}
pub fn check(mut cache: UpdateCache) -> UpdateCache {
    let today = chrono::Local::now().date_naive().to_string();
    if cache.date == today {
        return cache;
    }
    cache.date = today;
    let result = (|| -> Result<String> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::limited(8))
            .build()?;
        let response = client
            .head(DOWNLOAD)
            .header("Cache-Control", "no-cache")
            .send()?
            .error_for_status()?;
        from_download_url(response.url().as_str())
    })();
    match result {
        Ok(latest) => {
            cache.latest = Some(latest);
            cache.error = None;
        }
        Err(error) => {
            cache.latest = None;
            cache.error = Some(error.to_string());
        }
    }
    cache
}
pub fn installed(path: &Path) -> Option<String> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "version")]
        unsafe extern "system" {
            fn GetFileVersionInfoSizeW(name: *const u16, handle: *mut u32) -> u32;
            fn GetFileVersionInfoW(
                name: *const u16,
                handle: u32,
                len: u32,
                data: *mut std::ffi::c_void,
            ) -> i32;
            fn VerQueryValueW(
                data: *const std::ffi::c_void,
                sub: *const u16,
                out: *mut *mut std::ffi::c_void,
                len: *mut u32,
            ) -> i32;
        }
        let name = path
            .as_os_str()
            .encode_wide()
            .chain([0])
            .collect::<Vec<_>>();
        let mut ignored = 0;
        // SAFETY: valid NUL-terminated UTF-16 path and output DWORD.
        let size = unsafe { GetFileVersionInfoSizeW(name.as_ptr(), &mut ignored) };
        if size == 0 {
            return None;
        }
        let mut bytes = vec![0u8; size as usize];
        // SAFETY: buffer has the size returned by Windows for this version resource.
        if unsafe { GetFileVersionInfoW(name.as_ptr(), 0, size, bytes.as_mut_ptr().cast()) } == 0 {
            return None;
        }
        let sub = [b'\\' as u16, 0];
        let (mut pointer, mut length) = (std::ptr::null_mut(), 0u32);
        // SAFETY: resource buffer remains alive; outputs are valid writable locations.
        if unsafe {
            VerQueryValueW(
                bytes.as_ptr().cast(),
                sub.as_ptr(),
                &mut pointer,
                &mut length,
            )
        } == 0
            || length < 52
            || pointer.is_null()
        {
            return None;
        }
        // SAFETY: Windows returned a VS_FIXEDFILEINFO with at least 13 DWORDs.
        let info = unsafe { std::slice::from_raw_parts(pointer.cast::<u32>(), 13) };
        if info[0] != 0xFEEF04BD {
            return None;
        }
        let mut parts = vec![
            info[4] >> 16,
            info[4] & 0xffff,
            info[5] >> 16,
            info[5] & 0xffff,
        ];
        while parts.len() > 2 && parts.last() == Some(&0) {
            parts.pop();
        }
        Some(
            parts
                .iter()
                .map(|p| p.to_string())
                .collect::<Vec<_>>()
                .join("."),
        )
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        None
    }
}
