//! The system's fonts for the scripts the interface font has not (Chinese, Japanese, Korean,
//! Thai, Devanagari): nothing is shipped, the file is read from where each system keeps its
//! own, and only for a language that needs it.

use crate::i18n::Lang;

/// Where each system keeps a font for the language: Windows, macOS, then Linux and the rest.
fn candidates(l: Lang) -> &'static [&'static str] {
    match l {
        Lang::Ja => &[
            "C:\\Windows\\Fonts\\YuGothM.ttc",
            "C:\\Windows\\Fonts\\meiryo.ttc",
            "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc",
            "/System/Library/Fonts/Hiragino Sans GB.ttc",
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
        ],
        Lang::Zh => &[
            "C:\\Windows\\Fonts\\msyh.ttc",
            "/System/Library/Fonts/PingFang.ttc",
            "/System/Library/Fonts/Hiragino Sans GB.ttc",
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/wenquanyi/wqy-microhei/wqy-microhei.ttc",
            "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
        ],
        Lang::ZhTw => &[
            "C:\\Windows\\Fonts\\msjh.ttc",
            "/System/Library/Fonts/PingFang.ttc",
            "/System/Library/Fonts/STHeiti Light.ttc",
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/wenquanyi/wqy-microhei/wqy-microhei.ttc",
            "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
        ],
        Lang::Ko => &[
            "C:\\Windows\\Fonts\\malgun.ttf",
            "/System/Library/Fonts/AppleSDGothicNeo.ttc",
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/nanum/NanumGothic.ttf",
            "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
        ],
        // (Thai from fonts whose marks sit right without a shaper: Tahoma's do)
        Lang::Th => &[
            "C:\\Windows\\Fonts\\tahoma.ttf",
            "/System/Library/Fonts/Supplemental/Tahoma.ttf",
            "/System/Library/Fonts/Supplemental/Thonburi.ttc",
            "/System/Library/Fonts/Supplemental/Ayuthaya.ttf",
            "/usr/share/fonts/truetype/noto/NotoSansThai-Regular.ttf",
            "/usr/share/fonts/truetype/tlwg/Garuda.ttf",
            "/usr/share/fonts/noto/NotoSansThai-Regular.ttf",
        ],
        Lang::Hi => &[
            "C:\\Windows\\Fonts\\Nirmala.ttf",
            "C:\\Windows\\Fonts\\NirmalaUI.ttf",
            "/System/Library/Fonts/Kohinoor.ttc",
            "/System/Library/Fonts/Supplemental/Devanagari Sangam MN.ttc",
            "/usr/share/fonts/truetype/noto/NotoSansDevanagari-Regular.ttf",
            "/usr/share/fonts/noto/NotoSansDevanagari-Regular.ttf",
            "/usr/share/fonts/truetype/lohit-devanagari/Lohit-Devanagari.ttf",
        ],
        _ => &[],
    }
}

/// Does the language need a font of the system's? (The others are drawn by the interface
/// font.)
pub fn needed(l: Lang) -> bool {
    !candidates(l).is_empty()
}

/// The first font of the language the system has: its bytes and the face's index in a
/// collection.
pub fn find(l: Lang) -> Option<(Vec<u8>, u32)> {
    for p in candidates(l) {
        if let Ok(data) = std::fs::read(p) {
            return Some((data, 0));
        }
    }
    None
}
