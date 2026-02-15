//! Encoding fixes for Shift-JIS title/artist decoding.
//!
//! INFINITAS stores song metadata as Shift-JIS in memory. Characters outside the
//! Shift-JIS repertoire (e.g. Æ, Ü, ö, ♡, ♥) are written as `?` (0x3F) by the
//! game itself. This module provides a post-decode correction table, equivalent to
//! the `encodingfixes.txt` mechanism in the C# reference implementation (Reflux).

use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use tracing::debug;

/// Title encoding fixes.
///
/// Maps the Shift-JIS-decoded (broken) title to the correct Unicode title.
/// Only full-title matches are applied to avoid false positives.
static TITLE_FIXES: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    HashMap::from([
        // Latin characters (accents, special characters)
        ("\u{00dc}bertreffen", "Übertreffen"),
        ("\u{00c6}THER", "ÆTHER"),
        ("?bertreffen", "Übertreffen"),
        ("?THER", "ÆTHER"),
        ("?Viva!", "¡Viva!"),
        ("?影", "焱影"),
        ("?u Legends", "Ōu Legends"),
        ("ACT?", "ACTØ"),
        ("Amor De Ver?o", "Amor De Verão"),
        ("Dans la nuit de l'?ternit?", "Dans la nuit de l'éternité"),
        ("Geirsk?gul", "Geirskögul"),
        ("Ignis\u{2020}Ir?", "Ignis†Iræ"),
        ("M?ch? M?nky", "Mächö Mönky"),
        ("P?rvat?", "Pārvatī"),
        ("POL?AMA\u{0418}IA", "POLꓘAMAИIA"),
        ("Pr?ludium", "Präludium"),
        (
            "Raison d'?tre\u{ff5e}交差する宿命\u{ff5e}",
            "Raison d'être～交差する宿命～",
        ),
        ("V?ID", "VØID"),
        (
            "旋律のドグマ\u{ff5e}Mis?rables\u{ff5e}",
            "旋律のドグマ～Misérables～",
        ),
        ("u?n", "uən"),
        ("Marguerite du Pr?", "Marguerite du Pré"),
        ("CANVAS feat. Quim?r", "CANVAS feat. Quimär"),
        // Symbols (hearts, etc.)
        ("LOVE?SHINE", "LOVE♡SHINE"),
        ("Sweet Sweet?Magic", "Sweet Sweet♡Magic"),
        (
            "Raspberry?Heart(English version)",
            "Raspberry♡Heart(English version)",
        ),
        ("Double??Loving Heart", "Double♡♡Loving Heart"),
        ("Love?km", "Love♥km"),
        ("超!!遠距離らぶ?メ\u{ff5e}ル", "超!!遠距離らぶ♡メ～ル"),
        ("キャトられ?恋はモ\u{ff5e}モク", "キャトられ♥恋はモ～モク"),
        (
            "表裏一体\u{ff01}\u{ff1f}怪盗いいんちょの悩み?",
            "表裏一体！？怪盗いいんちょの悩み♥",
        ),
        ("ギュ\u{ff5e}っとしたい?Prim", "ギュ～っとしたい♥Prim"),
        (
            "ギョギョっと人魚 爆婚ブライダル",
            "ギョギョっと人魚♨爆婚ブライダル",
        ),
        // Compound (multiple types of mojibake)
        ("?LOVE? シュガ\u{2192}?", "♥LOVE² シュガ→♥"),
        (
            "ジオメトリック?ティーパーティー",
            "ジオメトリック∮ティーパーティー",
        ),
        // Encoding-only (no `?` but incorrect characters)
        ("fffff", "ƒƒƒƒƒ"),
    ])
});

/// Artist encoding fixes.
static ARTIST_FIXES: LazyLock<HashMap<&'static str, &'static str>> =
    LazyLock::new(|| HashMap::from([("fffff", "ƒƒƒƒƒ"), ("D? D? MOUSE", "DÉ DÉ MOUSE")]));

/// Apply encoding fix to a decoded title, returning a corrected `Arc<str>` if a fix exists.
pub fn fix_title_encoding(title: &str) -> Option<Arc<str>> {
    TITLE_FIXES.get(title).map(|&fixed| {
        debug!("Fixed title encoding: {:?} -> {:?}", title, fixed);
        Arc::from(fixed)
    })
}

/// Apply encoding fix to a decoded artist, returning a corrected `Arc<str>` if a fix exists.
pub fn fix_artist_encoding(artist: &str) -> Option<Arc<str>> {
    ARTIST_FIXES.get(artist).map(|&fixed| {
        debug!("Fixed artist encoding: {:?} -> {:?}", artist, fixed);
        Arc::from(fixed)
    })
}
