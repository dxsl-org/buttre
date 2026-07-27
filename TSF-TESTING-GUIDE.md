# Hướng dẫn Test TSF (Text Services Framework)

## 🚀 Cài đặt Nhanh

### Bước 1: Chạy Script Cài Đặt Tự Động

1. Mở **PowerShell as Administrator**:
   - Nhấn `Windows + X`
   - Chọn **Terminal (Admin)** hoặc **PowerShell (Admin)**

2. Chạy lệnh sau:
```powershell
cd "C:\Users\Admin\Download\buttre"
.\scripts\build-tsf.ps1 -Install
```

Script sẽ tự động:
- ✅ Build DLL ở chế độ Release
- ✅ Copy DLL vào `C:\Program Files\buttre\`
- ✅ Register COM server với Windows
- ✅ Hiển thị hướng dẫn tiếp theo

### Bước 2: Thêm Tiếng Việt (nếu chưa có)

1. Mở **Settings** (Windows + I)
2. Vào **Time & Language** > **Language & region**
3. Click **Add a language**
4. Tìm **Vietnamese** và click **Next** > **Install**

### Bước 3: Thêm buttre Input Method

1. Ở màn hình **Language & region**, tìm **Vietnamese**
2. Click nút **⋯** (3 chấm) bên cạnh Vietnamese
3. Chọn **Language options**
4. Trong phần **Keyboards**, click **Add a keyboard**
5. Tìm và chọn **buttre Vietnamese Input**
   - Nếu không thấy: Restart máy và thử lại

### Bước 4: Chuyển sang buttre TSF

**Cách 1: Dùng phím tắt**
- Nhấn `Windows + Space` để switch giữa các input methods
- Chọn **Vietnamese** > **buttre Vietnamese Input**

**Cách 2: Dùng Taskbar**
- Click vào icon ngôn ngữ ở góc phải taskbar (VIE hoặc ENG)
- Chọn **Vietnamese** > **buttre Vietnamese Input**

## 🧪 Test Cases

### Test 1: Telex Basic
Mở Notepad và gõ:
```
Input: viet
Output: việt

Input: hoa
Output: hòa

Input: thuong
Output: thương

Input: anh
Output: anh

Input: Aaron
Output: Aaron (English fallback)
```

### Test 2: VNI Basic
Switch sang VNI method trong settings, gõ:
```
Input: vie65t
Output: việt

Input: ho2a
Output: hòa

Input: d9i
Output: đi
```

### Test 3: Composition Window
TSF hỗ trợ composition window (pre-edit buffer):
```
Input: vi
Display: vi (underlined)

Input: vie
Display: vie (underlined)

Input: viet
Display: viet (underlined)

Input: f (tone)
Commit: việt (no underline)
```

### Test 4: Candidate UI
Nếu có từ điển (future feature):
```
Input: viet
Candidates:
1. việt
2. viết
3. viêt
```

### Test 5: Undo
```
Input: aa
Output: â

Input: aaa
Output: aa (undo circumflex)

Input: vietf
Output: việt

Input: vietff
Output: vietf (undo tone)
```

## 🔍 Kiểm Tra Trạng Thái

### Check TSF có được register không:
```powershell
# Xem trong Registry
reg query "HKLM\SOFTWARE\Classes\CLSID\{E6B8A6C0-1234-5678-9ABC-DEF012345678}"

# Hoặc dùng script có sẵn
.\scripts\check-tsf-status.ps1
```

### Xem Logs
TSF logs được ghi vào:
```
%TEMP%\buttre-tsf.log
```

Mở bằng:
```powershell
notepad $env:TEMP\buttre-tsf.log
```

## ❌ Troubleshooting

### Vấn đề: buttre không xuất hiện trong danh sách keyboards

**Giải pháp:**
1. Restart máy
2. Chạy lại `.\scripts\build-tsf.ps1 -Install`
3. Kiểm tra Event Viewer:
   - `Win + X` > Event Viewer
   - Windows Logs > Application
   - Tìm lỗi từ "buttre" hoặc "COM"

### Vấn đề: COM registration failed

**Nguyên nhân:**
- Thiếu Visual C++ Runtime
- DLL bị corrupt
- Windows Defender block

**Giải pháp:**
```powershell
# Uninstall và install lại
.\scripts\build-tsf.ps1 -Uninstall
.\scripts\build-tsf.ps1 -Install

# Check DLL dependencies
dumpbin /dependents "C:\Program Files\buttre\buttre_platform.dll"
```

### Vấn đề: Gõ không ra tiếng Việt

**Kiểm tra:**
1. Đảm bảo đã switch đúng input method (Windows + Space)
2. Icon ngôn ngữ trên taskbar phải hiện **VIE** và **buttre**
3. Restart ứng dụng đang test (Notepad, Word, etc.)
4. Xem logs tại `%TEMP%\buttre-tsf.log`

### Vấn đề: Composition không hoạt động

TSF composition yêu cầu:
- Ứng dụng phải hỗ trợ TSF (Notepad, Word, Chrome, VS Code...)
- Một số app cũ chỉ hỗ trợ IMM32

**Test app tốt nhất:**
- ✅ Notepad (Windows built-in)
- ✅ Microsoft Word
- ✅ Google Chrome
- ✅ Firefox
- ✅ VS Code
- ❌ Command Prompt (không hỗ trợ TSF)
- ❌ Some legacy apps

## 🗑️ Gỡ Cài Đặt

### Cách 1: Dùng Script
```powershell
.\scripts\build-tsf.ps1 -Uninstall
```

### Cách 2: Manual
1. Unregister COM:
   ```powershell
   regsvr32 /u "C:\Program Files\buttre\buttre_platform.dll"
   ```

2. Xóa files:
   ```powershell
   Remove-Item "C:\Program Files\buttre" -Recurse -Force
   ```

3. Xóa keyboard khỏi Windows:
   - Settings > Language > Vietnamese > Options
   - Tìm "buttre" và click Remove

## 📊 So Sánh TSF vs Hook

| Feature | TSF | Hook |
|---------|-----|------|
| Composition Window | ✅ Yes | ❌ No |
| Candidate List | ✅ Yes | ❌ No |
| Display Attributes | ✅ Yes | ❌ No |
| App Compatibility | ⚠️  Modern apps | ✅ All apps |
| Performance | ✅ Better | ⚠️  Good |
| Installation | ⚠️  Requires Admin | ✅ Simple |
| Windows Integration | ✅ Native | ⚠️  Hook-based |

## 🎯 Điểm Cần Test Kỹ

Ưu tiên test những điểm sau:

1. **Composition Lifecycle**
   - Start composition khi gõ ký tự đầu
   - Update composition khi transform
   - End composition khi Space/Enter
   - Cancel composition khi Esc

2. **Tone Positioning**
   - Các từ phức tạp: thuở, người, trường, khuỷu
   - Super vowels (ă, â, ê, ô, ơ, ư)
   - Special cases (QU, GI)

3. **Undo Behavior**
   - Triple key undo (aaa → aa)
   - Tone undo (vietff → vietf)
   - Transform undo

4. **English Fallback**
   - Aaron, Google, Facebook
   - Temp English mode activation
   - Reset on non-alphabetic

5. **Multi-app Testing**
   - Notepad, Word, Chrome, VS Code
   - Copy/paste between apps
   - Switch apps during composition

## 📝 Báo Lỗi

Khi test thấy lỗi, vui lòng cung cấp:
1. Input sequence (e.g., "vietf")
2. Expected output (e.g., "việt")
3. Actual output (e.g., "viet f")
4. Ứng dụng đang test (e.g., Notepad)
5. Screenshot (nếu có)
6. Log file từ `%TEMP%\buttre-tsf.log`

## ✅ Checklist Test TSF

- [ ] Cài đặt thành công
- [ ] buttre xuất hiện trong danh sách keyboards
- [ ] Switch được sang buttre bằng Windows + Space
- [ ] Gõ được tiếng Việt cơ bản (viet, hoa, thuong)
- [ ] Tone positioning đúng
- [ ] Undo hoạt động
- [ ] English fallback (Aaron, Google)
- [ ] Composition window hiển thị
- [ ] Test trên nhiều app (Notepad, Chrome, Word)
- [ ] Performance tốt, không lag

Chúc bạn test thành công! 🎉
