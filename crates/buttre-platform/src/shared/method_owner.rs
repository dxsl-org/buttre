//! Ai sở hữu lựa chọn kiểu gõ trên máy này — ADR-0003, phương án C.
//!
//! > buttre sở hữu lựa chọn kiểu gõ CHỈ ở nơi OS không cho ta một menu,
//! > hoặc nơi ta buộc phải phân xử nhiều đường truyền. Còn lại, OS lo.
//!
//! Một hàm duy nhất trả lời câu hỏi đó ([`decide`]) và `main.rs` phân nhánh
//! theo nó — quyết định không được rải rác thành `#[cfg]` ở nhiều chỗ. Trên
//! Linux câu trả lời phụ thuộc RUNTIME (fcitx5/ibus/wayland tự phát hiện qua
//! `backend_detect`), nên nó là hàm chứ không phải hằng số biên dịch; dùng
//! đúng `backend_detect::detect()` mà engine dùng, để tray và engine không
//! bao giờ lệch nhau về "đường IME nào đang phục vụ máy này".

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
    /// Windows: hai đường truyền (TSF + hook), chỉ buttre phân xử được.
    Windows,
    /// macOS IMKit: menu input-source của OS (giả định ADR-0003, chỉnh khi
    /// kiểm trên môi trường thật — đổi một dòng trong [`owner_of`]).
    MacosImkit,
    /// fcitx5 đang chạy VÀ addon fcitx5-buttre đã cài — addon dựng menu
    /// kiểu gõ trên panel (PR #17). Chỉ khi addon có mặt thì "OS có menu"
    /// mới là sự thật.
    LinuxFcitx5,
    /// fcitx5 đang chạy nhưng addon CHƯA cài: fcitx5 không vẽ menu buttre
    /// nào (gõ tiếng Việt đi qua ibus/wayland, `--doctor` đã cảnh báo xung
    /// đột) — bỏ tray ở đây là để người dùng không còn công tắc nào.
    LinuxFcitx5NoAddon,
    /// ibus-daemon đang chạy — `ibus_props.rs` đưa radio lên GNOME top-bar.
    LinuxIbus,
    /// Wayland-native `zwp_input_method_v2`/`v1` — compositor không vẽ menu
    /// kiểu gõ nào cho ta.
    LinuxWaylandNative,
    /// Không phát hiện được đường IME nào (X11 không daemon) — không có menu
    /// OS, giữ tray như hành vi trước giờ.
    LinuxUndetected,
}

/// Bảng quyết định thuần (ADR-0003) — test pin từng dòng.
pub fn owner_of(path: SurfacePath) -> MethodOwner {
    match path {
        SurfacePath::MacosImkit | SurfacePath::LinuxFcitx5 | SurfacePath::LinuxIbus => {
            MethodOwner::Os
        }
        SurfacePath::Windows
        | SurfacePath::LinuxWaylandNative
        | SurfacePath::LinuxFcitx5NoAddon
        | SurfacePath::LinuxUndetected => MethodOwner::Buttre,
    }
}

/// Bề mặt đang hiệu lực trên máy này. Trên Linux, chặn (blocking) vì probe
/// D-Bus — gọi lúc khởi động hoặc `--doctor`, không bao giờ trên đường phím.
pub fn current_surface() -> SurfacePath {
    #[cfg(platform_windows)]
    {
        SurfacePath::Windows
    }
    #[cfg(platform_macos)]
    {
        SurfacePath::MacosImkit
    }
    #[cfg(platform_linux)]
    {
        use crate::platforms::linux::backend_detect::{self, ImeBackend};
        use crate::platforms::linux::kwin_ime;
        // Plasma Wayland: KWin SỞ HỮU tiến trình IME (kwinrc trỏ buttre) —
        // đường gõ thật là wayland-native dù ibus-daemon vẫn thường chạy
        // kèm trong session. Hỏi kwinrc TRƯỚC khi hỏi daemon, không thì máy
        // Plasma bị xếp nhầm vào nhóm IBus/Os và mất tray (mất luôn nút
        // "Thoát" duy nhất tắt được KWin IME).
        if kwin_ime::manages_buttre_ime() {
            return SurfacePath::LinuxWaylandNative;
        }
        let probes = backend_detect::probe();
        match backend_detect::pick(probes) {
            Some(ImeBackend::Fcitx5) => {
                if backend_detect::fcitx5_addon_installed() {
                    SurfacePath::LinuxFcitx5
                } else {
                    SurfacePath::LinuxFcitx5NoAddon
                }
            }
            Some(ImeBackend::IBus) => SurfacePath::LinuxIbus,
            Some(ImeBackend::WaylandIme) => SurfacePath::LinuxWaylandNative,
            None => SurfacePath::LinuxUndetected,
        }
    }
}

/// [`current_surface`] + [`owner_of`] — cái `main.rs` phân nhánh theo.
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
        assert_eq!(owner_of(SurfacePath::Windows), MethodOwner::Buttre);
        assert_eq!(owner_of(SurfacePath::LinuxUndetected), MethodOwner::Buttre);
        // fcitx5 sống nhưng addon chưa cài = KHÔNG có menu OS nào vẽ buttre
        // — bỏ tray ở đây là lấy mất công tắc duy nhất của người dùng.
        assert_eq!(
            owner_of(SurfacePath::LinuxFcitx5NoAddon),
            MethodOwner::Buttre
        );
    }
}
