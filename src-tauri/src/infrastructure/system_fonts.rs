use crate::domain::error::{BackendError, ErrorCode};

#[cfg(windows)]
pub fn installed_families() -> Result<Vec<String>, BackendError> {
    use std::{collections::BTreeSet, ptr};
    use windows_sys::Win32::Graphics::Gdi::{
        CreateCompatibleDC, DEFAULT_CHARSET, DeleteDC, EnumFontFamiliesExW, LOGFONTW, TEXTMETRICW,
    };

    unsafe extern "system" fn collect(
        font: *const LOGFONTW,
        _: *const TEXTMETRICW,
        _: u32,
        context: isize,
    ) -> i32 {
        // GDI invokes this synchronously with a valid LOGFONTW. The set stays
        // alive and exclusively borrowed until EnumFontFamiliesExW returns.
        let face = unsafe { &(*font).lfFaceName };
        let length = face.iter().position(|unit| *unit == 0).unwrap_or(face.len());
        let name = String::from_utf16_lossy(&face[..length]);
        // @ families are vertical-writing aliases, not separate UI fonts.
        if !name.is_empty() && !name.starts_with('@') {
            unsafe { &mut *(context as *mut BTreeSet<String>) }.insert(name);
        }
        1
    }

    let mut families = BTreeSet::<String>::new();
    let filter = LOGFONTW { lfCharSet: DEFAULT_CHARSET, ..Default::default() };
    // Enumerate locally through GDI: no shell process or terminal window.
    unsafe {
        let dc = CreateCompatibleDC(ptr::null_mut());
        if dc.is_null() {
            return Err(BackendError::new(ErrorCode::Io, "无法读取系统字体。"));
        }
        EnumFontFamiliesExW(dc, &filter, Some(collect), &mut families as *mut _ as isize, 0);
        DeleteDC(dc);
    }
    if families.is_empty() {
        return Err(BackendError::new(ErrorCode::Io, "未能获取已安装字体，请重试。"));
    }
    Ok(families.into_iter().collect())
}

#[cfg(not(windows))]
pub fn installed_families() -> Result<Vec<String>, BackendError> {
    Err(BackendError::new(ErrorCode::Io, "当前平台暂不支持读取系统字体。"))
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn enumerates_real_windows_fonts_without_vertical_aliases_or_duplicates() {
        let families = installed_families().expect("Windows should expose installed fonts");
        assert!(!families.is_empty());
        assert!(families.iter().all(|name| !name.is_empty() && !name.starts_with('@')));
        assert!(families.windows(2).all(|pair| pair[0] < pair[1]));
        eprintln!("Enumerated {} installed font families", families.len());
    }
}
