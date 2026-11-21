# Tài Liệu buttre

> Tài liệu đầy đủ dành cho developer và contributor

**Cập nhật lần cuối**: 2026-06-14

---

## Bắt Đầu Nhanh Cho Developer

**Mới với buttre?** Đọc theo thứ tự sau:

1. **[../README.md](../README.md)** — Tổng quan dự án và hướng dẫn cài đặt
2. **[01-architecture.md](01-architecture.md)** — Kiến trúc hệ thống và thiết kế
3. **[02-coding-guide.md](02-coding-guide.md)** — Cách viết code trong dự án này
4. **[ROADMAP.md](ROADMAP.md)** — Lộ trình phát triển và kế hoạch tương lai

---

## Cấu Trúc Tài Liệu

```
docs/
├── README.md                      # File này
├── 00-context.md                  # Bối cảnh hệ thống & quy tắc thiết kế
├── 01-architecture.md             # ⭐ Kiến trúc hệ thống (toàn diện)
├── 02-coding-guide.md             # ⭐ Tiêu chuẩn code và các pattern
├── ROADMAP.md                     # ⭐ Lộ trình dự án và mốc thời gian
├── PIPELINE_ARCHITECTURE.md       # Tài liệu chi tiết pipeline 7 giai đoạn
├── VIETNAMESE_ACCENT.md           # Đặc tả chính tả tiếng Việt
├── MANUAL_TESTING_GUIDE.md        # Hướng dẫn kiểm thử thủ công TSF DLL
├── FFI_SAFETY_GUIDE.md            # Các pattern FFI an toàn (macOS/Linux)
├── linux-coverage-test-matrix.md  # Linux: độ phủ gói cài theo distro, ma trận máy ảo test
└── journals/                      # Nhật ký phát triển
```

---

## Tài Liệu Cốt Lõi

### [00-context.md](00-context.md)

**Nội dung**: Bối cảnh hệ thống và quy tắc thiết kế cho tất cả contributor (bao gồm AI agent)

**Bao gồm**:
- Metadata dự án (tên, phiên bản, nền tảng, tech stack)
- Cấu trúc crate và trách nhiệm từng crate
- Quy tắc chất lượng code (xử lý lỗi, unsafe code, type safety)
- Yêu cầu kiểm thử
- Quy trình làm việc cho AI agent
- Quy tắc nhập liệu tiếng Việt (vị trí dấu thanh, tự động sửa)
- Tổng quan kiến trúc pipeline
- Lệnh build
- Các implementation tham chiếu
- Quy tắc code Rust (bắt buộc)

**Khi nào nên đọc**:
- ⭐ **TRƯỚC KHI đóng góp bất kỳ code nào**
- Khi cài đặt môi trường phát triển
- Khi giới thiệu thành viên mới

---

### [01-architecture.md](01-architecture.md)

**Nội dung**: Tổng quan kiến trúc đầy đủ của buttre

**Bao gồm**:
- Tổng quan hệ thống và kiến trúc cấp cao
- Cấu trúc crate (buttre-engine, buttre-core, buttre-platform, buttre-test)
- Kiến trúc pipeline xử lý 7 giai đoạn
- Quản lý state và luồng dữ liệu
- Tích hợp platform (Windows TSF, macOS, Linux)
- Các nguyên tắc thiết kế

**Khi nào nên đọc**:
- ⭐ **ĐỌC TRƯỚC TIÊN** — Trước khi đóng góp code
- Khi cần hiểu bức tranh tổng thể
- Khi lên kế hoạch cho tính năng mới

---

### [02-coding-guide.md](02-coding-guide.md)

**Nội dung**: Tiêu chuẩn code và các pattern được trích xuất từ codebase thực tế

**Bao gồm**:
- Cài đặt dự án và cấu trúc workspace
- Tiêu chuẩn code Rust (xử lý lỗi, tài liệu, đặt tên)
- Các pattern thường dùng (Pipeline Stage, Action Enum, Configuration)
- Hướng dẫn kiểm thử (unit test, integration test)
- Best practice xử lý lỗi
- Hướng dẫn hiệu năng
- Cách thêm tính năng mới (từng bước)

**Khi nào nên đọc**:
- ⭐ **TRƯỚC KHI** viết code
- Khi không chắc về coding style
- Trước khi submit PR

---

### [ROADMAP.md](ROADMAP.md)

**Nội dung**: Kế hoạch chiến lược cho sự phát triển của buttre

**Bao gồm**:
- Trạng thái hiện tại và các tính năng đã hoàn thành
- Lộ trình theo từng phase (Q1–Q4 2026, 2027)
- Ưu tiên theo nền tảng (Windows, macOS, Linux)
- Kế hoạch hỗ trợ chữ Hán Nôm
- Các tính năng nâng cao đang được cân nhắc
- Nợ kỹ thuật và vấn đề đã biết
- Mốc thời gian và các deliverable

**Khi nào nên đọc**:
- Khi muốn đóng góp (tìm xem có gì được lên kế hoạch)
- Khi đề xuất tính năng mới
- Để hiểu định hướng dự án

---

## Tài Liệu Chuyên Biệt

### [PIPELINE_ARCHITECTURE.md](PIPELINE_ARCHITECTURE.md)

**Nội dung**: Tài liệu chi tiết về pipeline xử lý 7 giai đoạn

**Bao gồm**:
- Mô tả từng giai đoạn
- Điều khiển luồng và cây quyết định
- Quản lý state trong TypingContext
- Tối ưu hóa hiệu năng
- Ví dụ thực tế (gõ "người")

**Khi nào nên đọc**:
- Khi làm việc trên engine (buttre-engine)
- Khi debug quá trình xử lý nhập liệu
- Khi thêm giai đoạn mới

---

### [VIETNAMESE_ACCENT.md](VIETNAMESE_ACCENT.md)

**Nội dung**: Đặc tả chính tả tiếng Việt

**Bao gồm**:
- Phase 1: Biến đổi ký tự (mũ, râu, trăng)
- Phase 2: Parser & chuẩn hóa (âm đầu, nhân nguyên âm, âm cuối)
- Phase 3: Logic vị trí dấu thanh (quy tắc đặt dấu)
- Quy tắc ưu tiên cho vị trí dấu thanh
- Test case

**Khi nào nên đọc**:
- Khi làm việc trên logic nhập liệu tiếng Việt
- Khi sửa lỗi vị trí dấu thanh
- Khi kiểm tra quy tắc chính tả

---

### [MANUAL_TESTING_GUIDE.md](MANUAL_TESTING_GUIDE.md)

**Nội dung**: Hướng dẫn kiểm thử thủ công Windows TSF DLL

**Bao gồm**:
- Vị trí file build output
- Lệnh đăng ký
- Kiểm thử trong Notepad/Word/trình duyệt
- Các vấn đề thường gặp và cách giải quyết

**Khi nào nên đọc**:
- Khi kiểm thử thay đổi Windows TSF
- Khi debug tích hợp TSF
- Trước khi phát hành bản Windows

---

### [FFI_SAFETY_GUIDE.md](FFI_SAFETY_GUIDE.md)

**Nội dung**: Các pattern FFI an toàn cho tích hợp platform

**Bao gồm**:
- Đạt được zero unsafe trong FFI
- Các pattern FFI Objective-C ↔ Rust (cho macOS)
- Sử dụng windows-rs một cách an toàn
- Best practice cho platform binding

**Khi nào nên đọc**:
- Khi làm việc trên tích hợp platform macOS/Linux
- Khi thêm unsafe code
- Khi review FFI code

---

## Bảo Trì Tài Liệu

### Khi Nào Cần Cập Nhật Tài Liệu

**01-architecture.md**: Cập nhật khi:
- Thêm crate mới
- Thay đổi trách nhiệm của crate
- Sửa đổi kiến trúc pipeline
- Thêm nền tảng mới

**02-coding-guide.md**: Cập nhật khi:
- Thiết lập pattern code mới
- Thay đổi quy ước đặt tên
- Thêm hướng dẫn kiểm thử mới
- Phát hiện anti-pattern

**ROADMAP.md**: Cập nhật khi:
- Hoàn thành các phase
- Điều chỉnh mốc thời gian
- Thêm/xóa tính năng
- Thay đổi ưu tiên nền tảng

**PIPELINE_ARCHITECTURE.md**: Cập nhật khi:
- Thêm/xóa giai đoạn
- Thay đổi trách nhiệm giai đoạn
- Sửa đổi điều khiển luồng

**VIETNAMESE_ACCENT.md**: Cập nhật khi:
- Sửa lỗi chính tả
- Thêm quy tắc mới
- Làm rõ đặc tả

### Tiêu Chuẩn Tài Liệu

**Định dạng**: Tất cả tài liệu dùng GitHub-flavored Markdown

**Phong cách**:
- Ngôn ngữ rõ ràng, súc tích
- Bao gồm ví dụ code từ codebase thực tế
- Cung cấp hướng dẫn "Khi nào nên đọc"
- Giữ tài liệu cập nhật theo code

**Đặt tên file**:
- Dùng `UPPERCASE_WITH_UNDERSCORES.md` cho tài liệu chính
- Dùng `lowercase-with-hyphens.md` cho thư mục con

**Cấu trúc**:
- Bắt đầu với mô tả ngắn gọn
- Thêm mục lục cho tài liệu dài
- Dùng heading để điều hướng
- Thêm ngày "Cập nhật lần cuối"

---

## Đóng Góp Tài Liệu

Đóng góp cải thiện tài liệu rất được trân trọng!

**Cách đóng góp**:

1. **Sửa lỗi chính tả/nội dung**: Submit PR trực tiếp
2. **Làm rõ tài liệu hiện có**: Submit PR kèm giải thích
3. **Thêm phần mới**: Thảo luận qua issue trước, sau đó PR
4. **Thêm tài liệu mới**: Thảo luận qua issue trước (tránh trùng lặp)

**Đóng góp tài liệu tốt**:
- Sửa thông tin lỗi thời
- Thêm ví dụ còn thiếu
- Làm rõ các phần khó hiểu
- Thêm sơ đồ/hình ảnh minh họa
- Cải thiện điều hướng
- Thêm hướng dẫn "khi nào nên đọc"

**Checklist PR tài liệu**:
- [ ] Thông tin chính xác (đã kiểm tra với code)
- [ ] Ví dụ lấy từ codebase thực tế
- [ ] Định dạng nhất quán
- [ ] Các liên kết hoạt động
- [ ] Ngày "Cập nhật lần cuối" là ngày hiện tại
- [ ] Không có lỗi chính tả/ngữ pháp

---

## Bảng Tra Cứu Nhanh

| Nhiệm vụ | Đọc file này |
|----------|-------------|
| Tôi mới và muốn hiểu bối cảnh | [00-context.md](00-context.md) |
| Tôi muốn hiểu kiến trúc của buttre | [01-architecture.md](01-architecture.md) |
| Tôi muốn viết code cho buttre | [02-coding-guide.md](02-coding-guide.md) |
| Tôi muốn đóng góp tính năng | [ROADMAP.md](ROADMAP.md) |
| Tôi đang làm việc trên engine | [PIPELINE_ARCHITECTURE.md](PIPELINE_ARCHITECTURE.md) |
| Tôi đang sửa lỗi vị trí dấu thanh | [VIETNAMESE_ACCENT.md](VIETNAMESE_ACCENT.md) |
| Tôi đang kiểm thử Windows TSF | [MANUAL_TESTING_GUIDE.md](MANUAL_TESTING_GUIDE.md) |
| Tôi đang thêm hỗ trợ macOS/Linux | [FFI_SAFETY_GUIDE.md](FFI_SAFETY_GUIDE.md) |

---

## Câu Hỏi?

- **Câu hỏi chung**: [GitHub Discussions](https://github.com/dxsl-org/buttre/discussions)
- **Báo lỗi**: [GitHub Issues](https://github.com/dxsl-org/buttre/issues)
- **Vấn đề tài liệu**: [GitHub Issues](https://github.com/dxsl-org/buttre/issues) (nhãn: documentation)

---

**Cập nhật lần cuối**: 2026-06-14

_Cảm ơn bạn đã đọc tài liệu! Sự chú ý của bạn giúp buttre ngày càng hoàn thiện hơn._
