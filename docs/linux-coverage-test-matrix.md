# Linux: độ phủ gói cài và ma trận test

- **Ngày**: 2026-11-21 (bản 0.9.1-beta)
- **Liên quan**: [LINUX_README.md](../LINUX_README.md) (cài và bật bộ gõ theo từng desktop),
  `crates/buttre-platform/Cargo.toml` (`[package.metadata.deb]`, `[package.metadata.generate-rpm]`),
  `installers/linux/`, `addons/fcitx5-buttre/`, `.github/workflows/release.yml`

## Không cần một gói cho mỗi desktop

Một binary `buttre` chọn backend lúc chạy: `--ibus` (engine IBus, GNOME và desktop X11) hoặc
`--ime` (Wayland-native `zwp_input_method_v2` cho sway/Hyprland/KDE, tự fallback IBus). Gói
chính đã mang đủ file cho mọi desktop: `buttre.xml` (IBus component), `buttre-ime.desktop`
(`X-KDE-Wayland-VirtualKeyboard=true` cho KDE Wayland), autostart tray. Khác nhau giữa các
desktop chỉ ở bước bật sau khi cài:

| Môi trường | Cài | Bật |
|---|---|---|
| GNOME | gói chính | Settings → Keyboard → Input Sources → thêm buttre; Super+Space |
| Cinnamon (Linux Mint) | gói chính | System Settings → Keyboard → Layouts → thêm buttre; Super+Space |
| XFCE, MATE… (IBus) | gói chính | `ibus-setup` → thêm buttre |
| KDE Plasma Wayland | gói chính | System Settings → Virtual Keyboard → "buttre (Wayland IME)" |
| sway / Hyprland | gói chính | `exec buttre --ime` trong config compositor |
| Người dùng **fcitx5** | gói chính **+ `fcitx5-buttre`** | thêm buttre trong cấu hình fcitx5 |

Ngoại lệ duy nhất là fcitx5: một framework bộ gõ khác (không phải một desktop), cần addon C++
link `libFcitx5Core`. Addon phụ thuộc ABI C++ của bản fcitx5 lúc build nên hiện chỉ có `.deb`;
Fedora build từ source.

## Thị phần theo họ distro

Không có số liệu chung cho Linux desktop. Nguồn chi tiết nhất là
[Steam Survey 10/2026](https://www.gamingonlinux.com/2026/11/steam-survey-october-2026-shows-an-uptick-for-the-linux-user-share/),
lệch về người chơi game (Ubuntu/Mint ngoài đời phổ biến hơn). Không tìm được số liệu riêng
cho người dùng Việt Nam. Gộp theo họ, bỏ SteamOS (Steam Deck, read-only, 22% số Linux trên
Steam), còn 78%:

| Họ distro | Distro trong survey (%) | % Linux trên Steam | Tỷ lệ sau khi bỏ SteamOS | Gói buttre |
|---|---|---|---|---|
| Arch | CachyOS 16,4 · Arch 8,9 · EndeavourOS 1,4 | 26,7 | **34%** | chưa có |
| Debian/Ubuntu | Mint 8,4 · Ubuntu (26.04, 24.04, Core) 8,2 · Debian 1,9 · Pop!_OS 1,3 | 19,7 | **25%** | `.deb` |
| Fedora | Bazzite 6,7 · Fedora KDE 2,8 · Workstation 1,8 · Nobara 1,5 | 12,8 | **16%** | `.rpm` |
| Khác / Flatpak runtime | — | 18,8 | 24% | — |

- `.deb` + `.rpm` phủ khoảng **41%**; thêm gói **AUR** (`PKGBUILD`, ví dụ `buttre-bin` từ
  binary CI) lên khoảng **75%** — việc đáng làm nhất tiếp theo.
- **Flatpak không khả thi** cho bộ gõ: engine IBus và addon fcitx5 phải nằm trên host.
- Bazzite / Fedora Silverblue (immutable): dùng `.rpm` sẵn có qua `rpm-ostree install`
  (cần reboot).
- SteamOS: ngoài phạm vi.

## Ma trận test (4 máy ảo)

Trục ảnh hưởng tới bộ gõ là định dạng gói, framework (IBus/fcitx5), backend (`--ibus` /
`--ime`) và session (X11/Wayland), không phải tên distro:

| # | Máy ảo | Gói | Thứ cần test | Đại diện cho |
|---|---|---|---|---|
| 1 | **Ubuntu 26.04 LTS** GNOME | `.deb` | IBus trên Wayland (GNOME 50 không còn session X11) | Ubuntu, Zorin, Debian GNOME |
| 2 | **Linux Mint 22.3** Cinnamon | `.deb` + `fcitx5-buttre` | IBus trên X11 **và fcitx5** | Mint, MX, Xfce/MATE X11 |
| 3 | **Fedora 44 KDE** | `.rpm` | KDE Wayland qua `--ime` (Virtual Keyboard) | Fedora KDE, Nobara, Bazzite |
| 4 | **CachyOS** KDE (hoặc Arch + Hyprland) | AUR (chưa có) | fcitx5 trên Arch, Hyprland `--ime` | CachyOS, Arch, EndeavourOS, Manjaro |

Không cần máy riêng: sway (CI đã chạy headless sway), IBus headless (CI chạy `ibus-daemon`
thật — `scripts/ci-ibus-smoke.sh`, `scripts/ci-wayland-smoke.sh`), Debian thuần (cùng gói và
backend với máy 1/2).

WSL2 + WSLg chỉ đủ thử nhanh engine và gói `.deb` (gõ vào app GUI Linux như gedit): không có
desktop (Input Sources, tray, autostart), compositor dựa trên Weston nên `--ime` rơi về IBus,
phải tự chạy `ibus-daemon -drx` và đặt `GTK_IM_MODULE`/`QT_IM_MODULE`/`XMODIFIERS`, và phải
tắt bàn phím buttre trên Windows (Windows biến đổi phím trước khi tới WSL). Xem
`scripts/setup-ibus-wsl2.sh`.

**Chưa kiểm chứng:**
- Pop!_OS 24.04 dùng COSMIC, compositor riêng — chưa xác minh hỗ trợ `zwp_input_method_v2`.
- Cinnamon Wayland trên Mint vẫn thử nghiệm.

Thêm máy thứ 5 khi có báo lỗi từ một trong hai.

## Thứ tự

1. Máy 1 và 2: phủ Ubuntu/Mint (giả định là nhóm người Việt dùng nhiều nhất, chưa có số
   liệu) và fcitx5.
2. Máy 3.
3. Viết `PKGBUILD` AUR (có thể sinh trong `release.yml`), test trên máy 4.
