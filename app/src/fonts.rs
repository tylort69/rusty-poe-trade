//! System font fallbacks for non-Latin game languages (Korean, Traditional
//! Chinese). We don't bundle CJK fonts (tens of MB); instead we load what the
//! OS ships: Noto CJK on Arch (`noto-fonts-cjk`), Malgun Gothic / Microsoft
//! JhengHei on Windows 11.

pub fn system_fallbacks(language: &str) -> Vec<(String, Vec<u8>)> {
    let candidates: &[&str] = match language {
        "ko" => &[
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/OTF/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/noto-cjk/NotoSansKR-Regular.otf",
            "C:\\Windows\\Fonts\\malgun.ttf",
        ],
        "cmn-Hant" => &[
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/OTF/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/adobe-source-han-sans/SourceHanSansTW-Regular.otf",
            "C:\\Windows\\Fonts\\msjh.ttc",
        ],
        // Cyrillic is covered by Roboto.
        _ => &[],
    };
    candidates
        .iter()
        .find_map(|p| std::fs::read(p).ok().map(|b| (format!("system-{language}"), b)))
        .into_iter()
        .collect()
}
