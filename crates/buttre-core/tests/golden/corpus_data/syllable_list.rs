//! Static corpus data: Vietnamese syllables, English words, undo sequences.

/// ~290 representative Vietnamese syllables covering every nucleus, tone,
/// coda pattern, hard onset cluster, uppercase, and bare uo/uơ branches.
/// Each entry is unique — no duplicates.
#[rustfmt::skip]
pub const SYLLABLES: &[&str] = &[
    // ── single-vowel nuclei × 6 tones ──────────────────────────────────────
    "a","á","à","ả","ã","ạ",
    "ă","ắ","ằ","ẳ","ẵ","ặ",
    "â","ấ","ầ","ẩ","ẫ","ậ",
    "e","é","è","ẻ","ẽ","ẹ",
    "ê","ế","ề","ể","ễ","ệ",
    "i","í","ì","ỉ","ĩ","ị",
    "o","ó","ò","ỏ","õ","ọ",
    "ô","ố","ồ","ổ","ỗ","ộ",
    "ơ","ớ","ờ","ở","ỡ","ợ",
    "u","ú","ù","ủ","ũ","ụ",
    "ư","ứ","ừ","ử","ữ","ự",
    "y","ý","ỳ","ỷ","ỹ","ỵ",

    // ── C+V syllables ───────────────────────────────────────────────────────
    "ba","bá","bà","bả","bã","bạ",
    "da","đa","đà","đã",
    "đi","đí","đỉ","đị",
    "du","dú","dù","dủ","dũ","dụ",
    "gi","gì","gí","giả","giã",
    "ha","hà","hả","hã","hạ",
    "ho","hó","hò","hỏ","hõ","họ",
    "ke","ké","kè","kẻ","kẽ","kẹ",
    "la","lá","lề","lẻ","lỡ",
    "ma","má","mà","mả","mã","mạ",
    "mo","mò","mó","mổ",
    "na","ná","nà","nả","nã","nạ",
    "no","nó","nò","nỏ","nõ","nọ",
    "pa","pi","pu",
    "ra","rá","rà","rả","rã","rạ",
    "ro","ró","rò","rỏ","rõ","rọ",
    "sa","sá","sà","sả","sã","sạ",
    "ta","tá","tà","tả","tã","tạ",
    "ti","tí","tì","tỉ","tĩ","tị",
    "tu","tú","tù","tủ","tũ","tụ",
    "va","vá","và","vả","vã","vạ",
    "xa","xá","xà","xả","xã","xạ",

    // ── compound nuclei ─────────────────────────────────────────────────────
    "ai","ái","ài","ải","ãi","ại",
    "ao","áo","ào","ảo","ão","ạo",
    "au","áu","àu","ảu",
    "ay","ấy","ầy","ẩy","ẫy","ậy",
    "eo","éo","èo","ẻo","ẽo","ẹo",
    "ia","ía","ìa","ỉa","ĩa","ịa",
    "iê","iế","iề","iể","iễ","iệ",
    "oa","oá","oà","oả","oã","oạ",
    "oe","oé","oè","oẻ","oẽ","oẹ",
    "oi","ói","òi","ỏi","õi","ọi",
    "ôi","ối","ồi","ổi","ỗi","ội",
    "ơi","ới","ời","ởi","ỡi","ợi",
    "ua","úa","ùa","ủa","ũa","ụa",
    "ui","úi","ùi","ủi","ũi","ụi",
    // bare uo cluster: u+o without horn → end-of-word form
    "uo","uó","uò","uỏ","uõ","uọ",
    // u+ô cluster (circumflex-o): uô, uố, uồ, uổ, uỗ, uộ
    "uô","uố","uồ","uổ","uỗ","uộ",
    // ươ cluster (horn+horn): end-of-word and with coda
    "ươ","ướ","ường","ưở","ưỡ","ượ",
    "uy","úy","ùy","ủy","ũy","ụy",
    "ưa","ứa","ừa","ửa","ữa","ựa",
    "yê","yế","yề","yể","yễ","yệ",

    // ── explicit bare-cluster syllables (uo/uơ distinction) ─────────────────
    // End-of-word uơ (horn+round) without coda — thuở appears in the
    // hard-onset section below, so only thuơ (bare, untoned) is listed here.
    "thuơ",
    // ươ+coda examples:
    "nước","được","hươu",

    // ── CVC / VC syllables with codas ────────────────────────────────────────
    "an","ăn","ân","en","ên","in","on","ôn","ơn","un","ưn","yn",
    "am","em","im","om","ôm","ơm","um","ưm",
    "ang","ăng","âng","eng","êng","ing","ong","ông","ơng","ung","ưng",
    "anh","ênh","inh","oanh","unh",
    "at","ăt","ât","et","êt","it","ot","ôt","ơt","ut","ưt",
    "ac","ăc","âc","ec","êc","ic","oc","ôc","ơc","uc","ưc",
    "ap","êp","ip","op","ôp","ơp","up","ưp",
    "ach","êch","ich","och","ôch",
    "ban","bán","bàn","bẩn","bận",
    "can","cán","cần","cẩn","cãn","cạn",
    "dan","đan","đàn","đắn","đặn",
    "han","hàn","hán","hỏn","hãn","hạn",
    "lan","lán","lần","lẩn","lãn","lận",
    "man","mán","mần","mẩn","mãn","mận",
    "tan","tán","tần","tẩn","tãn","tận",
    "van","vàn","ván","vẻn","vạn",
    "bat","bát","bàt","bạt",
    "cat","cát","càt","cạt",
    "mat","mát","mạt",
    "nat","nát","nạt",

    // ── hard onset clusters ─────────────────────────────────────────────────
    // Note: chó and chú appear only here (removed duplicates from cho/chu rows)
    "cha","chá","chè","chí","chó","chú","chơ","chư",
    "chi","chì","chỉ","chĩ","chị",
    "cho","chò","chỏ","chõ","chọ",
    "chu","chù","chủ","chũ","chụ",
    "chân","chắn","chần","chẩn","chận",
    "chiều","chiếu","chiệu",
    "pha","phá","phà","phả","phã","phạ",
    "phe","phê","phế","phề","phể","phễ","phệ",
    "phi","phí","phì","phỉ","phĩ","phị",
    "phong","phóng","phòng","phổng","phọng",
    "tha","thá","thà","thả","thã","thạ",
    "the","thê","thế","thề","thể","thễ","thệ",
    "thi","thí","thì","thỉ","thĩ","thị",
    "tho","thô","thố","thồ","thổ","thỗ","thộ",
    "thu","thú","thù","thủ","thũ","thụ",
    // thuở appears only here (deduplicated from real-words section)
    "thuở","thuần","thuận","thuế","thuệ",
    "nha","nhà","nhá","nhả","nhã","nhạ",
    "nhê","nhế","nhề","nhể","nhễ","nhệ",
    "nhi","nhí","nhì","nhỉ","nhĩ","nhị",
    "nho","nhó","nhò","nhỏ","nhõ","nhọ",
    "nhu","nhú","nhù","nhủ","nhũ","nhụ",
    "nhân","nhấn","nhần","nhẩn","nhận",
    "tra","trá","trà","trả","trã","trạ",
    "tre","trê","trế","trề","trể","trễ","trệ",
    "tri","trí","trì","trỉ","trĩ","trị",
    "tro","trô","trố","trồ","trổ","trỗ","trộ",
    "tru","trú","trù","trủ","trũ","trụ",
    // trường and trước appear only here (deduplicated from real-words section)
    "trường","trướng","trước","trừng",
    "nghe","nghé","nghề","nghể","nghẽ","nghẹ",
    "nghi","nghĩ","nghị","nghin","nghìn",
    "ngó","ngủ","ngư","ngừ",
    "ngoài","ngoặt","ngoan","ngoạn",
    // ngườ (bare ươ+i without tone) + toned forms; người deduplicated from real-words
    "ngườ","người","ngồi",
    "khuya","khuỷu","khuất","khuây",
    "qua","quá","quà","quả","quã","quạ",
    "que","qué","quê","quế","quề","quể","quễ","quệ",
    "qui","quí","quì","quỉ","quĩ","quị",
    // quo cluster: bare q+u+o and q+u+ô variants
    "quo","quô","quố","quồ","quổ","quỗ","quộ",
    "quy","quý","quỳ","quỷ","quỹ","quỵ",
    // quyền appears only here (deduplicated from real-words section)
    "quyết","quyền","quyện","quyến",
    // giải appears only here (deduplicated from real-words section)
    "giải","giành","giàn","giặt","giống",
    "giờ","giữ","giúp","giận","giấc",

    // ── real words with complex tone placement ───────────────────────────────
    // (thuở, trường, trước, người, khuỷu, quyền, giải deduplicated above)
    "quở","quyệt","nghiêng",
    "mướn","vượt","thuyền","chiếc","buổi","muốn","suốt","luật","nhiều",
    "phương","hướng","tường","đường","rượu","ướt","lướt",
    "ngước","sướng","dượng","thước","bướng","cướp","nướng","lượng",
    "thuốc","buộc","nuốt","cuốn","luộc","muộn","ruộng","đuổi","muỗi",
    "tiền","tiếng","điền","điếu","triều","miền","nhiễu","riêng","biển",
    "chiến","xuyên","duyên","tuyến","huyền","luyện",

    // ── uppercase coverage ──────────────────────────────────────────────────
    // These exercise the case-preservation path that a refactor could break.
    // Single-char uppercase diacritics:
    "Â","Ă","Ê","Ô","Ơ","Ư","Đ",
    // Uppercase toned single-vowel syllables:
    "Á","À","Ả","Ã","Ạ","Ấ","Ầ","Ẩ","Ẫ","Ậ",
    "Ắ","Ằ","Ẳ","Ẵ","Ặ",
    "É","È","Ẻ","Ẽ","Ẹ","Ế","Ề","Ể","Ễ","Ệ",
    "Ó","Ò","Ỏ","Õ","Ọ","Ố","Ồ","Ổ","Ỗ","Ộ",
    "Ớ","Ờ","Ở","Ỡ","Ợ",
    "Ú","Ù","Ủ","Ũ","Ụ","Ứ","Ừ","Ử","Ữ","Ự",
    "Ý","Ỳ","Ỷ","Ỹ","Ỵ",
    // Uppercase onset + vowel (capitalized words):
    "Ba","Bà","Bá","Bả",
    "Đi","Đó","Đây",
    "Có","Của",
    "Việt","Viet",
    "Đúng","Đường",
    "Ôtô","Ân",
    // Mixed-case onset clusters:
    "NGười","Người","NGƯỜI",
    "THuở","Thuở",
    "TRường","Trường",
    "NGước","Ngước",
];

/// ~50 pure ASCII English words for the `EnglishWord` tag.
///
/// // known-attested-collisions: entries below whose composed output is a
/// REAL, attested Vietnamese syllable that the attestation gate cannot and
/// must not try to reject (attestation only knows "is this a real syllable",
/// not "is this English or Vietnamese lexically") — accepted by design; the
/// escape hatch is non-adjacent undo (Phase 4: retype the trigger key to
/// revert). Corpus-verified via golden regen (phase-05):
/// - `reset` → `rết` (centipede) — Telex only; VNI has no letter-doubling
///   transform, so VNI `reset` stays the literal English word.
/// - `mama` → `mâm` (tray) — Telex only, same reason.
///
/// This list is NOT exhaustive: any English word whose non-adjacent transform
/// happens to yield a real Vietnamese syllable will collide (attestation knows
/// only "real syllable?", not "English or Vietnamese?"). This is an accepted
/// design trade-off (leniency over aggressive spell-check); the universal
/// escape is the undo above, and frequency-based collision tiering was
/// explicitly descoped. Entries here are the ones surfaced by this corpus.
///
/// P6 addition — coda-"k" leak (red-team M1, accept-with-pins): adding coda
/// "k" (Đắk Lắk class, `pipeline::validation`) also structurally validates
/// nucleus "ă"/"u" + "k", which the ADJACENT `aw`→`ă` / tone-`r`→hook-on-`u`
/// paths reach ungated (same as `how`→`hơ` — deliberately not gated by the
/// non-adjacent attestation check):
/// - `hawk` → `hăk`, `gawk` → `găk` — Telex only (`aw` doubling has no VNI
///   equivalent; VNI has no letter-doubling transform at all).
/// - `murk` → `mủk` — Telex only (`r` is a Telex hook-tone key after a vowel;
///   VNI's tone keys are digits, so VNI `murk` never fires any mark and stays
///   literal).
///
/// This documents the leak as KNOWN behavior; it is not fixed here (the
/// per-nucleus coda-k rows are load-bearing for the Đắk Lắk place-name class
/// and cannot distinguish "English word" from "real Vietnamese word" any more
/// than the rest of the attestation-collision list above can).
///
/// The remaining new entries (`meme`, `photo`, `papa`, `salsa`, `radar`,
/// `banana`, `canal`, `media`, `dad`, `dads`, `nasa`) compose to their literal
/// ASCII form in both methods.
pub const ENGLISH_WORDS: &[&str] = &[
    "file", "text", "next", "expect", "window", "water", "their", "weird",
    "fix", "email", "password", "data", "type", "user", "name", "first",
    "last", "list", "from", "this", "that", "with", "have", "will", "been",
    "some", "what", "when", "where", "which", "would", "could", "should",
    "Claus", "hello", "world", "class", "style", "color", "width", "height",
    "meme", "photo", "papa", "salsa", "radar", "banana", "canal", "media",
    "dad", "dads", "reset", "nasa", "mama", "hawk", "gawk", "murk",
];

/// Telex sequences testing undo / double-key toggle behaviour.
pub const TELEX_UNDO_TOGGLE: &[&str] = &[
    "aaa", "aww", "eee", "ooo", "uww", "ddd",
    "ass", "aff", "arr", "axx", "ajj",
    "aaaa", "dddd",
    "bas", "bass", "hass",
    "sin", "can", "ban", "tan", "man", "fan", "ran", "dan", "van", "pan", "win", "fin",
];

/// VNI sequences testing undo / double-digit toggle behaviour.
pub const VNI_UNDO_TOGGLE: &[&str] = &[
    "a11", "a22", "a33", "a44", "a55", "a66", "a88", "d99",
    "a61", "a62", "a63", "a64", "a65", "a81", "a82",
    "ba1", "ba11", "ba2", "ba22",
    "dua71", "dua72", "nguoi72", "nguoi73",
    "sin", "can", "ban", "tan", "man", "fan", "ran", "dan",
];
