//! Help dialog using native Windows MessageBox

/// Show help dialog using native Windows MessageBox
#[cfg(target_os = "windows")]
pub fn show_help_dialog() {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONINFORMATION, MB_OK};

    let title = "buttre - Bộ Gõ Tiếng Việt";
    let message =
        "Phiên bản: 0.7.5-beta\n\
         Giấy phép: GPL 3.0\n\
         Chuẩn mã: NFC (Unicode dựng sẵn)\n\
         \n\
         KIỂU GÕ & PHÍM TẮT:\n\
         • Ctrl+Shift+Space    →  Bật/Tắt tiếng Việt\n\
         • Ctrl+Shift+1 → Telex\n\
         • Ctrl+Shift+2 → VNI\n\
         • Ctrl+Shift+3 → Nôm\n\
         • Ctrl+Shift+4 → ...\n\
         \n\
         TÀI LIỆU & MÃ NGUỒN:\n\
         https://github.com/dxsl-org/buttre\n\
         \n\
         BÁO LỖI:\n\
         https://github.com/dxsl-org/buttre/issues";

    // Convert to UTF-16 for Windows API
    let title_wide: Vec<u16> = OsStr::new(title)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let message_wide: Vec<u16> = OsStr::new(&message)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    // SAFETY:
    // 1. MessageBoxW is properly declared in windows_sys
    // 2. title_wide and message_wide are valid null-terminated UTF-16 strings
    // 3. as_ptr() returns valid pointer to the string data
    // 4. 0 for parent window means no parent (valid value)
    // 5. MB_OK and MB_ICONINFORMATION are valid MessageBox flags
    // 6. MessageBoxW is a blocking call that displays a modal dialog
    // 7. Strings remain valid for the duration of the call (on stack)
    unsafe {
        MessageBoxW(
            0, // No parent window
            message_wide.as_ptr(),
            title_wide.as_ptr(),
            MB_OK | MB_ICONINFORMATION,
        );
    }
}

#[cfg(not(target_os = "windows"))]
pub fn show_help_dialog() {
    eprintln!("Help dialog is only available on Windows");
}
