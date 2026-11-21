# buttre - Bộ Gõ Tiếng Việt

[![License: GPL 3.0](https://img.shields.io/badge/License-GPL_3.0-brightgreen.svg)](https://www.gnu.org/licenses/gpl-3.0)
[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange.svg)](https://www.rust-lang.org/)

> _Được tạo ra bởi một nhóm dev yêu thích Rust và low-level programming._

---

## ✨ Tại Sao Lại Có buttre?

Thật ra, thế giới này không thiếu bộ gõ tiếng Việt. Unikey là huyền thoại, OpenKey cũng rất tuyệt. Nhưng chúng tôi có một niềm đam mê riêng: **tạo ra một công cụ hoàn toàn mới, được viết bằng Rust, tối ưu từng dòng code, và thuộc về cộng đồng.**

buttre không phải là sản phẩm thương mại. Đây là **dự án tình yêu** - được sinh ra từ những đêm cuối tuần, những buổi brainstorm say sưa, và niềm đam mê với systems programming.

Chúng tôi code buttre vì:
- **Đam mê thuần túy** với Rust và low-level programming
- **Thích thú** với việc tối ưu hiệu năng đến từng byte
- **Tự do sáng tạo** không bị ràng buộc bởi lợi nhuận hay deadline

---

## 🎯 Điểm Đặc Biệt

### 1. ⚡ Hiệu Năng Cực Đỉnh
- **Viết bằng Rust thuần túy**: Không dependency rườm rà, không runtime nặng nề
- **Khởi động tức thì**: Nhẹ hơn, nhanh hơn các giải pháp hiện có
- **Zero-allocation hot paths**: Tối ưu bộ nhớ ở mức độ cao nhất
- **Tiết kiệm tài nguyên**: Chạy êm ái ngay cả trên máy cấu hình khiêm tốn

### 2. 🧠 Thuật Toán Thông Minh
- **Logic xử lý "W" thế hệ mới**: Phân biệt chính xác `ư`, `uw`, `ươ` trong mọi ngữ cảnh
- **Tuân thủ chuẩn chính tả Việt Nam**: Ưu tiên các quy tắc chính thống (`hòa` thay vì `hoà`)
- **Xử lý edge cases**: Được test kỹ lưỡng với hàng nghìn trường hợp đặc biệt

### 3. 🔒 Bảo Mật & Riêng Tư Tuyệt Đối
- **Zero telemetry**: Không thu thập, không gửi bất kỳ dữ liệu nào về server
- **Không keylogging**: Mọi thao tác đều xử lý local
- **Quyền admin chỉ lúc cài**: Đăng ký bộ gõ với Windows (MSI) cần Admin một lần; khi gõ, buttre chạy với quyền user thông thường
- **Open source hoàn toàn**: Bạn có thể audit từng dòng code

### 4. 🎨 Thiết Kế Tối Giản
- **Giao diện tối giản**: Windows — mỗi kiểu gõ là một bàn phím Windows, trạng thái hiện ở icon nhỏ cạnh chữ ENG (không tray); bản Hook có icon tray. Xem [QUICK-START.md](QUICK-START.md)
- **Không quảng cáo, không spam**: Tập trung 100% vào trải nghiệm gõ
- **Không phiền nhiễu**: Không có popup, notification không cần thiết

### 5. 🌏 Hỗ Trợ Đa Dạng (Roadmap)
Chúng tôi đang phát triển hỗ trợ cho:
- Chữ Nôm
- Chữ dân tộc Tây Nguyên
- Chữ Thái
- Chữ Chăm

_Timeline phụ thuộc vào thời gian rảnh của team, nhưng chúng tôi cam kết sẽ thực hiện. Hoặc bạn có thể contribute cùng nhé!_

---

## 📦 Cài Đặt

### Người Dùng Thông Thường
1. Truy cập [Releases](https://github.com/dxsl-org/buttre/releases)
2. Tải bản build mới nhất
3. Chạy installer và tận hưởng

### Developers
```bash
git clone https://github.com/dxsl-org/buttre.git
cd buttre
cargo build --release
```

_Lưu ý: Đảm bảo bạn đã cài Rust toolchain và các dependencies cần thiết._

---

## 🏗️ Kiến Trúc Kỹ Thuật

buttre được thiết kế theo kiến trúc modular, dễ bảo trì và mở rộng:

- **`buttre-core`**: Keyboard configuration (Telex, VNI, Nôm), services, events, platform-agnostic
- **`buttre-engine`**: Core recompute pipeline (compose, tone, validation) - pure Vietnamese/Nôm processing
- **`buttre-platform`**: OS integration (Windows TSF + hook, Linux ibus, macOS FFI, system tray/UI)
- **`buttre-test`**: Shared test utilities and fixtures

---

## 🚀 Trạng Thái Dự Án

**Version hiện tại**: `0.7.0-beta` (Open Beta)

buttre đang trong giai đoạn beta, có nghĩa là:
- ✅ Các tính năng cơ bản hoạt động ổn định
- ⚠️ Có thể còn một số edge cases chưa được xử lý
- 🐛 Nếu bạn phát hiện bug, rất mong nhận được feedback lịch sự qua [Issues](https://github.com/dxsl-org/buttre/issues)

Chúng tôi đánh giá cao mọi đóng góp mang tính xây dựng!

---

## 🤝 Đóng Góp

buttre là dự án mã nguồn mở và chúng tôi rất hoan nghênh mọi đóng góp!

### Quy Tắc Đóng Góp
- **Code quality**: Code sạch, dễ đọc, tuân thủ Rust best practices
- **Testing**: Mọi thay đổi cần có tests tương ứng
- **Formatting**: Sử dụng `rustfmt` để format code
- **CI/CD**: Đảm bảo tất cả tests pass trước khi submit PR

### Cách Thức
1. Fork repository
2. Tạo feature branch (`git checkout -b feature/AmazingFeature`)
3. Commit changes (`git commit -m 'Add some AmazingFeature'`)
4. Push to branch (`git push origin feature/AmazingFeature`)
5. Mở Pull Request

---

## 💝 Hỗ Trợ Dự Án

buttre miễn phí hoàn toàn và sẽ luôn như vậy. Nhưng nếu buttre hữu ích với bạn và bạn muốn giúp dự án tiếp tục phát triển, hãy mua cho chúng tôi vài lý cà phê:

<div align="center">


[![Ko-fi](https://img.shields.io/badge/Ko--fi-Donate-%23FF5E5B?logo=ko-fi)](https://ko-fi.com/dxsl_org)

</div>

### Các Cách Hỗ Trợ Khác
- ⭐ **Star repo**: Giúp buttre tiếp cận nhiều người dùng hơn
- 🐛 **Báo bug**: Giúp buttre ngày càng hoàn thiện qua [Issues](https://github.com/dxsl-org/buttre/issues)
- 📝 **Contribute**: Code, tài liệu, bản dịch — đều quý như nhau
- 📣 **Giới thiệu**: Chia sẻ với bạn bè, đồng nghiệp

### Dịch Vụ Thương Mại

buttre là phần mềm tự do, nhưng nếu tổ chức của bạn cần:
- **Đào tạo, Hỗ trợ kỹ thuật** (tích hợp, cài đặt hàng loạt, troubleshooting,...)
- **Tùy chỉnh** theo yêu cầu riêng (bộ từ điển chuyên ngành, layout đặc biệt,...)
- **SLA và cam kết** phản hồi cho môi trường doanh nghiệp

Liên hệ: **service@dxsl.org** hoặc **dichvu@dxsl.org**

### Triết Lý
buttre là **passion project** thuộc về cộng đồng — không bị ràng buộc bởi lợi nhuận hay nhà đầu tư. Quyết định về product direction thuộc về core team và cộng đồng contributor. Chúng tôi lắng nghe feedback nhưng code theo vision riêng.

---

## 📜 Giấy Phép

**buttre là phần mềm tự do vĩnh viễn — miễn phí cho mọi cá nhân, tổ chức và chính phủ, mãi mãi.**

Được phát hành theo **GNU General Public License v3.0 (GPL-3.0)**.

Bạn tự do:
- ✅ Sử dụng không giới hạn — cá nhân, doanh nghiệp, cơ quan nhà nước, không cần xin phép
- ✅ Nghiên cứu và chỉnh sửa source code
- ✅ Phân phối lại bản gốc hoặc bản đã chỉnh sửa

Điều kiện khi phân phối:
- 📝 Giữ nguyên thông báo bản quyền và license
- 📝 Phân phối cùng source code (hoặc cung cấp link tải)
- 📝 Các bản fork/derivative phải giữ nguyên GPL-3.0 — không được đóng lại thành proprietary

Chi tiết: [LICENSE](LICENSE) · [GNU GPL v3](https://www.gnu.org/licenses/gpl-3.0)

---

## 🙏 Lời Cảm Ơn

Cảm ơn bạn đã quan tâm đến buttre!

Dự án này là minh chứng cho niềm tin rằng: **Code tốt được sinh ra từ đam mê thuần túy, không phải từ lợi nhuận.**

Nếu bạn chia sẻ niềm đam mê này, hãy cùng chúng tôi xây dựng buttre ngày càng tốt hơn!

---

<div align="center">

**Crafted with ❤️, ☕, and countless hours of debugging**

_by DXSL Team_
</div>
