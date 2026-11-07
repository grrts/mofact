//! Every word the player reads, in seven languages (T65).
//!
//! The strings live in `crates/ui/lang/<code>.txt`, one `key = text` per
//! line, with `{0}`, `{1}` … where numbers or names go. English is the
//! source: a key missing from another language falls back to English, so a
//! new string shows up in English until it is translated.
//!
//! Content names (items, recipes, buildings, research, goals) are English in
//! `data/*.ron`; the other languages translate them by id as `item.<id>`,
//! `recipe.<id>`, `building.<id>`, `stage.<building>.<n>`, `research.<id>`,
//! `goal.<id>.title` and `goal.<id>.hint`. [`localize`] makes a copy of the
//! content with those names swapped in, for the HUD to show.
//!
//! The language is per thread: the UI runs on one thread, and tests can each
//! pick their own without stepping on one another.

use mofact_core::content::{BuildingRole, Content};
use std::cell::Cell;
use std::collections::HashMap;
use std::fmt::Display;
use std::sync::OnceLock;

/// A language the game is translated into.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Lang {
    #[default]
    En,
    De,
    Fr,
    Es,
    PtBr,
    Ja,
    ZhHans,
}

impl Lang {
    /// Every language, in the order the settings button steps through them.
    pub const ALL: [Lang; 7] = [
        Lang::En,
        Lang::De,
        Lang::Fr,
        Lang::Es,
        Lang::PtBr,
        Lang::Ja,
        Lang::ZhHans,
    ];

    /// Its BCP 47 tag, as saved in settings and named in `lang/`.
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::De => "de",
            Lang::Fr => "fr",
            Lang::Es => "es",
            Lang::PtBr => "pt-BR",
            Lang::Ja => "ja",
            Lang::ZhHans => "zh-Hans",
        }
    }

    /// Its name in itself, for the settings button.
    pub fn native(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::De => "Deutsch",
            Lang::Fr => "Français",
            Lang::Es => "Español",
            Lang::PtBr => "Português (Brasil)",
            Lang::Ja => "日本語",
            Lang::ZhHans => "简体中文",
        }
    }

    /// The language saved under `code`.
    pub fn from_code(code: &str) -> Option<Lang> {
        Lang::ALL.into_iter().find(|l| l.code() == code)
    }

    /// The language for a device locale such as `de-AT`, `pt_PT`, `zh-Hans-CN`
    /// or `ja`: the closest one there is, or none for a language we don't have.
    pub fn from_locale(locale: &str) -> Option<Lang> {
        let lower = locale.trim().to_ascii_lowercase().replace('_', "-");
        let lang = lower.split(['-', '.', '@']).next().unwrap_or_default();
        Some(match lang {
            "en" => Lang::En,
            "de" => Lang::De,
            "fr" => Lang::Fr,
            "es" => Lang::Es,
            // Portuguese everywhere gets the Brazilian text: far closer than English.
            "pt" => Lang::PtBr,
            "ja" => Lang::Ja,
            // Simplified Chinese reads better than English for Traditional
            // readers too, until there is a Traditional table.
            "zh" => Lang::ZhHans,
            _ => return None,
        })
    }

    /// The first of the device's preferred locales that we have, or English.
    pub fn for_device<'a>(locales: impl IntoIterator<Item = &'a str>) -> Lang {
        locales
            .into_iter()
            .find_map(Lang::from_locale)
            .unwrap_or_default()
    }

    /// Whether it is written in Han characters, without spaces between words.
    pub fn is_cjk(self) -> bool {
        matches!(self, Lang::Ja | Lang::ZhHans)
    }

    fn source(self) -> &'static str {
        match self {
            Lang::En => include_str!("../lang/en.txt"),
            Lang::De => include_str!("../lang/de.txt"),
            Lang::Fr => include_str!("../lang/fr.txt"),
            Lang::Es => include_str!("../lang/es.txt"),
            Lang::PtBr => include_str!("../lang/pt-BR.txt"),
            Lang::Ja => include_str!("../lang/ja.txt"),
            Lang::ZhHans => include_str!("../lang/zh-Hans.txt"),
        }
    }

    fn index(self) -> usize {
        Lang::ALL.iter().position(|l| *l == self).unwrap_or(0)
    }
}

/// The player's choice: follow the device, or one language whatever it says.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Choice {
    #[default]
    Device,
    Fixed(Lang),
}

impl Choice {
    /// How it is saved: `auto` or a language code.
    pub fn code(self) -> &'static str {
        match self {
            Choice::Device => "auto",
            Choice::Fixed(l) => l.code(),
        }
    }

    /// The choice saved as `code`; anything unknown follows the device.
    pub fn from_code(code: &str) -> Choice {
        Lang::from_code(code.trim()).map_or(Choice::Device, Choice::Fixed)
    }

    /// The next one the settings button steps to: the device's, then each language.
    pub fn next(self) -> Choice {
        match self {
            Choice::Device => Choice::Fixed(Lang::ALL[0]),
            Choice::Fixed(l) => Lang::ALL
                .get(l.index() + 1)
                .map_or(Choice::Device, |n| Choice::Fixed(*n)),
        }
    }

    /// The language it comes to on a device that prefers `device`.
    pub fn resolve(self, device: Lang) -> Lang {
        match self {
            Choice::Device => device,
            Choice::Fixed(l) => l,
        }
    }
}

/// One table: key to text, for one language.
pub type Table = HashMap<&'static str, &'static str>;

/// Parse a `lang/*.txt` file: `key = text` lines; `#` starts a comment line.
pub fn parse(source: &'static str) -> Table {
    source
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.split_once(" = ").or_else(|| l.split_once('=')))
        .map(|(k, v)| (k.trim(), v.trim()))
        .collect()
}

/// The table for `lang`.
pub fn table(lang: Lang) -> &'static Table {
    static TABLES: OnceLock<Vec<Table>> = OnceLock::new();
    &TABLES.get_or_init(|| Lang::ALL.iter().map(|l| parse(l.source())).collect())[lang.index()]
}

thread_local! {
    static CURRENT: Cell<Lang> = const { Cell::new(Lang::En) };
}

/// The language the UI is in on this thread.
pub fn lang() -> Lang {
    CURRENT.with(Cell::get)
}

/// Switch this thread's UI to `lang`.
pub fn set_lang(lang: Lang) {
    CURRENT.with(|c| c.set(lang));
}

/// The text for `key` in `lang`, falling back to English, then to the key.
pub fn tr_in(lang: Lang, key: &str) -> &'static str {
    if let Some(t) = table(lang).get(key) {
        return t;
    }
    if let Some(t) = table(Lang::En).get(key) {
        return t;
    }
    debug_assert!(false, "no English text for {key:?}");
    "?"
}

/// The text for `key` in the current language.
pub fn tr(key: &str) -> &'static str {
    tr_in(lang(), key)
}

/// `key`'s text with `{0}`, `{1}` … filled in from `args`.
pub fn trf(key: &str, args: &[&dyn Display]) -> String {
    fill(tr(key), args)
}

/// `text` with `{0}`, `{1}` … filled in from `args`.
pub fn fill(text: &str, args: &[&dyn Display]) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}').and_then(|close| {
            let n: usize = after[..close].parse().ok()?;
            Some((n, close))
        }) {
            Some((n, close)) => {
                if let Some(a) = args.get(n) {
                    out.push_str(&a.to_string());
                }
                rest = &after[close + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// `names` as a list: "a, b, c" in English, "a、b、c" in Japanese.
pub fn join(names: &[&str]) -> String {
    let pattern = tr("list.join");
    let mut it = names.iter();
    let first = it.next().map(|s| s.to_string()).unwrap_or_default();
    it.fold(first, |acc, n| fill(pattern, &[&acc, n]))
}

/// A name as it reads inside a sentence: "burning crude oil", but German
/// keeps its capital nouns and Han script has no case.
pub fn in_sentence(name: &str) -> String {
    match lang() {
        Lang::De | Lang::Ja | Lang::ZhHans => name.to_string(),
        _ => name.to_lowercase(),
    }
}

/// Whether `c` is written full width, with no spaces between words: Han
/// characters, kana and their punctuation.
pub fn is_wide(c: char) -> bool {
    matches!(u32::from(c),
        0x3000..=0x30ff | 0x3400..=0x4dbf | 0x4e00..=0x9fff | 0xf900..=0xfaff | 0xff00..=0xffef)
}

/// Characters a line may not start with: closing marks, and the small kana
/// and long vowel mark that belong to the character before.
fn no_start(c: char) -> bool {
    "、。，．・：；！？）」』】〕ー～ぁぃぅぇぉっゃゅょゎァィゥェォッャュョヮヵヶ％,.:;!?)·/"
        .contains(c)
}

/// Characters a line may not end with: opening marks.
fn no_end(c: char) -> bool {
    "（「『【〔(".contains(c)
}

/// `text` cut into the pieces a line may break between, each with whether
/// a space comes before it. Spaces part words; Han script and kana break
/// between any two characters, except before a closing mark or after an
/// opening one.
pub fn breaks(text: &str) -> Vec<(&str, bool)> {
    let mut out = Vec::new();
    for word in text.split_whitespace() {
        let mut spaced = true;
        let mut start = 0;
        let mut prev: Option<char> = None;
        for (i, c) in word.char_indices() {
            if let Some(p) = prev {
                if (is_wide(c) || is_wide(p)) && !no_start(c) && !no_end(p) {
                    out.push((&word[start..i], spaced));
                    spaced = false;
                    start = i;
                }
            }
            prev = Some(c);
        }
        out.push((&word[start..], spaced));
    }
    out
}

/// The content with every name the player reads in the current language.
/// Ids and numbers stay as they are; a name with no translation stays English.
pub fn localize(content: &Content) -> Content {
    let lang = lang();
    let mut c = content.clone();
    if lang == Lang::En {
        return c;
    }
    let t = table(lang);
    let swap = |name: &mut String, key: String| {
        if let Some(text) = t.get(key.as_str()) {
            *name = (*text).to_string();
        }
    };
    for i in &mut c.items {
        swap(&mut i.name, format!("item.{}", i.id.as_str()));
    }
    for r in &mut c.recipes {
        swap(&mut r.name, format!("recipe.{}", r.id.as_str()));
    }
    for b in &mut c.buildings {
        let id = b.id.as_str().to_string();
        swap(&mut b.name, format!("building.{id}"));
        if let BuildingRole::Megaproject { stages } = &mut b.role {
            for (n, s) in stages.iter_mut().enumerate() {
                swap(&mut s.name, format!("stage.{id}.{n}"));
            }
        }
    }
    for r in &mut c.research {
        swap(&mut r.name, format!("research.{}", r.id.as_str()));
    }
    for g in &mut c.goals {
        swap(&mut g.title, format!("goal.{}.title", g.id));
        swap(&mut g.hint, format!("goal.{}.hint", g.id));
    }
    c
}

/// Every content key [`localize`] looks up, with its English text.
pub fn content_keys(content: &Content) -> Vec<(String, String)> {
    let mut keys = Vec::new();
    for i in &content.items {
        keys.push((format!("item.{}", i.id.as_str()), i.name.clone()));
    }
    for r in &content.recipes {
        keys.push((format!("recipe.{}", r.id.as_str()), r.name.clone()));
    }
    for b in &content.buildings {
        keys.push((format!("building.{}", b.id.as_str()), b.name.clone()));
        if let BuildingRole::Megaproject { stages } = &b.role {
            for (n, s) in stages.iter().enumerate() {
                keys.push((format!("stage.{}.{n}", b.id.as_str()), s.name.clone()));
            }
        }
    }
    for r in &content.research {
        keys.push((format!("research.{}", r.id.as_str()), r.name.clone()));
    }
    for g in &content.goals {
        keys.push((format!("goal.{}.title", g.id), g.title.clone()));
        keys.push((format!("goal.{}.hint", g.id), g.hint.clone()));
    }
    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_locales_pick_the_closest_language() {
        assert_eq!(Lang::from_locale("de-AT"), Some(Lang::De));
        assert_eq!(Lang::from_locale("pt_PT"), Some(Lang::PtBr));
        assert_eq!(Lang::from_locale("zh-Hans-CN"), Some(Lang::ZhHans));
        assert_eq!(Lang::from_locale("ja"), Some(Lang::Ja));
        assert_eq!(Lang::from_locale("nl-NL"), None);
        assert_eq!(Lang::for_device(["nl-NL", "fr-CA", "en"]), Lang::Fr);
        assert_eq!(Lang::for_device(["ko"]), Lang::En);
    }

    #[test]
    fn the_choice_steps_through_every_language_and_back() {
        let mut c = Choice::Device;
        let mut seen = Vec::new();
        for _ in 0..Lang::ALL.len() {
            c = c.next();
            seen.push(c.resolve(Lang::En));
        }
        assert_eq!(seen, Lang::ALL);
        assert_eq!(c.next(), Choice::Device);
        assert_eq!(
            Choice::from_code(Choice::Fixed(Lang::PtBr).code()),
            Choice::Fixed(Lang::PtBr)
        );
        assert_eq!(Choice::from_code("auto"), Choice::Device);
    }

    #[test]
    fn han_text_breaks_between_characters_but_not_before_closing_marks() {
        let pieces: Vec<&str> = breaks("鉄板を50枚、届ける。 OK")
            .iter()
            .map(|p| p.0)
            .collect();
        assert_eq!(
            pieces,
            ["鉄", "板", "を", "50", "枚、", "届", "け", "る。", "OK"]
        );
        assert_eq!(breaks("Two words"), [("Two", true), ("words", true)]);
    }

    #[test]
    fn placeholders_fill_in_any_order() {
        assert_eq!(fill("{1} of {0}", &[&3, &"ten"]), "ten of 3");
        assert_eq!(fill("{x} {0}", &[&1]), "{x} 1");
    }

    /// The `{n}` placeholders in `text`, sorted.
    fn holes(text: &str) -> Vec<usize> {
        let mut out = Vec::new();
        let mut rest = text;
        while let Some(i) = rest.find('{') {
            rest = &rest[i + 1..];
            if let Some(n) = rest.find('}').and_then(|j| rest[..j].parse().ok()) {
                out.push(n);
            }
        }
        out.sort_unstable();
        out
    }

    #[test]
    fn every_language_has_every_string_with_the_same_placeholders() {
        let en = table(Lang::En);
        let mut wrong = Vec::new();
        for lang in Lang::ALL {
            let t = table(lang);
            for (key, text) in en {
                match t.get(key) {
                    None => wrong.push(format!("{}: missing {key}", lang.code())),
                    Some(tx) if holes(tx) != holes(text) => {
                        wrong.push(format!("{}: {key} has {tx:?}", lang.code()))
                    }
                    _ => {}
                }
            }
            for key in t.keys() {
                let content = [
                    "item.",
                    "recipe.",
                    "building.",
                    "stage.",
                    "research.",
                    "goal.",
                ];
                if !en.contains_key(key) && !content.iter().any(|p| key.starts_with(p)) {
                    wrong.push(format!("{}: {key} isn't an English key", lang.code()));
                }
            }
        }
        wrong.sort();
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }
}
