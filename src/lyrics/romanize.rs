//! Algorithmic real-time romanization engine for Korean (Hangul), Japanese (Kana),
//! Cyrillic (Russian/Ukrainian/Slavic), and Greek lyrics.
//!
//! Provides instant, offline romanization with 0ms runtime rendering latency.

/// Checks whether the text contains characters that benefit from romanization.
pub fn needs_romanization(text: &str) -> bool {
    text.chars().any(|c| {
        // Korean Hangul Syllables and Jamo
        ('\u{AC00}'..='\u{D7A3}').contains(&c)
            || ('\u{3131}'..='\u{318E}').contains(&c)
            // Japanese Hiragana and Katakana
            || ('\u{3040}'..='\u{309F}').contains(&c)
            || ('\u{30A0}'..='\u{30FF}').contains(&c)
            // Cyrillic
            || ('\u{0400}'..='\u{04FF}').contains(&c)
            // Greek
            || ('\u{0370}'..='\u{03FF}').contains(&c)
    })
}

// ---------------------------------------------------------------------------
// Korean Hangul Revised Romanization (Algorithmic Mathematical Decomposition)
// ---------------------------------------------------------------------------

const CHOSEONG: [&str; 19] = [
    "g", "kk", "n", "d", "tt", "r", "m", "b", "pp", "s", "ss", "", "j", "jj", "ch", "k", "t",
    "p", "h",
];

const JUNGSEONG: [&str; 21] = [
    "a", "ae", "ya", "yae", "eo", "e", "yeo", "ye", "o", "wa", "wae", "oe", "yo", "u", "wo",
    "we", "wi", "yu", "eu", "ui", "i",
];

const JONGSEONG: [&str; 28] = [
    "", "k", "k", "ks", "n", "nj", "nh", "t", "l", "lg", "lm", "lb", "ls", "lt", "lp", "lh",
    "m", "p", "ps", "s", "ss", "ng", "j", "ch", "k", "t", "p", "h",
];

fn romanize_hangul_char(c: char, out: &mut String) -> bool {
    let code = c as u32;
    if (0xAC00..=0xD7A3).contains(&code) {
        let syllable_idx = code - 0xAC00;
        let initial_idx = (syllable_idx / 588) as usize;
        let vowel_idx = ((syllable_idx % 588) / 28) as usize;
        let final_idx = (syllable_idx % 28) as usize;

        out.push_str(CHOSEONG[initial_idx]);
        out.push_str(JUNGSEONG[vowel_idx]);
        out.push_str(JONGSEONG[final_idx]);
        true
    } else if (0x3131..=0x318E).contains(&code) {
        let s = match c {
            'ㄱ' => "g", 'ㄲ' => "kk", 'ㄳ' => "gs", 'ㄴ' => "n", 'ㄵ' => "nj", 'ㄶ' => "nh",
            'ㄷ' => "d", 'ㄸ' => "tt", 'ㄹ' => "r", 'ㄺ' => "lg", 'ㄻ' => "lm", 'ㄼ' => "lb",
            'ㄽ' => "ls", 'ㄾ' => "lt", 'ㄿ' => "lp", 'ㅀ' => "lh", 'ㅁ' => "m", 'ㅂ' => "b",
            'ㅃ' => "pp", 'ㅄ' => "bs", 'ㅅ' => "s", 'ㅆ' => "ss", 'ㅇ' => "ng", 'ㅈ' => "j",
            'ㅉ' => "jj", 'ㅊ' => "ch", 'ㅋ' => "k", 'ㅌ' => "t", 'ㅍ' => "p", 'ㅎ' => "h",
            'ㅏ' => "a", 'ㅐ' => "ae", 'ㅑ' => "ya", 'ㅒ' => "yae", 'ㅓ' => "eo", 'ㅔ' => "e",
            'ㅕ' => "yeo", 'ㅖ' => "ye", 'ㅗ' => "o", 'ㅘ' => "wa", 'ㅙ' => "wae", 'ㅚ' => "oe",
            'ㅛ' => "yo", 'ㅜ' => "u", 'ㅝ' => "wo", 'ㅞ' => "we", 'ㅟ' => "wi", 'ㅠ' => "yu",
            'ㅡ' => "eu", 'ㅢ' => "ui", 'ㅣ' => "i",
            _ => "",
        };
        if !s.is_empty() {
            out.push_str(s);
            true
        } else {
            false
        }
    } else {
        false
    }
}

// ---------------------------------------------------------------------------
// Japanese Kana (Hepburn Romaji with Digraphs and Sokuon)
// ---------------------------------------------------------------------------

fn to_hiragana(c: char) -> char {
    let code = c as u32;
    if (0x30A1..=0x30F6).contains(&code) {
        std::char::from_u32(code - 0x60).unwrap_or(c)
    } else {
        c
    }
}

fn kana_digraph(first: char, second: char) -> Option<&'static str> {
    let f = to_hiragana(first);
    let s = to_hiragana(second);
    match (f, s) {
        ('き', 'ゃ') => Some("kya"),
        ('き', 'ゅ') => Some("kyu"),
        ('き', 'ょ') => Some("kyo"),
        ('し', 'ゃ') => Some("sha"),
        ('し', 'ゅ') => Some("shu"),
        ('し', 'ょ') => Some("sho"),
        ('ち', 'ゃ') => Some("cha"),
        ('ち', 'ゅ') => Some("chu"),
        ('ち', 'ょ') => Some("cho"),
        ('に', 'ゃ') => Some("nya"),
        ('に', 'ゅ') => Some("nyu"),
        ('に', 'ょ') => Some("nyo"),
        ('ひ', 'ゃ') => Some("hya"),
        ('ひ', 'ゅ') => Some("hyu"),
        ('ひ', 'ょ') => Some("hyo"),
        ('み', 'ゃ') => Some("mya"),
        ('み', 'ゅ') => Some("myu"),
        ('み', 'ょ') => Some("myo"),
        ('り', 'ゃ') => Some("rya"),
        ('り', 'ゅ') => Some("ryu"),
        ('り', 'ょ') => Some("ryo"),
        ('ぎ', 'ゃ') => Some("gya"),
        ('ぎ', 'ゅ') => Some("gyu"),
        ('ぎ', 'ょ') => Some("gyo"),
        ('じ', 'ゃ') => Some("ja"),
        ('じ', 'ゅ') => Some("ju"),
        ('じ', 'ょ') => Some("jo"),
        ('び', 'ゃ') => Some("bya"),
        ('び', 'ゅ') => Some("byu"),
        ('び', 'ょ') => Some("byo"),
        ('ぴ', 'ゃ') => Some("pya"),
        ('ぴ', 'ゅ') => Some("pyu"),
        ('ぴ', 'ょ') => Some("pyo"),
        _ => None,
    }
}

fn kana_monograph(c: char) -> Option<&'static str> {
    let h = to_hiragana(c);
    match h {
        'あ' => Some("a"),
        'い' => Some("i"),
        'う' => Some("u"),
        'え' => Some("e"),
        'お' => Some("o"),
        'か' => Some("ka"),
        'き' => Some("ki"),
        'く' => Some("ku"),
        'け' => Some("ke"),
        'こ' => Some("ko"),
        'さ' => Some("sa"),
        'し' => Some("shi"),
        'す' => Some("su"),
        'せ' => Some("se"),
        'そ' => Some("so"),
        'た' => Some("ta"),
        'ち' => Some("chi"),
        'つ' => Some("tsu"),
        'て' => Some("te"),
        'と' => Some("to"),
        'な' => Some("na"),
        'に' => Some("ni"),
        'ぬ' => Some("nu"),
        'ね' => Some("ne"),
        'の' => Some("no"),
        'は' => Some("ha"),
        'ひ' => Some("hi"),
        'ふ' => Some("fu"),
        'へ' => Some("he"),
        'ほ' => Some("ho"),
        'ま' => Some("ma"),
        'み' => Some("mi"),
        'む' => Some("mu"),
        'め' => Some("me"),
        'も' => Some("mo"),
        'や' => Some("ya"),
        'ゆ' => Some("yu"),
        'よ' => Some("yo"),
        'ら' => Some("ra"),
        'り' => Some("ri"),
        'る' => Some("ru"),
        'れ' => Some("re"),
        'ろ' => Some("ro"),
        'わ' => Some("wa"),
        'を' => Some("wo"),
        'ん' => Some("n"),
        'が' => Some("ga"),
        'ぎ' => Some("gi"),
        'ぐ' => Some("gu"),
        'げ' => Some("ge"),
        'ご' => Some("go"),
        'ざ' => Some("za"),
        'じ' => Some("ji"),
        'ず' => Some("zu"),
        'ぜ' => Some("ze"),
        'ぞ' => Some("zo"),
        'だ' => Some("da"),
        'ぢ' => Some("ji"),
        'づ' => Some("dzu"),
        'で' => Some("de"),
        'ど' => Some("do"),
        'ば' => Some("ba"),
        'び' => Some("bi"),
        'ぶ' => Some("bu"),
        'べ' => Some("be"),
        'ぼ' => Some("bo"),
        'ぱ' => Some("pa"),
        'ぴ' => Some("pi"),
        'ぷ' => Some("pu"),
        'ぺ' => Some("pe"),
        'ぽ' => Some("po"),
        'ぁ' => Some("a"),
        'ぃ' => Some("i"),
        'ぅ' => Some("u"),
        'ぇ' => Some("e"),
        'ぉ' => Some("o"),
        'ゃ' => Some("ya"),
        'ゅ' => Some("yu"),
        'ょ' => Some("yo"),
        'ゎ' => Some("wa"),
        'ー' => Some("-"),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Cyrillic (Russian / Ukrainian / Slavic)
// ---------------------------------------------------------------------------

fn romanize_cyrillic(c: char) -> Option<&'static str> {
    match c {
        'А' => Some("A"), 'а' => Some("a"), 'Б' => Some("B"), 'б' => Some("b"),
        'В' => Some("V"), 'в' => Some("v"), 'Г' => Some("G"), 'г' => Some("g"),
        'Д' => Some("D"), 'д' => Some("d"), 'Е' => Some("E"), 'е' => Some("e"),
        'Ё' => Some("Yo"), 'ё' => Some("yo"), 'Ж' => Some("Zh"), 'ж' => Some("zh"),
        'З' => Some("Z"), 'з' => Some("z"), 'И' => Some("I"), 'и' => Some("i"),
        'Й' => Some("Y"), 'й' => Some("y"), 'К' => Some("K"), 'к' => Some("k"),
        'Л' => Some("L"), 'л' => Some("l"), 'М' => Some("M"), 'м' => Some("m"),
        'Н' => Some("N"), 'н' => Some("n"), 'О' => Some("O"), 'о' => Some("o"),
        'П' => Some("P"), 'п' => Some("p"), 'Р' => Some("R"), 'р' => Some("r"),
        'С' => Some("S"), 'с' => Some("s"), 'Т' => Some("T"), 'т' => Some("t"),
        'У' => Some("U"), 'у' => Some("u"), 'Ф' => Some("F"), 'ф' => Some("f"),
        'Х' => Some("Kh"), 'х' => Some("kh"), 'Ц' => Some("Ts"), 'ц' => Some("ts"),
        'Ч' => Some("Ch"), 'ч' => Some("ch"), 'Ш' => Some("Sh"), 'ш' => Some("sh"),
        'Щ' => Some("Shch"), 'щ' => Some("shch"), 'Ъ' => Some(""), 'ъ' => Some(""),
        'Ы' => Some("Y"), 'ы' => Some("y"), 'Ь' => Some(""), 'ь' => Some(""),
        'Э' => Some("E"), 'э' => Some("e"), 'Ю' => Some("Yu"), 'ю' => Some("yu"),
        'Я' => Some("Ya"), 'я' => Some("ya"),
        'Є' => Some("Ye"), 'є' => Some("ye"), 'І' => Some("I"), 'і' => Some("i"),
        'Ї' => Some("Yi"), 'ї' => Some("yi"),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Greek Transliteration
// ---------------------------------------------------------------------------

fn romanize_greek(c: char) -> Option<&'static str> {
    match c {
        'Α' => Some("A"), 'α' => Some("a"), 'Β' => Some("V"), 'β' => Some("v"),
        'Γ' => Some("G"), 'γ' => Some("g"), 'Δ' => Some("D"), 'δ' => Some("d"),
        'Ε' => Some("E"), 'ε' => Some("e"), 'Ζ' => Some("Z"), 'ζ' => Some("z"),
        'Η' => Some("I"), 'η' => Some("i"), 'Θ' => Some("Th"), 'θ' => Some("th"),
        'Ι' => Some("I"), 'ι' => Some("i"), 'Κ' => Some("K"), 'κ' => Some("k"),
        'Λ' => Some("L"), 'λ' => Some("l"), 'Μ' => Some("M"), 'μ' => Some("m"),
        'Ν' => Some("N"), 'ν' => Some("n"), 'Ξ' => Some("X"), 'ξ' => Some("x"),
        'Ο' => Some("O"), 'ο' => Some("o"), 'Π' => Some("P"), 'π' => Some("p"),
        'Ρ' => Some("R"), 'ρ' => Some("r"), 'Σ' => Some("S"), 'σ' | 'ς' => Some("s"),
        'Τ' => Some("T"), 'τ' => Some("t"), 'Υ' => Some("Y"), 'υ' => Some("y"),
        'Φ' => Some("F"), 'φ' => Some("f"), 'Χ' => Some("Ch"), 'χ' => Some("ch"),
        'Ψ' => Some("Ps"), 'ψ' => Some("ps"), 'Ω' => Some("O"), 'ω' => Some("o"),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Unified Romanizer
// ---------------------------------------------------------------------------

/// Romanizes any string containing non-Latin text (Korean, Japanese Kana, Cyrillic, Greek).
/// Returns `None` if the text contains no non-Latin characters.
pub fn romanize_text(text: &str) -> Option<String> {
    if !needs_romanization(text) {
        return None;
    }

    let mut out = String::with_capacity(text.len() * 2);
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];

        // 1. Korean Hangul
        if romanize_hangul_char(c, &mut out) {
            i += 1;
            continue;
        }

        // 2. Japanese Kana
        let h_c = to_hiragana(c);
        if h_c == 'っ' || c == 'ッ' {
            // Sokuon (small tsu): double next consonant
            if i + 1 < chars.len() {
                let next_c = chars[i + 1];
                if let Some(next_rom) = kana_digraph(next_c, chars.get(i + 2).copied().unwrap_or(' '))
                    .or_else(|| kana_monograph(next_c))
                {
                    if let Some(first_char) = next_rom.chars().next() {
                        out.push(first_char);
                    }
                }
            }
            i += 1;
            continue;
        }

        // Check 2-character Japanese Kana digraphs (e.g., きゃ -> kya)
        if i + 1 < chars.len() {
            if let Some(digraph) = kana_digraph(c, chars[i + 1]) {
                out.push_str(digraph);
                i += 2;
                continue;
            }
        }

        // Check single Japanese Kana monograph
        if let Some(mono) = kana_monograph(c) {
            out.push_str(mono);
            i += 1;
            continue;
        }

        // 3. Cyrillic
        if let Some(cyr) = romanize_cyrillic(c) {
            out.push_str(cyr);
            i += 1;
            continue;
        }

        // 4. Greek
        if let Some(grk) = romanize_greek(c) {
            out.push_str(grk);
            i += 1;
            continue;
        }

        // 5. Default Latin / ASCII / Punctuation
        out.push(c);
        i += 1;
    }

    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_korean_romanization() {
        assert_eq!(romanize_text("안녕"), Some("annyeong".to_string()));
        assert_eq!(romanize_text("사랑해"), Some("saranghae".to_string()));
        assert_eq!(romanize_text("케이팝 Music"), Some("keipap Music".to_string()));
    }

    #[test]
    fn test_japanese_kana_romanization() {
        assert_eq!(romanize_text("ありがとう"), Some("arigatou".to_string()));
        assert_eq!(romanize_text("とうきょう"), Some("toukyou".to_string()));
        assert_eq!(romanize_text("きっと"), Some("kitto".to_string()));
        assert_eq!(romanize_text("アニメ"), Some("anime".to_string()));
    }

    #[test]
    fn test_cyrillic_romanization() {
        assert_eq!(romanize_text("Привет"), Some("Privet".to_string()));
        assert_eq!(romanize_text("Москва"), Some("Moskva".to_string()));
    }

    #[test]
    fn test_ascii_no_op() {
        assert_eq!(romanize_text("Hello World! 123"), None);
    }
}
