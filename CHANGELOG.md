# Nhật Ký Thay Đổi

Tất cả thay đổi đáng chú ý của buttre được ghi lại tại đây. Định dạng theo [Keep a Changelog](https://keepachangelog.com); phiên bản theo SemVer.

## [Unreleased]

## [0.9.1-beta] — 2026-11-21

- build: sửa MSI và .deb/.rpm không build được với Rust 1.100 — Cargo đổi thư mục `OUT_DIR` (`build/<pkg>/<hash>/out`), `build.rs` chép `keyboards/*.toml` và `buttre_nom.db` vào `target/release/build/` thay vì `target/release/`; giờ tìm thư mục `build` gần nhất thay vì lùi cố định 3 cấp
- windows (TSF): mỗi kiểu gõ là một bàn phím Windows riêng — "buttre Telex", "buttre VNI", "buttre Chữ Nôm" và từng bàn phím TOML — chọn bằng Win+Space như mọi bàn phím khác. Kiểu gõ lấy theo profile đang bật (`OnActivated`/`GetActiveProfile`), không còn đọc `Settings::input_method`. Telex giữ GUID profile cũ nên bàn phím đã thêm trước đó vẫn còn; GUID của các kiểu khác tính cố định từ id (`tsf/profiles.rs`), TOML thêm sau có profile khi đăng ký lại
- windows (TSF): Ctrl+Shift+1…9 chuyển sang bàn phím buttre thứ N (1 Telex, 2 VNI, 3 Nôm, …) và bật buttre; Ctrl+Shift+` tắt buttre (gõ thẳng, bàn phím giữ nguyên) — Ctrl+Shift+0 cũng tắt nhưng Windows thường chặn tổ hợp này. Bật/tắt chung cho mọi ứng dụng (global compartment của TSF), không ghi file
- windows (TSF): icon buttre cạnh chữ ENG trên thanh tác vụ (`GUID_LBI_INPUTMODE`): icon kiểu gõ khi bật, "E" khi tắt; bấm trái bật/tắt, bấm phải mở menu "Cấu hình buttre…" / Bật–Tắt
- windows: bản TSF không còn tray — đã thêm bàn phím buttre thì `buttre` mở cửa sổ Cấu hình, chạy lúc đăng nhập thì thoát im lặng; tray chỉ còn ở bản Hook (chưa thêm bàn phím buttre nào, hoặc `buttre --hook`). Bỏ chế độ TSF + hook chạy cùng lúc và `Settings::hook_fallback`; app hook chạy riêng vẫn đứng yên trong app đang dùng bàn phím buttre (`transport_claim`)
- installer (macOS): bộ cài `buttre-<version>-macos.pkg` — bấm đúp là cài, nút Customize của Installer.app chọn bàn phím như MSI Windows: Telex, VNI, Chữ Nôm chọn sẵn, từng bàn phím TOML không chọn. App vào `/Library/Input Methods`, mỗi bàn phím được chọn bật ngay trong phiên người đang đăng nhập; cài xong khởi động lại `imklaunchagent` của người dùng — bản đang chạy giữ trạng thái của bundle cũ, bàn phím hiện trong menu nhưng không gõ được tới khi khởi động lại máy (`install.sh` cũng vậy); bật lỗi không làm hỏng cả bộ cài. Cài đè dừng bản đang chạy và xoá bản cũ trước (file thừa trong bundle làm hỏng chữ ký). `hosts/macos/build_pkg.sh` build từ `Buttre.app` đã build, danh sách bàn phím và tên lấy từ chính bundle; release ký Developer ID Installer + notarize khi có secret `MACOS_INSTALLER_CERT_P12_BASE64`, không có thì chỉ phát hành zip như trước. `buttre --install [method…]` nhận danh sách kiểu gõ (không đối số = Telex/VNI/Nôm như cũ; kiểu gõ không có trong bundle → lỗi, không đụng tới TIS)
- installer (Windows): bước chọn bàn phím — Telex, VNI, Chữ Nôm chọn sẵn, 7 bàn phím TOML không chọn (vẫn đăng ký, thêm sau được trong Settings); mục chọn sẵn "Tắt Ctrl+Shift đổi bàn phím của Windows" (để Ctrl+Shift+1…9 của buttre không bị Windows giành). Bàn phím được chọn thêm vào danh sách của người cài bằng `buttre --add-keyboard <id>` (chỉ dưới ngôn ngữ có sẵn trong danh sách của người dùng — không bao giờ thêm ngôn ngữ mới). Sửa luôn lỗi link MSI có sẵn từ trước (`LGHT0311`: chuỗi tiếng Việt không vừa codepage 1252) bằng `buttre.wxl` codepage UTF-8
- config (Windows): ẩn "Gõ thẳng không gạch chân" — chỉ IBus/Wayland/macOS đọc `use_preedit`, backend Hook lẫn TSF bỏ qua; giá trị vẫn được giữ nguyên khi lưu từ Windows
- windows (TSF): sửa luồng phím — phím nào `OnTestKeyDown` không nhận thì `OnKeyDown` không bao giờ chạy, nên nhánh Backspace/Enter trong `OnKeyDown` là code chết. (1) Backspace giữa từ đi thẳng vào ứng dụng, xoá chữ mà engine vẫn giữ, phím dấu sau đó làm chữ đã xoá hiện lại — giờ Backspace được nhận khi đang có composition; xoá hết chữ thì composition được xoá trống rồi mới đóng (trước đây chữ cuối bị chốt lại thay vì xoá). (2) Enter, Tab, Delete, mũi tên, phím tắt (Ctrl+V…) giữa từ tới ứng dụng khi composition còn mở: dòng mới/dán rơi vào trong composition rồi bị lần cập nhật sau ghi đè, engine mang phím của từ cũ sang từ mới — giờ chốt từ (đồng bộ) ngay trong `OnTestKeyDown` trước khi ứng dụng xử lý phím; chỉ phím bổ trợ/khoá/IME không chốt. (3) `key_busy` chỉ xoá ở `OnKeyUp` vốn không bao giờ chạy, nên sau phím đầu tiên chuyển focus không còn đóng composition; đóng cả khi chuyển sang document khác cùng thread (tab Notepad), không chỉ khi mất focus hẳn. (4) Đóng composition không còn kéo con trỏ về cuối từ khi người dùng click ra chỗ khác. (5) Phím đã nhận mà engine trả `DoNothing` thì gõ ra nguyên ký tự thay vì mất. (6) Nhánh phím reset không còn giữ borrow `RefCell` của engine qua lời gọi COM (re-entrancy dưới `panic = "abort"` sẽ làm sập app chủ)
- windows (TSF): có host không bao giờ gọi `OnTestKeyDown` mà đưa mọi phím thẳng vào `OnKeyDown` (Notepad: 0 so với 459 lần đo được), nên giá trị trả về của `OnKeyDown` quyết định phím có bị nuốt. `OnKeyDown` giờ tự áp đúng quy tắc nhận phím của `OnTestKeyDown`: phím không nhận thì trả lại ứng dụng (chốt từ trước nếu phím tác động lên văn bản) — trước đó Backspace sau khi từ đã chốt bị nuốt hết
- windows (TSF): đăng ký thêm category `GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT` và `GUID_TFCAT_TIPCAP_SYSTRAYSUPPORT` (mọi IME bàn phím có sẵn của Windows đều khai báo) — thiếu chúng, Windows Terminal không bao giờ nạp buttre. Cần cài/đăng ký lại để có hiệu lực
- windows (TSF): chọn chữ Nôm xong con trỏ không còn nằm giữa chữ — TSF đo vị trí theo đơn vị UTF-16 còn buttre đếm theo ký tự, mà phần lớn chữ Nôm (𡗶) ngoài BMP chiếm 2 đơn vị, nên con trỏ rơi giữa cặp surrogate và phím gõ tiếp làm vỡ chữ. Đổi đơn vị một lần trong `write_text_with`
- windows (TSF): cửa sổ ứng viên Nôm giờ là owned window của cửa sổ ứng dụng (`ITfContextView::GetWnd`, không có thì `GetFocus`) theo yêu cầu IME của Microsoft, tự tạo lại nếu owner đóng làm nó mất theo; khi chưa có composition thì đo vị trí theo con trỏ (selection) thay vì rơi về (100, 100) ở góc màn hình, app từ chối edit session đồng bộ thì đo trễ rồi tự dời cửa sổ. Chưa kiểm chứng trên Windows Terminal
- scripts: `build-tsf.ps1 -Install` báo `--register-tsf failed (code )` dù đăng ký thành công — `buttre.exe` là GUI-subsystem nên PowerShell không chờ, giờ chờ bằng `Start-Process -Wait`; `check-tsf-status.ps1` mục 4 hỏng vì `$preloadKey` chưa từng được khai báo
- windows (Hook): sửa gõ trên Notepad Windows 11 — Nôm không hiện ứng viên (cả dòng ứng viên biến thành `))))))`), Telex/VNI gõ nhanh hoặc khi Space sửa từ thì mất/lặp dấu. Editor của Notepad (`RichEditD2DPT`) đọc sai `SendInput` khi hàng đợi phím dồn: mọi ký tự Unicode trong một lô ra thành ký tự CUỐI cùng, phần lớn Backspace bị rơi (giãn ≤5 ms/phím vẫn hỏng). EVKey/UniKey chế độ hook dính đúng lỗi này. Khi control đang focus là `RichEditD2DPT`, hook gửi chữ bằng `WM_KEYDOWN(VK_BACK)` + `WM_CHAR` post thẳng vào editor; phím lẽ ra đi thẳng (chữ không biến đổi, Backspace thường) cũng đi chung kênh này để giữ đúng thứ tự (message post luôn được xử lý trước phím còn trong hàng đợi). Ctrl+Shift+Z vẫn dùng `SendInput`. Enter/mũi tên/phím tắt vẫn đi đường thường: chữ gõ ngay sau chúng trong lúc Notepad còn lag vẫn có thể vượt lên
- windows (Hook): dòng ứng viên Nôm chèn vào văn bản bỏ ngoặc quanh nghĩa (`1𡗶 trời` thay `1𡗶 (trời)`) — mỗi ký tự inline phải gõ rồi xoá lại ở mỗi phím, trên Notepad Windows 11 mỗi ký tự tốn vài ms (đo: ~210 → ~150 ms/phím với 5 ứng viên). Cửa sổ ứng viên của TSF/IBus/macOS không đổi
- build: Rust 1.100 không còn biên dịch `objc2 0.4` (kéo vào bởi winit 0.29) trên macOS — code tray tách sang `tray_app.rs`, chỉ biên dịch ngoài macOS, và `winit` thành dependency không-macOS (trên macOS binary `buttre` chỉ là helper cửa sổ Cấu hình; ADR-0003)
- deps: `rusqlite` 0.30 → 0.43 (SQLite đóng gói ≥ 3.54, vá RUSTSEC-2026-0459); bỏ `chrono` (không dùng) khỏi `buttre-platform` và `thiserror` (không dùng) khỏi `buttre-engine`; `deny.toml` bỏ qua RUSTSEC-2026-0543 (chrono qua Slint, chưa có bản thay)
- ci: sửa lint clippy mới (`chunks_exact_mut` → `as_chunks_mut`, FFI `buttre_engine_select_method` thành `unsafe extern "C"` kèm `# Safety`)
- ci: sửa job IBus chập chờn — log engine (mỗi tiến trình một file, ghi từng phím ở mức debug) cho thấy ibus-daemon tự đổi engine toàn cục về `xkb:us::eng` vài giây sau khi khởi động (FocusOut → Disable → Destroy, không client nào yêu cầu); khi việc đó rơi vào giữa kịch bản thì mọi phím sau đi vào xkb (0/7). Script smoke chạy daemon không panel và chỉ bắt đầu gõ khi buttre giữ là engine hiện hành 6 giây liên tục

## [0.8.0-beta] — 2026-11-20

- ci: release có job `build-macos-app` build `Buttre.app` ký Developer ID + notarize trên keychain tạm, đính kèm `buttre-<version>-macos-app.zip` vào release; thiếu secret Apple thì job fail thay vì phát hành bản chưa ký. `build_app.sh` nhận thêm `MACOS_NOTARY_KEYCHAIN`. Job CI `macos-imkit` build với version `dev` (SHA commit không phải bundle version hợp lệ nên script từ chối)
- macos: "Gõ thẳng không gạch chân" giờ hoạt động — Telex/VNI chốt chữ ngay khi gõ, sửa dấu bằng `insertText:replacementRange:` thay cho marked text. Trước khi thay, host đọc lại đúng đoạn chữ trước con trỏ (`attributedSubstringFromRange:`); lệch (con trỏ đã di chuyển, app không đọc lại được) thì không đụng gì, bắt đầu từ mới và gõ lại phím đó. App không báo được `selectedRange` vẫn dùng gạch chân; Nôm luôn gạch chân (cần bảng ứng viên). Click chuột kết thúc từ đang gõ. FFI v4: `ButtreKeyResult.replaced` + `buttre_engine_sync_client`
- macos, linux (IBus/Wayland): "Xóa lùi theo phím thô" giờ có tác dụng — trước đây chỉ bàn phím của tray (backend Hook) áp `backspace_mode`; engine của IMKit/IBus/Wayland bỏ qua. Ở chế độ gạch chân Backspace giờ cũng bỏ phím thô cuối (`vieetj` ⌫ → `viêt`) thay vì luôn xoá theo ký tự hiển thị
- engine bridge: chuyển sang Nôm khi đang gõ thẳng không còn ánh xạ nhầm composition thành hành động gõ thẳng; phím điều hướng/phím tắt ở chế độ gõ thẳng kết thúc từ, để phím dấu sau đó không sửa lùi lên chữ không liên quan
- macos: sửa bấm "Cấu hình buttre…" trên menu input không mở cửa sổ Cấu hình — trên macOS crate `single-instance` `flock` một FILE theo đúng tên truyền vào, nên tên trần `buttre-config` rơi vào thư mục hiện hành; helper do IMKit spawn chạy ở `/` nên tạo file lock thất bại và thoát ngay. Lock giờ là đường dẫn tuyệt đối `~/Library/Application Support/buttre/buttre-config.lock` (cạnh `settings.toml`), nên chống mở hai cửa sổ cũng đúng bất kể cwd
- linux: addon fcitx5 giờ có package riêng `fcitx5-buttre-*.deb` trong release (Debian/Ubuntu) — cài là menu kiểu gõ hiện ngay trên panel fcitx5, không cần build từ source; kèm `libbuttre_ffi.so` tự chứa (RPATH `$ORIGIN`), Depends tự tính qua shlibdeps. Fedora/RPM vẫn build từ source (ABI C++ của fcitx5 khác theo distro). CI thêm job compile addon để hỏng header là biết ở PR, không đợi tới release
- macos: sửa `Buttre.app` đã cài nhưng không xuất hiện trong Input Sources — bundle ID chuyển sang dạng IMKit tương thích `org.dxsl.inputmethod.buttre`, khai báo ngôn ngữ đích `vi`, và build từ chối metadata làm macOS không thể phân loại bộ gõ; test FFI được cô lập để không còn phụ thuộc hoặc ghi đè kiểu gõ hiện tại của người dùng
- macos: sửa luồng chạy IMKit — (1) bôi đen rồi Cmd+C/Cmd+X, Shift+mũi tên, Tab… không còn xoá vùng chọn: engine chỉ trả `preedit` khi marked text thật sự đổi (`setMarkedText:@""` khi không có marked text sẽ thay thế vùng chọn); (2) phím không ra chữ (keypad Enter, F-key, phím ISO/JIS, dead key) chốt từ đang gõ trước khi tới app thay vì để app ghi đè làm mất chữ; (3) từ điển Nôm và keyboard đóng gói trong `Contents/Resources` giờ được nạp; (4) Nôm có bảng ứng viên `IMKCandidates` (↑/↓, Space/Enter/1–9/click chọn, Esc huỷ); (5) phím đọc theo ký tự của layout hiện tại (`event.characters`) nên Dvorak/AZERTY/Colemak gõ đúng; log ở `~/Library/Logs/buttre/imk.log`. FFI macOS lên v3 (`process_char`, `preedit` nullable, candidate API; bỏ `process_key`/`set_method`/`set_enabled`)
- macos: mỗi kiểu gõ giờ là một input source riêng trong menu bàn phím (buttre Telex, buttre VNI, buttre Chữ Nôm, và từng keyboard dân tộc đóng gói — Chăm, H'Mông, Khmer, Mường/Tày/Nùng, Tây Nguyên, Thái), ngang hàng ABC, đổi bằng click hoặc Ctrl+Space; bỏ mục English (tắt tiếng Việt = chọn ABC) và bỏ menu lệnh IMKit. Mode cũ `buttre Vietnamese` không còn — sau khi cài phải đăng xuất/đăng nhập rồi thêm các mode mới trong System Settings. FFI: `buttre_engine_select_method` thay `buttre_engine_menu`/`activate_menu_item`; macOS bỏ qua `Settings::enabled`
- macos: icon trên thanh menu đúng theo từng kiểu gõ (Telex/VNI/Nôm dùng icon riêng như tray Windows/Linux; keyboard đóng gói dùng `metadata.icon` nếu có, không thì icon custom) và không còn bị phóng to/méo — build sinh TIFF 16pt (16px + 32px @2x) thay vì dùng thẳng PNG 128px gắn ~23 dpi (≈390pt)
- engine: học thông minh giờ hoạt động trên MỌI backend — TSF, IBus, Wayland-native — không chỉ Windows Hook. Composition mode thu thập tín hiệu ở thời điểm chốt từ (`ConfirmComposition`/flush); learning.toml giờ an toàn với nhiều tiến trình ghi cùng lúc qua merge-on-write (union additive, không clobber; file hỏng thì từ chối ghi thay vì ghi đè bằng rỗng). Toggle "Học thông minh" áp dụng ngay không cần restart trên mọi backend; engine Linux + mỗi ứng dụng TSF có thread ghi riêng ngoài đường phím (xem [`ADR-0001`](docs/adr/0001-learning-additive-only-record-replay-invariant.md) để hiểu định nghĩa).
- linux: sở hữu kiểu gõ per-platform (ADR-0003 phương án C) — trên IBus/fcitx5(có addon)/macOS, menu của OS là nơi chọn kiểu gõ, buttre không dựng tray, `buttre` lệnh không tham số mở cửa sổ Cấu hình, thay vào đó hiển thị cửa sổ Cấu hình khi chạy không tham số. Wayland-native và Windows vẫn dựng tray. Flag `--autostart` phân biệt login launch với click: trên nền tảng OS-owned nó thoát im lặng, trên Buttre-owned nó khởi động tray. Engine Linux tự áp dụng `Settings::enabled` khi cấu hình được chỉnh sửa bằng tay hoặc bởi tray (file-watch + `macro_sync.rs`). Radio English ở menu IBus thành lệnh toggle `Settings::enabled` (qua `command_state`). `--doctor` in owner + surface để giúp chẩn đoán. Cửa sổ Cấu hình bỏ ô chọn kiểu gõ trên nền tảng OS-owned, ẩn autostart checkbox trên nền tảng không có tray
- windows: sửa Ctrl+Shift+Z không ăn trên backend Hook — nguyên nhân không phải xung đột phím tắt với app mà là phím do buttre tự inject bị chính modifier người dùng đang giữ biến thành shortcut. Hotkey được poll từ vòng lặp tray mỗi 50 ms nên khi inject diễn ra, Ctrl và Shift vẫn đang bị giữ vật lý; app nhận `Ctrl+Shift+Backspace` (xoá cả từ trước trong Word và VS Code, âm thầm mất chữ thay vì thay chữ) và phần Unicode bị đẩy sang accelerator thay vì chèn vào văn bản. Đây là đường inject duy nhất trong hook có modifier đang giữ. Cách sửa: gộp **nhả modifier + payload + nhấn lại** vào **một lần `SendInput` duy nhất**. Windows bảo đảm các event trong cùng một lần gọi không bị chen ngang bởi input khác (kể cả phím thật của người dùng), nên trạng thái bàn phím không thể đổi giữa batch và bước nhấn lại không cần đọc lại trạng thái phím — điều bắt buộc, vì `SendInput` cập nhật luôn async key state nên sau khi tự nhả Ctrl thì không còn cách nào biết người dùng còn giữ hay không. Tách nhả và nhấn lại thành hai lần gọi (thử rồi, đã bỏ) khiến bước nhấn lại đọc chính cái nhả của mình và không nhấn lại gì cả, để hệ thống tưởng người dùng đã thả tay và **mọi lần nhấn lặp lại của chord bị nuốt** (lần đầu ăn, lần hai thành `z` trơn). Đảo có tác dụng ngay lúc nhấn, không phải chờ thả phím. Đổi chord khác không giải quyết được, miễn là chord còn chứa Ctrl hoặc Alt

- windows: Ctrl+Shift+Z (đảo từ vừa gõ giữa nguyên văn/bỏ dấu) chạy được trên backend TSF — trước đây chỉ nối vào backend Hook, mà Windows lại chạy TSF trước nên với hầu hết người dùng nút này không làm gì cả. Ba lớp chặn đều được mở: TSF không đăng ký hotkey toàn cục nữa (RegisterHotKey giữ phím lại, không cho tới app đang focus) mà tự bắt chord trong tiến trình app; `should_ignore` cho Ctrl+Shift+Z đi qua; và `Keyboard::toggle_composition` mới xử lý được chế độ composition vốn không có cửa sổ multiword. Đảo hai chiều, lặp được, giữ nguyên chữ hoa, và lựa chọn nguyên văn theo tới lúc chốt từ (nhấn dấu cách ra đúng phím thô, không phải dạng bỏ dấu). Phạm vi: chỉ từ đang gõ dở — từ đã chốt vào tài liệu thì ngoài tầm với. Khi không có gì để đảo, phím rơi xuống cho app xử lý nên Ctrl+Shift+Z "redo" của editor vẫn hoạt động

- engine: bỏ dấu tự do (gõ hết chữ rồi mới bỏ dấu, kiểu Unikey) chạy được với nhóm từ `đ` âm tiết mở trong Telex — `dads`→`đá`, `did`→`đi`, `duadf`→`đùa`, `daodj`→`đạo`. Trước đây dấu `đ` chỉ nổ khi còn nguyên âm phía sau chữ `d` thứ hai, nên 20/44 từ `đ` thông dụng không gõ được theo thứ tự này (VNI không dính vì phím `đ` là chữ số 9). Đánh đổi: `dad`/`dads` giờ ra `đa`/`đá` — trùng nghĩa không tách được bằng từ điển, lối thoát là double-key undo như `reset`→`rết`; đo trên corpus 9.858 từ tiếng Anh chỉ thêm 4 từ cần escape, không từ nào mất khả năng gõ
- engine: VNI bỏ dấu tự do không còn phụ thuộc thứ tự phím — dấu suy ra không liền kề chỉ cần hợp lệ về cấu trúc thay vì phải có trong từ điển 7.889 âm tiết (`dech91`→`đéch` như `d9ech1`). Phím chữ số không thể nằm trong từ tiếng Anh nên cổng từ vựng ở nhánh này không bảo vệ được gì, chỉ chặn nhầm tiếng lóng/tên riêng/phương ngữ
- engine: gõ tắt `đt`/`đc`/`đhqg` với dấu `9` đặt cuối (`dt9`) không còn bị hoàn nguyên về phím thô khi nhấn dấu cách — chốt biên từ giờ miễn trừ dạng viết tắt `đ`, vốn không bao giờ có dấu thanh để thỏa điều kiện cũ
- test: golden corpus thêm nhãn `FreeToneMarking` (+193 case mỗi kiểu gõ) và test bất biến "thứ tự phím không đổi kết quả"; bộ dữ liệu `buttre-test` thêm 353 case bỏ dấu tự do

## [0.7.11-beta] — 2026-07-25

- linux: tải custom keyboard
- linux: gõ được trên Wayland/fcitx5
- linux: đồng bộ kiểu gõ giữa tray/cửa sổ Cấu hình và radio trên menu IBus
- linux: tùy chọn bật/tắt gạch chân với các app hỗ trợ

## [0.7.10-beta] — 2026-07-21

- linux: sửa tray app crash ngay khi mở

## [0.7.9-beta] — 2026-07-20

- engine: gõ tắt kiểu "đt"/"đc"/"đkkd" chạy được ngay không cần tự định nghĩa
- windows: sửa TSF nuốt ký tự phân giới ở cuối composition (`xin.` ra `xin`, dấu `.` biến mất) — `VietnameseEngine::process_key` trước đây chỉ lấy action đầu tiên trong `Vec<Action>`, bỏ luôn `Commit(dấu phân giới)` đi kèm `ConfirmComposition`; giờ áp dụng đủ toàn bộ vector. Đồng thời vá race ẩn do fix này mở ra: `write_text` gộp các lần ghi liên tiếp qua `pending_edit` để tối ưu, nhưng nếu session ghi từ vừa xác nhận chưa kịp chạy thì lần ghi dấu phân giới kế tiếp sẽ ghi đè lên nó thay vì tạo session riêng — mất luôn cả từ. `end_composition` giờ luôn làm mới `pending_edit` trước khi trả về (đóng #4)
- engine: thêm gõ tắt (macros.toml)
- tray: thêm "Tùy chọn → Gõ tắt" bật/tắt gõ tắt ngay lập tức + menu "Quản lý gõ tắt" mở macros.toml để xem/tự sửa
- core: bất biến record-replay cho học cá nhân hóa - toggle literal trên raw dạng undo xóa pref cũ thay vì ghi pref không thể replay
- test: guard hai-phiên chống lớp bug "học xong thì gõ khác đi" — gõ 10k từ Anh (double-key escape) + toàn bộ corpus telex qua 3 phiên chung learning store, chạy trong CI
- tray: thêm "Tùy chọn → Học thông minh" bật/tắt học cá nhân hóa ngay lập tức
- tray: đăng ký/hủy autostart theo OS (Windows Run key, Linux XDG autostart)
- tray: thêm menu "Từ đã học" mở learning.toml để xem/tự sửa; file có hướng dẫn ngay đầu
- tool: `buttre-test explain <method> <raw> [--learning]` — chẩn đoán từng tầng (pref/overlay/undo/gate), nạp learning.toml thật
- engine: sửa "yes" không gõ được trên máy đã bật learning — bỏ tự ghi pref literal từ double-tap undo
- engine: dấu thanh trên vần khuyết "ye"/"ie" đặt vào e — "yes" ra "yé" như Unikey, không còn "ýe"
- engine: sửa lớp từ tiếng Anh không gõ được (rows, towns, owns, lows) — cặp hủy transform (ww) không còn bị hồi sinh khi có phím dấu theo sau
- engine: undo dấu không còn nuốt phím dấu bị ghi đè trước đó ("meterss" ra "meters", không phải "metes")
- test: guard 10.000 từ tiếng Anh thông dụng — mọi từ phải gõ được bằng gõ thẳng hoặc double-key escape, chạy trong CI chống tái phát
- engine: sửa "quo7t1"/"quowts" không ra "quớt" — quy tắc ghép uo→ươ không còn chiếm chữ u thuộc phụ âm đầu "qu"

## [0.7.7-beta] — 2026-07-07

- engine: undo giữa từ giờ là chung cuộc đến hết từ (chuẩn Unikey)
- linux: engine đăng ký đúng với ibus-daemon (private bus + Factory)
- linux: sửa semantics preedit — composition dựng dần có gạch chân, commit đúng ở dấu cách/dấu câu
- linux: thêm backend Wayland-native cho sway/Hyprland/KDE
- linux: đổi kiểu gõ Telex/VNI/Nôm từ tray áp dụng ngay vào engine đang chạy, không cần restart
- linux: ô mật khẩu bypass engine; không tap phím toàn cục ở cả hai backend
- macos: FFI v2 (`ButtreKeyResult`) map thẳng vào IMKit; semantics composition dùng chung với Linux; kèm header C
- ci: job integration Linux chạy ibus-daemon thật + headless sway để chống hồi quy

## [0.7.6-beta] — 2026-07-04

- engine: hỗ trợ gõ w thay ư sau phụ âm đầu
- engine: w đầu từ giữ literal cho từ tiếng Anh
- engine: kết quả w không attested tự trả về literal
- engine: sửa mất một chữ s cuối từ tiếng Anh
- engine: sửa từ VNI có dấu bị trả raw khi commit
- engine: thêm từ lóng thông dụng vào từ điển chứng thực
- engine: compose nhanh gấp 5 lần nhờ bỏ allocation
- engine: pipeline executor giảm còn 592 ns mỗi phím
- engine: opt-level 3 riêng cho crate engine
- bench: thêm benchmark so sánh với engine tham chiếu
- bench: đo steady-state, sửa số liệu bị thổi phồng

## [0.7.5-beta] — 2026-07-03

- hook: xử lý lỗi gõ trong Chrome bị lặp ký tự
- hook: UIA lỗi hoặc timeout tự rơi về đường backspace cũ
- hook: passthrough tự nhiên cho ký tự commit không biến đổi
- hook: passthrough giúp app nhận scancode thật, tốt cho game, terminal, RDP
- tsf: thêm chế độ phục hồi khi app tự kết thúc composition giữa chừng
- tsf: ghi đè tại chỗ thay vì chèn ký tự sau phần đã commit
- tsf: đếm ký tự thay vì byte cho previous_length và last_text_len
- tsf: không ghi đè previous_length khi tái dùng edit-session đang chờ
- engine: hoàn thiện kiến trúc event-sourcing, nguyên tắc raw keystroke là event log
- engine: thay temp_english_mode latch một chiều bằng tái suy diễn mỗi phím
- engine: sửa lỗi dấu thanh đến sau transform, ví dụ vietj + e thành việt
- engine: double-tap undo vẫn literal, data-class không nhấp nháy
- engine: thêm sửa lỗi cuối cùng tại ranh giới từ
- engine: mark suy luận không liền kề mà dấu thanh chưa đến, phục hồi về literal
- engine: áp dụng đồng nhất cho Hook multiword và TSF ConfirmComposition
- hook: thêm phím tắt Ctrl+Shift+Z đảo literal và composed cho từ đang gõ
- hook: thêm chord-exemption giữ Ctrl, Shift không reset engine
- hook: thêm focus-guard, alt-tab trước khi bấm hotkey sẽ không làm gì
- core: thêm cài đặt backspace_mode raw, xóa theo phím thô
- core: thêm học cá nhân hóa lưu vào learning.toml
- core: tắt được qua cài đặt learning_enabled
- core: âm tiết gõ đủ ba lần riêng biệt sẽ tự chứng thực cho người dùng
- core: hành động chủ ý như double-tap undo, toggle cũng được ghi nhớ ưu tiên
- engine: mở rộng coda k cho lớp địa danh như đắk, lắk
- engine: làm chặt lớp trigger của cổng chứng thực
- engine: đóng băng số field bool trên TypingContext bằng test purity_audit
- test: thêm test tương tác xuyên phase cho un-latch, boundary-repair, toggle
- test: regen golden snapshot thêm chín âm tiết lớp coda-k
- engine: nhúng bảng 7884 âm tiết thật từ từ điển ibus-bamboo
- engine: mark suy luận không liền kề chỉ giữ khi âm tiết có thật
- engine: áp dụng cổng chứng thực cho cả Telex và VNI
- engine: thêm hoàn tác không liền kề khi gõ lại đúng phím trigger
- engine: chấp nhận va chạm âm tiết thật theo thiết kế, không phân biệt được tiếng Anh
- test: mở rộng golden snapshot Telex/VNI với các từ tiếng Anh mới
- hook: sửa lỗi record_output_hwnd bỏ sót ở nhánh passthrough
- tsf: sửa rò rỉ COM reference trên đường phục hồi khi lỗi
- hook: bỏ qua Shift đồng bộ khi người dùng đang giữ phím Shift

## [0.7.4-beta] — 2026-06-19

- hook: sửa lỗi rớt phím khi gõ nhanh
- hook: try_write bỏ qua phím khi tranh chấp lock gây lệch buffer
- hook: đổi sang write blocking, chịu poison, không bao giờ bỏ phím
- hook: sửa lỗi nhảy ngược lên dòng trên khi nhấn Enter rồi gõ tiếp
- hook: đổi sang reset blocking, ép KEYBOARD_DIRTY để luôn reset đúng ranh giới
- hook: backspace nhận biết grapheme, xóa đúng một ký tự hiển thị
- hook: vẫn cho phép sửa lại từ đang gõ thay vì reset sạch
- hook: thêm cửa sổ nhiều từ kiểu Unikey, backspace xuyên dấu cách
- hook: sửa được một đến hai từ trước đó qua dấu cách
- hook: giữ ba từ gần nhất trong window, từ cũ hơn đóng băng
- hook: hard-reset khi Enter, mũi tên, hoặc chuột để tránh lệch con trỏ
- hook: chỉ áp dụng cho backend hook Telex/VNI, TSF và Nôm giữ đường cũ
- engine: chặn O(n bình phương) bằng giới hạn độ dài âm tiết recompute
- engine: sửa lỗi bỏ dấu khi phụ âm đầu trùng phím thanh Telex
- engine: dùng trailing-run detection đúng theo Unikey và OpenKey
- engine: sửa fallback tiếng Anh cho nguyên âm lặp xuyên ranh giới phụ âm
- engine: chỉ bắn luật non-adjacent khi phần trước là âm tiết Việt hoàn chỉnh
- engine: bỏ luật w thành ư ở đầu từ
- engine: nâng cấp bảng âm vị, port từ Unikey VSeqList và VCPairList
- engine: bổ sung đầy đủ nuclei và ràng buộc nucleus-coda
- engine: sửa lỗi từ chối nhầm iê cộng p hoặc c
- engine: English fallback kiểm tra hợp lệ trước khi trả kết quả
- engine: âm tiết không hợp lệ trả về literal và bật chế độ tiếng Anh
- engine: sửa VNI gõ dấu thanh trước transform, ví dụ huyen26 thành huyền
- engine: thêm dạng bare uye cho nguyen64 và quyen26
- engine: hỗ trợ đ không liền kề khi d cuối tạo thành đ
- engine: chỉ bắn khi âm tiết có coda hoặc dấu thanh, giữ dad là tiếng Anh
- engine: z xóa dấu theo chuẩn Telex
- engine: z và dz đầu từ làm phụ âm cho văn phong informal
- engine: giữ âm tiết hợp lệ khi kéo dài ký tự trong chat, văn chương
- engine: nối đuôi lặp literal thay vì fallback cả từ

## [0.7.1-beta] — 2026-06-14

- engine: tái cấu trúc recompute từ 12 xuống 7 giai đoạn
- engine: gộp bảng dấu thanh và logic vị trí vào tone module
- engine: pipeline config-driven cho Telex, VNI, VIQR và Nôm
- engine: segment mode và validator chọn qua config, không hardcode
- engine: English fallback dùng validation trước khi chấp nhận kết quả
- engine: undo giữ nguyên transform đã áp dụng
- engine: hiệu năng khoảng 250ns đến 8 micro giây mỗi lần gõ
- installer: sửa lỗi cài đặt Windows TSF, macOS FFI, Linux IBus

## [0.6.2-alpha] — 2026-01-13

- engine: sửa lỗi bỏ digit kiểu H2O trong nhập alphanumeric
- engine: cải thiện giữ nguyên literal-mark

## [0.6.1-alpha] — 2026-01-10

- hook: sửa lỗi desync backspace xuyên từ
- hook: mở rộng phát hiện separator

## [0.6.0-alpha] — 2026-01-05

- engine: mốc kiến trúc core, pipeline 12 giai đoạn
- engine: thêm PGO, khoảng 1 micro giây mỗi lần gõ
- engine: thêm gõ linh hoạt bằng permutation
- engine: thêm đồng bộ xuyên từ
- platform: thêm backend hybrid Hook cộng TSF
- engine: thêm retrofix và undo

## [0.2.0-alpha] — 2025-12-27

- engine: tối ưu hiệu năng VNI bằng bảng dấu thanh tính sẵn
- engine: thêm phát hiện range-based
- engine: thêm PGO cho engine core

## [0.1.0-alpha] — 2025-12-19

- engine: hỗ trợ phương thức Telex, VNI, Nôm
- platform: hỗ trợ Windows Hook và TSF, Linux IBus, macOS
- engine: có English fallback, raw mode, tone toggle, undo
