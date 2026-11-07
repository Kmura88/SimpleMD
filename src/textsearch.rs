//! # 機能: 日本語に強い文字列照合
//!
//! ページ内検索・全文検索・ファイル名検索で共通に使う。次の違いを同一視して探す。
//! - 大文字 / 小文字（`ABC` = `abc`）
//! - 全角 / 半角の英数字・記号（`ＡＢＣ１` = `ABC1`、全角スペース = 半角スペース）
//! - ひらがな / カタカナ（`めも` = `メモ`）
//! - 半角カタカナ（`ﾒﾓ` = `メモ`、濁点・半濁点付きの `ｶﾞ` = `ガ` も 1 文字として扱う）
//! - 波ダッシュの表記ゆれ（`〜` = `～` = `~`）
//!
//! 照合は文字を「正規化した列」に変換して行い、見つかった位置は元の文字列の
//! バイト範囲に戻して返す（ハイライト表示にそのまま使える）。
//!
//! また、IME が使えない環境でもファイルを探せるよう、ローマ字 → ひらがな変換
//! （[`romaji_to_hiragana`]）を用意している（ファイル名のあいまい検索で使う）。

use std::ops::Range;

// ==== 機能: 文字の正規化 ====

/// 半角カタカナ（U+FF66〜U+FF9D）に対応する全角カタカナ。
const HALF_KANA: &str = "ヲァィゥェォャュョッーアイウエオカキクケコサシスセソタチツテトナニヌネノハヒフヘホマミムメモヤユヨラリルレロワン";

/// 1 文字を正規化する（照合用）。
pub fn fold_char(c: char) -> char {
    let c = match c as u32 {
        // 全角英数字・記号 → 半角
        0xFF01..=0xFF5E => char::from_u32(c as u32 - 0xFF01 + 0x21).unwrap_or(c),
        // 全角スペース → 半角スペース
        0x3000 => ' ',
        // 波ダッシュ
        0x301C => '~',
        // 半角カタカナ → 全角カタカナ
        0xFF66..=0xFF9D => HALF_KANA
            .chars()
            .nth((c as u32 - 0xFF66) as usize)
            .unwrap_or(c),
        _ => c,
    };
    // カタカナ → ひらがな（ァ〜ヶ。長音記号「ー」などはそのまま）
    let c = match c as u32 {
        0x30A1..=0x30F6 => char::from_u32(c as u32 - 0x60).unwrap_or(c),
        _ => c,
    };
    c.to_lowercase().next().unwrap_or(c)
}

/// 濁点・半濁点を前の文字（正規化後のひらがな）と合成する。合成できなければ `None`。
fn compose(base: char, mark: char) -> Option<char> {
    const DAKUTEN_BASE: &str = "かきくけこさしすせそたちつてとはひふへほ";
    const HANDAKUTEN_BASE: &str = "はひふへほ";
    match mark {
        // ﾞ / ゛
        '\u{FF9E}' | '\u{309B}' | '\u{3099}' => {
            if base == 'う' {
                return Some('ゔ');
            }
            DAKUTEN_BASE
                .contains(base)
                .then(|| {
                    // は行は「は ば ぱ」と 3 つ並ぶ。他は「か が」と 2 つ並ぶ。
                    char::from_u32(base as u32 + 1)
                })
                .flatten()
        }
        // ﾟ / ゜
        '\u{FF9F}' | '\u{309C}' | '\u{309A}' => HANDAKUTEN_BASE
            .contains(base)
            .then(|| char::from_u32(base as u32 + 2))
            .flatten(),
        _ => None,
    }
}

/// 正規化した文字列と、各文字が元の文字列のどのバイト範囲から来たか。
struct Folded {
    chars: Vec<char>,
    spans: Vec<Range<usize>>,
}

/// 文字列を正規化する。濁点・半濁点（半角の ﾞﾟ を含む）は前の文字と 1 文字に合成する。
fn fold(s: &str) -> Folded {
    let mut chars: Vec<char> = Vec::with_capacity(s.len());
    let mut spans: Vec<Range<usize>> = Vec::with_capacity(s.len());
    for (i, c) in s.char_indices() {
        let end = i + c.len_utf8();
        if let (Some(prev), Some(span)) = (chars.last_mut(), spans.last_mut())
            && let Some(composed) = compose(*prev, c)
        {
            *prev = composed;
            span.end = end;
            continue;
        }
        chars.push(fold_char(c));
        spans.push(i..end);
    }
    Folded { chars, spans }
}

/// 照合用に正規化した文字列（ファイル名のあいまい検索などで使う）。
pub fn normalize(s: &str) -> String {
    fold(s).chars.into_iter().collect()
}

// ==== 機能: 検索 ====

/// 検索語。正規化を 1 回だけ行い、何度も照合に使う。
#[derive(Debug, Clone, PartialEq)]
pub struct Needle {
    chars: Vec<char>,
    /// true: 完全一致（大文字小文字・かな・全半角を区別する）。
    exact: bool,
}

impl Needle {
    /// 検索語を作る。`exact` なら正規化しない（「Aa」ボタンが押されたとき）。
    pub fn new(query: &str, exact: bool) -> Option<Self> {
        let chars: Vec<char> = if exact {
            query.chars().collect()
        } else {
            fold(query).chars
        };
        (!chars.is_empty()).then_some(Self { chars, exact })
    }

    /// `hay` の中の一致をすべて探し、元の文字列でのバイト範囲で返す（重ならない）。
    pub fn find_all(&self, hay: &str) -> Vec<Range<usize>> {
        let (chars, spans): (Vec<char>, Vec<Range<usize>>) = if self.exact {
            hay.char_indices()
                .map(|(i, c)| (c, i..i + c.len_utf8()))
                .unzip()
        } else {
            let f = fold(hay);
            (f.chars, f.spans)
        };
        let n = self.chars.len();
        let mut out = Vec::new();
        let mut i = 0;
        while i + n <= chars.len() {
            if chars[i..i + n] == self.chars[..] {
                out.push(spans[i].start..spans[i + n - 1].end);
                i += n;
            } else {
                i += 1;
            }
        }
        out
    }

    /// `hay` に一致があるか。
    pub fn is_in(&self, hay: &str) -> bool {
        !self.find_all(hay).is_empty()
    }

    /// 一致の数。
    pub fn count(&self, hay: &str) -> usize {
        self.find_all(hay).len()
    }
}

// ==== 機能: ローマ字 → ひらがな ====

/// ローマ字の綴りとひらがなの対応（長い綴りから順に試す）。
const ROMAJI: &[(&str, &str)] = &[
    ("ltsu", "っ"),
    ("xtsu", "っ"),
    ("kya", "きゃ"),
    ("kyu", "きゅ"),
    ("kyo", "きょ"),
    ("sha", "しゃ"),
    ("shi", "し"),
    ("shu", "しゅ"),
    ("she", "しぇ"),
    ("sho", "しょ"),
    ("sya", "しゃ"),
    ("syu", "しゅ"),
    ("syo", "しょ"),
    ("cha", "ちゃ"),
    ("chi", "ち"),
    ("chu", "ちゅ"),
    ("che", "ちぇ"),
    ("cho", "ちょ"),
    ("tya", "ちゃ"),
    ("tyu", "ちゅ"),
    ("tyo", "ちょ"),
    ("tsu", "つ"),
    ("nya", "にゃ"),
    ("nyu", "にゅ"),
    ("nyo", "にょ"),
    ("hya", "ひゃ"),
    ("hyu", "ひゅ"),
    ("hyo", "ひょ"),
    ("mya", "みゃ"),
    ("myu", "みゅ"),
    ("myo", "みょ"),
    ("rya", "りゃ"),
    ("ryu", "りゅ"),
    ("ryo", "りょ"),
    ("gya", "ぎゃ"),
    ("gyu", "ぎゅ"),
    ("gyo", "ぎょ"),
    ("jya", "じゃ"),
    ("jyu", "じゅ"),
    ("jyo", "じょ"),
    ("bya", "びゃ"),
    ("byu", "びゅ"),
    ("byo", "びょ"),
    ("pya", "ぴゃ"),
    ("pyu", "ぴゅ"),
    ("pyo", "ぴょ"),
    ("dya", "ぢゃ"),
    ("dyu", "ぢゅ"),
    ("dyo", "ぢょ"),
    ("thi", "てぃ"),
    ("dhi", "でぃ"),
    ("xtu", "っ"),
    ("ltu", "っ"),
    ("xya", "ゃ"),
    ("xyu", "ゅ"),
    ("xyo", "ょ"),
    ("lya", "ゃ"),
    ("lyu", "ゅ"),
    ("lyo", "ょ"),
    ("ja", "じゃ"),
    ("ji", "じ"),
    ("ju", "じゅ"),
    ("je", "じぇ"),
    ("jo", "じょ"),
    ("fa", "ふぁ"),
    ("fi", "ふぃ"),
    ("fu", "ふ"),
    ("fe", "ふぇ"),
    ("fo", "ふぉ"),
    ("ka", "か"),
    ("ki", "き"),
    ("ku", "く"),
    ("ke", "け"),
    ("ko", "こ"),
    ("ca", "か"),
    ("cu", "く"),
    ("co", "こ"),
    ("sa", "さ"),
    ("si", "し"),
    ("su", "す"),
    ("se", "せ"),
    ("so", "そ"),
    ("ta", "た"),
    ("ti", "ち"),
    ("tu", "つ"),
    ("te", "て"),
    ("to", "と"),
    ("na", "な"),
    ("ni", "に"),
    ("nu", "ぬ"),
    ("ne", "ね"),
    ("no", "の"),
    ("ha", "は"),
    ("hi", "ひ"),
    ("hu", "ふ"),
    ("he", "へ"),
    ("ho", "ほ"),
    ("ma", "ま"),
    ("mi", "み"),
    ("mu", "む"),
    ("me", "め"),
    ("mo", "も"),
    ("ya", "や"),
    ("yu", "ゆ"),
    ("ye", "いぇ"),
    ("yo", "よ"),
    ("ra", "ら"),
    ("ri", "り"),
    ("ru", "る"),
    ("re", "れ"),
    ("ro", "ろ"),
    ("la", "ら"),
    ("li", "り"),
    ("lu", "る"),
    ("le", "れ"),
    ("lo", "ろ"),
    ("wa", "わ"),
    ("wi", "うぃ"),
    ("we", "うぇ"),
    ("wo", "を"),
    ("ga", "が"),
    ("gi", "ぎ"),
    ("gu", "ぐ"),
    ("ge", "げ"),
    ("go", "ご"),
    ("za", "ざ"),
    ("zi", "じ"),
    ("zu", "ず"),
    ("ze", "ぜ"),
    ("zo", "ぞ"),
    ("da", "だ"),
    ("di", "ぢ"),
    ("du", "づ"),
    ("de", "で"),
    ("do", "ど"),
    ("ba", "ば"),
    ("bi", "び"),
    ("bu", "ぶ"),
    ("be", "べ"),
    ("bo", "ぼ"),
    ("pa", "ぱ"),
    ("pi", "ぴ"),
    ("pu", "ぷ"),
    ("pe", "ぺ"),
    ("po", "ぽ"),
    ("va", "ゔぁ"),
    ("vi", "ゔぃ"),
    ("vu", "ゔ"),
    ("ve", "ゔぇ"),
    ("vo", "ゔぉ"),
    ("xa", "ぁ"),
    ("xi", "ぃ"),
    ("xu", "ぅ"),
    ("xe", "ぇ"),
    ("xo", "ぉ"),
    ("nn", "ん"),
    ("n'", "ん"),
    ("a", "あ"),
    ("i", "い"),
    ("u", "う"),
    ("e", "え"),
    ("o", "お"),
    ("-", "ー"),
];

/// ローマ字をひらがなにする（例: `memo` → `めも`、`kitte` → `きって`、`shinbun` → `しんぶん`）。
/// 英字以外を含む、または最後までかなにできない綴りがあれば `None`。
pub fn romaji_to_hiragana(s: &str) -> Option<String> {
    let s = s.to_ascii_lowercase();
    if s.is_empty()
        || !s
            .chars()
            .all(|c| c.is_ascii_alphabetic() || c == '-' || c == '\'')
    {
        return None;
    }
    let b = s.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    'outer: while i < b.len() {
        let c = b[i];
        // 子音の重ね（kk, tt …）は「っ」。n は除く。
        if i + 1 < b.len() && c == b[i + 1] && c.is_ascii_alphabetic() && !b"aiueon".contains(&c) {
            out.push('っ');
            i += 1;
            continue;
        }
        // 「nn」の後に母音・y が続く（konnichiha）なら、最初の n だけで「ん」。
        if c == b'n' && i + 2 < b.len() && b[i + 1] == b'n' && b"aiueoy".contains(&b[i + 2]) {
            out.push('ん');
            i += 1;
            continue;
        }
        // 母音・y 以外の前の n は「ん」。
        if c == b'n' && (i + 1 == b.len() || !b"aiueoyn'".contains(&b[i + 1])) {
            out.push('ん');
            i += 1;
            continue;
        }
        for (r, h) in ROMAJI {
            if s[i..].starts_with(r) {
                out.push_str(h);
                i += r.len();
                continue 'outer;
            }
        }
        return None;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find<'a>(hay: &'a str, q: &str) -> Vec<&'a str> {
        Needle::new(q, false)
            .unwrap()
            .find_all(hay)
            .into_iter()
            .map(|r| &hay[r])
            .collect()
    }

    #[test]
    fn folds_kana_width_and_case() {
        assert_eq!(find("会議のメモと、めも", "めも"), ["メモ", "めも"]);
        assert_eq!(find("ＡＢＣ abc Abc", "abc"), ["ＡＢＣ", "abc", "Abc"]);
        assert_eq!(find("ﾒﾓ帳", "メモ"), ["ﾒﾓ"]);
        assert_eq!(find("ｶﾞｲﾄﾞ と ガイド", "がいど"), ["ｶﾞｲﾄﾞ", "ガイド"]);
        assert_eq!(find("ﾊﾟﾝ", "パン"), ["ﾊﾟﾝ"]);
        assert_eq!(find("10〜20 と 10～20", "10~20"), ["10〜20", "10～20"]);
        assert_eq!(
            find("日本語　テキスト", "日本語 てきすと"),
            ["日本語　テキスト"]
        );
        assert_eq!(find("aaa", "aa"), ["aa"]); // 重ならない
        assert!(Needle::new("", false).is_none());
    }

    #[test]
    fn exact_mode_distinguishes() {
        let n = Needle::new("メモ", true).unwrap();
        assert_eq!(n.count("メモ めも ﾒﾓ"), 1);
        let n = Needle::new("ABC", true).unwrap();
        assert_eq!(n.count("abc ABC"), 1);
    }

    #[test]
    fn romaji() {
        assert_eq!(romaji_to_hiragana("memo").as_deref(), Some("めも"));
        assert_eq!(romaji_to_hiragana("kitte").as_deref(), Some("きって"));
        assert_eq!(romaji_to_hiragana("shinbun").as_deref(), Some("しんぶん"));
        assert_eq!(
            romaji_to_hiragana("konnichiha").as_deref(),
            Some("こんにちは")
        );
        assert_eq!(romaji_to_hiragana("gaido").as_deref(), Some("がいど"));
        assert_eq!(romaji_to_hiragana("kyou").as_deref(), Some("きょう"));
        assert_eq!(romaji_to_hiragana("ra-men").as_deref(), Some("らーめん"));
        assert_eq!(romaji_to_hiragana("readme.md"), None);
        assert_eq!(romaji_to_hiragana("xq"), None);
    }

    #[test]
    fn normalize_for_fuzzy() {
        assert_eq!(normalize("ガイド.MD"), "がいど.md");
        assert_eq!(normalize("ｶﾞｲﾄﾞ"), "がいど");
    }
}
