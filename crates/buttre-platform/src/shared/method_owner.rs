//! Ai sở hữu lựa chọn kiểu gõ trên máy này — ADR-0003, phương án C.
//!
//! > buttre sở hữu lựa chọn kiểu gõ CHỈ ở nơi OS không cho ta một menu,
//! > hoặc nơi ta buộc phải phân xử nhiều đường truyền. Còn lại, OS lo.
//!
//! Một hàm duy nhất trả lời câu hỏi đó ([`decide`]) và `main.rs` phân nhánh
//! theo nó — quyết định không được rải rác thành `#[cfg]` ở nhiều chỗ. Trên
//! Linux câu trả lời phụ thuộc RUNTIME, gộp từ nhiều tín hiệu độc lập
//! (`backend_detect`: kwinrc, biến môi trường IM, shell tự chạy IBus, daemon
//! trên D-Bus, giao thức của compositor), nên nó là hàm chứ không phải hằng
//! số biên dịch.

/// Ai sở hữu lựa chọn kiểu gõ (và kèm theo nó: có dựng tray hay không).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MethodOwner {
    /// OS đã có menu kiểu gõ của engine (IBus properties, fcitx5 panel,
    /// macOS input-source menu) — buttre KHÔNG dựng tray; `buttre` không
    /// tham số mở cửa sổ Cấu hình.
    Os,
    /// Không có menu OS nào (Wayland-native), hoặc phải phân xử nhiều đường
    /// truyền (Windows: TSF + hook) — buttre dựng tray và sở hữu kiểu gõ.
    Buttre,
}

/// Bề mặt đang phục vụ máy này, quy về dạng trung lập nền tảng để bảng
/// quyết định ([`owner_of`]) test được trên MỌI OS — `backend_detect` chỉ
/// biên dịch trên Linux nên không thể là đầu vào của bảng.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfacePath {
    /// Windows with a buttre keyboard added: every input method is its own
    /// Windows keyboard (TSF profile), switched with Win+Space or buttre's
    /// Ctrl+Shift+0…9 inside the text service — no tray (ADR-0004).
    WindowsTsf,
    /// Windows without a buttre keyboard added, or `buttre --hook`: the
    /// global keyboard hook, which only buttre can steer — tray.
    WindowsHook,
    /// macOS IMKit: menu input-source của OS — mỗi kiểu gõ (Telex, VNI, Nôm,
    /// từng keyboard đóng gói) là một input mode riêng trong Info.plist.
    MacosImkit,
    /// fcitx5 đang chạy VÀ addon fcitx5-buttre đã cài — addon dựng menu
    /// kiểu gõ trên panel (PR #17). Chỉ khi addon có mặt thì "OS có menu"
    /// mới là sự thật.
    LinuxFcitx5,
    /// fcitx5 đang chạy nhưng addon CHƯA cài: fcitx5 không vẽ menu buttre
    /// nào (gõ tiếng Việt đi qua ibus/wayland, `--doctor` đã cảnh báo xung
    /// đột) — bỏ tray ở đây là để người dùng không còn công tắc nào.
    LinuxFcitx5NoAddon,
    /// ibus-daemon đang chạy — mỗi kiểu gõ là một input source IBus riêng
    /// (`ibus_engines.rs`, ADR-0005).
    LinuxIbus,
    /// Wayland-native `zwp_input_method_v2`/`v1`: KWin spawns `buttre --ime`,
    /// or a wlroots compositor offers the protocol — compositor không vẽ
    /// menu kiểu gõ nào cho ta.
    LinuxWaylandNative,
    /// Plasma Wayland mà KWin CHƯA chạy buttre (Virtual Keyboard chưa chọn
    /// buttre, hoặc đang chọn IME khác không có daemon): chưa gõ được — tray
    /// vẫn dựng để báo người dùng cách bật (`kwin_ime::notify_not_enabled`).
    LinuxKwinUnconfigured,
    /// Không phát hiện được đường IME nào (X11 không daemon, hoặc Wayland
    /// không có giao thức) — không có menu OS, giữ tray như trước giờ.
    LinuxUndetected,
}

/// Bảng quyết định thuần (ADR-0003) — test pin từng dòng.
pub fn owner_of(path: SurfacePath) -> MethodOwner {
    match path {
        SurfacePath::MacosImkit
        | SurfacePath::LinuxFcitx5
        | SurfacePath::LinuxIbus
        | SurfacePath::WindowsTsf => MethodOwner::Os,
        SurfacePath::WindowsHook
        | SurfacePath::LinuxWaylandNative
        | SurfacePath::LinuxFcitx5NoAddon
        | SurfacePath::LinuxKwinUnconfigured
        | SurfacePath::LinuxUndetected => MethodOwner::Buttre,
    }
}

/// Bề mặt đang hiệu lực, chờ tối đa `wait` nếu phiên đã cấu hình một
/// framework (IBus/fcitx5) mà daemon của nó chưa chạy — lúc đăng nhập tray
/// và ibus-daemon khởi động cùng một giây, kết luận sớm là kết luận sai.
/// Hết `wait` thì quyết định với những gì đang chạy. Trên Linux, chặn
/// (blocking) vì probe D-Bus/Wayland — không bao giờ gọi trên đường phím.
pub fn settled_surface(wait: std::time::Duration) -> SurfacePath {
    #[cfg(platform_linux)]
    {
        use crate::platforms::linux::backend_detect::{self, Judgement};
        let deadline = std::time::Instant::now() + wait;
        loop {
            let evidence = backend_detect::gather();
            let waiting = std::time::Instant::now() < deadline;
            match backend_detect::judge(&evidence, waiting) {
                Judgement::Settled(surface) => return surface,
                Judgement::Pending(framework) => {
                    tracing::info!("waiting for {framework:?} to start ({evidence:?})");
                    std::thread::sleep(std::time::Duration::from_millis(500));
                }
            }
        }
    }
    #[cfg(not(platform_linux))]
    {
        let _ = wait;
        current_surface()
    }
}

/// Bề mặt đang hiệu lực trên máy này, không chờ (doctor, cửa sổ Cấu hình,
/// lần kiểm tra lại định kỳ của tray).
pub fn current_surface() -> SurfacePath {
    #[cfg(platform_windows)]
    {
        use crate::platforms::windows::tsf::{lang_check, registration};
        // `--hook` runs the hook app deliberately, whatever is installed. The
        // TSF text service keeps publishing its transport claim, so in apps
        // where a buttre keyboard is active the hook still stands down.
        let forced_hook = std::env::args().any(|arg| arg == "--hook");
        if !forced_hook
            && registration::is_tsf_registered()
            && lang_check::is_buttre_text_service_enabled()
        {
            SurfacePath::WindowsTsf
        } else {
            SurfacePath::WindowsHook
        }
    }
    #[cfg(platform_macos)]
    {
        SurfacePath::MacosImkit
    }
    #[cfg(platform_linux)]
    {
        settled_surface(std::time::Duration::ZERO)
    }
}

/// [`current_surface`] + [`owner_of`] — no waiting (the config window).
pub fn decide() -> MethodOwner {
    owner_of(current_surface())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bảng ADR-0003, từng dòng một — đổi chủ sở hữu của một nền tảng
    /// (ví dụ macOS hoá ra không render được menu) phải sửa test này có
    /// chủ ý, không thể trôi ngầm.
    #[test]
    fn adr_0003_ownership_table() {
        assert_eq!(owner_of(SurfacePath::LinuxIbus), MethodOwner::Os);
        assert_eq!(owner_of(SurfacePath::LinuxFcitx5), MethodOwner::Os);
        assert_eq!(owner_of(SurfacePath::MacosImkit), MethodOwner::Os);
        assert_eq!(
            owner_of(SurfacePath::LinuxWaylandNative),
            MethodOwner::Buttre
        );
        // ADR-0004: Windows TSF hands method choice to the OS keyboard list;
        // the hook app keeps the tray.
        assert_eq!(owner_of(SurfacePath::WindowsTsf), MethodOwner::Os);
        assert_eq!(owner_of(SurfacePath::WindowsHook), MethodOwner::Buttre);
        assert_eq!(owner_of(SurfacePath::LinuxUndetected), MethodOwner::Buttre);
        // Plasma chưa chọn buttre: tray là chỗ duy nhất báo cách bật.
        assert_eq!(
            owner_of(SurfacePath::LinuxKwinUnconfigured),
            MethodOwner::Buttre
        );
        // fcitx5 sống nhưng addon chưa cài = KHÔNG có menu OS nào vẽ buttre
        // — bỏ tray ở đây là lấy mất công tắc duy nhất của người dùng.
        assert_eq!(
            owner_of(SurfacePath::LinuxFcitx5NoAddon),
            MethodOwner::Buttre
        );
    }
}
