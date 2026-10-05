//! Scoring core of `blabber-bench`: text normalisation, alignment, error rates, term and number
//! metrics and bootstrap statistics.
//!
//! Normalisation `blabber-norm-v1` ([`NORMALISATION_VERSION`]) treats reference and hypothesis
//! identically: NFC → lowercase → symbols → numbers spelled out → word tokens → language rules →
//! filler removal.
//! - Numbers are spelled in the language of the neighbouring words (stopword vote over up to six
//!   words before, then after, preferring the same sentence; else the first language). Thousands
//!   separators: de/fr `.` and space, en `,`, NBSP in any language. Decimals: de "komma", fr
//!   "virgule", en "point"; a two-digit fraction without leading zero is read as a number (`3,20`
//!   → "trois virgule vingt", `4.12` → "four point twelve"), other fractions digit by digit
//!   (`0,05` → "null komma null fünf"); de `2.3` (fraction not three digits) → "zwei punkt drei"
//!   unless a unit or magnitude follows (`8.5 %` → "acht komma fünf prozent").
//!   Times: de "neun uhr dreißig", en "two thirteen p m" (`:00` dropped, `:05` → "oh five"), fr
//!   "quatorze heure trente". Ordinals: de `15.` before a non-function word → "fünfzehnte" (spelled
//!   ordinals lose their inflection), en `24th`, fr `2e`. en years: plain 1100–2099 except x00 and
//!   2000–2009, unless a symbol, unit or 4+ letter word ending in "s" follows → "twenty twenty
//!   six". Digit runs with a leading zero or beyond 999,999,999,999 are read digit by digit.
//!   German number words are regenerated canonically ("hundert" → "einhundert", "ein million" →
//!   "eine million"); "and" after hundred/thousand/million/billion before a number is dropped.
//! - Hyphens join compounds ("EBITDA-Marge" → "ebitdamarge") unless a part is a number word
//!   ("twenty-six" → "twenty six"); letter–digit tokens split ("Q1" → "q eins").
//! - en contractions expand ("don't" → "do not"), other apostrophes join ("today's" → "todays");
//!   fr elisions split ("l'unité" → "l unité"), accents are kept; umlauts fold (ä → ae, ß → ss)
//!   whenever German is among the languages.
//! - Units, currencies, magnitudes and abbreviations map to one lemma (`mm`/"Millimetern" →
//!   "millimeter", "dollars" → "dollar", "Mio." → "million", "Dr." → "doctor", `%` → "prozent" /
//!   "percent" / "pour cent").
//! - Fillers are dropped (äh ähm öh öhm hm hmm uh uhm erm euh, en "um"/"mm", fr "hum"); "um" is
//!   kept when German is possible.
//!
//! [`score`] also reconciles compounds: hypothesis tokens are merged or split to match the
//! reference segmentation ("pull request" ↔ "pullrequest"); the reference is never changed.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::{HashMap, HashSet};
use unicode_normalization::UnicodeNormalization;

pub const NORMALISATION_VERSION: &str = "blabber-norm-v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    De,
    En,
    Fr,
}

impl Lang {
    /// "de"/"german"/"deutsch", "en"/"english", "fr"/"french"/"français", also "de-DE" style.
    pub fn from_code(code: &str) -> Option<Lang> {
        let lower = code.trim().nfc().collect::<String>().to_lowercase();
        match lower.split(['-', '_']).next().unwrap_or("") {
            "de" | "deu" | "ger" | "german" | "deutsch" => Some(Lang::De),
            "en" | "eng" | "english" | "englisch" => Some(Lang::En),
            "fr" | "fra" | "fre" | "french" | "français" | "francais" => Some(Lang::Fr),
            _ => None,
        }
    }

    pub fn code(self) -> &'static str {
        pick(self, "de", "en", "fr")
    }
}

/// Reference text: plain (languages given separately) or per-sentence segments with known language.
#[derive(Clone, Copy, Debug)]
pub enum ReferenceText<'a> {
    Plain(&'a str),
    Segments(&'a [(Lang, String)]),
}

// MARK: Normalisation

/// Language-specific token rules that are on (umlauts, contractions, elisions, fillers).
#[derive(Clone, Copy, Debug, Default)]
struct Features {
    de: bool,
    en: bool,
    fr: bool,
}

struct Ctx {
    /// Candidate languages for spelling numbers.
    langs: Vec<Lang>,
    feat: Features,
}

impl Ctx {
    fn new(langs: &[Lang]) -> Ctx {
        let mut unique: Vec<Lang> = Vec::new();
        for lang in langs {
            if !unique.contains(lang) {
                unique.push(*lang);
            }
        }
        if unique.is_empty() {
            unique = vec![Lang::De, Lang::En];
        }
        let has = |l| unique.contains(&l);
        let feat = Features {
            de: has(Lang::De),
            en: has(Lang::En),
            fr: has(Lang::Fr),
        };
        Ctx {
            langs: unique,
            feat,
        }
    }
}

/// Normalised scoring tokens; digit runs are spelled in the language of the neighbouring words.
pub fn normalise(text: &str, langs: &[Lang]) -> Vec<String> {
    normalise_ctx(text, &Ctx::new(langs)).0
}

/// Reference with known per-segment language: each segment normalised in its own language.
pub fn normalise_segments(segments: &[(Lang, String)]) -> Vec<String> {
    let langs: Vec<Lang> = segments.iter().map(|(lang, _)| *lang).collect();
    segments_ctx(segments, Ctx::new(&langs).feat).0
}

/// Tokens and numeric entities (each digit expression, normalised).
type Normalised = (Vec<String>, Vec<Vec<String>>);

fn segments_ctx(segments: &[(Lang, String)], feat: Features) -> Normalised {
    let (mut tokens, mut entities) = (Vec::new(), Vec::new());
    for (lang, text) in segments {
        let (t, e) = normalise_ctx(
            text,
            &Ctx {
                langs: vec![*lang],
                feat,
            },
        );
        tokens.extend(t);
        entities.extend(e);
    }
    (tokens, entities)
}

fn normalise_ctx(text: &str, ctx: &Ctx) -> Normalised {
    let lexemes = lex(text);
    let (pieces, entities) = Conv::run(&lexemes, ctx);
    (finish(assemble(&pieces, ctx.feat), ctx.feat), entities)
}

#[derive(Clone, Debug, PartialEq)]
enum Lx {
    Word(String),
    Num(String),
    Sym(char),
    /// 0 regular, 1 non-breaking only, 2 contains a line break.
    Space(u8),
}

fn lex(text: &str) -> Vec<Lx> {
    let chars: Vec<char> = text
        .nfc()
        .collect::<String>()
        .to_lowercase()
        .chars()
        .collect();
    let space_kind = |c: char| match c {
        '\n' | '\r' | '\u{2028}' | '\u{2029}' => 2,
        '\u{a0}' | '\u{202f}' | '\u{2007}' | '\u{2009}' => 1,
        _ => 0,
    };
    let word_char = |c: char| c.is_alphanumeric() && !c.is_ascii_digit();
    let (mut out, mut i) = (Vec::new(), 0);
    while i < chars.len() {
        let (c, start) = (chars[i], i);
        i += 1;
        let mut run = |f: &dyn Fn(char) -> bool| {
            while i < chars.len() && f(chars[i]) {
                i += 1;
            }
            chars[start..i].iter().collect::<String>()
        };
        out.push(if c.is_whitespace() {
            let kinds = run(&|c| c.is_whitespace())
                .chars()
                .map(space_kind)
                .collect::<Vec<_>>();
            let kind = if kinds.contains(&2) {
                2
            } else {
                kinds.into_iter().min().unwrap_or(0)
            };
            Lx::Space(kind)
        } else if c.is_ascii_digit() {
            Lx::Num(run(&|c| c.is_ascii_digit()))
        } else if word_char(c) {
            Lx::Word(run(&word_char))
        } else {
            Lx::Sym(match c {
                '’' | '‘' | 'ʼ' | '`' | '´' => '\'',
                '‐' | '‑' | '−' => '-',
                _ => c,
            })
        });
    }
    out
}

/// Number-stage output: words (`true` = generated, never joined), joiners ('-', '\''), breaks.
enum Pc {
    W(String, bool),
    Join(char),
    Break,
}

/// Number stage: spells digit expressions and symbols, recording numeric entities.
struct Conv<'a> {
    lx: &'a [Lx],
    ctx: &'a Ctx,
    votes: Vec<Option<Lang>>,
    out: Vec<Pc>,
    entities: Vec<Vec<String>>,
}

impl<'a> Conv<'a> {
    fn run(lx: &'a [Lx], ctx: &'a Ctx) -> (Vec<Pc>, Vec<Vec<String>>) {
        let vote = |l: &Lx| match l {
            Lx::Word(w) if ctx.langs.len() > 1 => stopword_lang(w, &ctx.langs),
            _ => None,
        };
        let votes = lx.iter().map(vote).collect();
        let mut conv = Conv {
            lx,
            ctx,
            votes,
            out: Vec::new(),
            entities: Vec::new(),
        };
        let mut i = 0;
        while i < lx.len() {
            i = match &lx[i] {
                Lx::Num(_) => conv.number(i, None, None),
                Lx::Sym(c) => conv.symbol(i, *c),
                Lx::Word(w) => {
                    conv.out.push(Pc::W(w.clone(), false));
                    i + 1
                }
                Lx::Space(_) => {
                    conv.out.push(Pc::Break);
                    i + 1
                }
            };
        }
        (conv.out, conv.entities)
    }

    fn word(&self, i: usize) -> Option<&'a str> {
        match self.lx.get(i) {
            Some(Lx::Word(w)) => Some(w),
            _ => None,
        }
    }

    fn num(&self, i: usize) -> Option<&'a str> {
        match self.lx.get(i) {
            Some(Lx::Num(n)) => Some(n),
            _ => None,
        }
    }

    /// Value of a digit run of `min..=max` digits.
    fn num_value(&self, i: usize, min: usize, max: usize) -> Option<u64> {
        self.num(i)
            .filter(|s| (min..=max).contains(&s.len()))?
            .parse()
            .ok()
    }

    fn sym(&self, i: usize, c: char) -> bool {
        self.lx.get(i) == Some(&Lx::Sym(c))
    }

    /// Skips one same-line space.
    fn gap(&self, i: usize) -> usize {
        i + usize::from(matches!(self.lx.get(i), Some(Lx::Space(0 | 1))))
    }

    fn lang_at(&self, i: usize) -> Lang {
        let langs = &self.ctx.langs;
        if langs.len() == 1 {
            return langs[0];
        }
        let boundary = |k: usize| match self.lx[k] {
            Lx::Sym('.' | '!' | '?' | '…') => {
                matches!(self.lx.get(k + 1), None | Some(Lx::Space(_)))
            }
            Lx::Space(kind) => kind == 2,
            _ => false,
        };
        let vote = |range: &mut dyn Iterator<Item = usize>, sentence: bool| {
            let words = range
                .take_while(|k| !sentence || !boundary(*k))
                .filter(|k| matches!(self.lx[*k], Lx::Word(_)));
            tally(words.take(6).map(|k| self.votes[k]), langs)
        };
        let len = self.lx.len();
        vote(&mut (0..i).rev(), true)
            .or_else(|| vote(&mut (i + 1..len), true))
            .or_else(|| vote(&mut (0..i).rev(), false))
            .or_else(|| vote(&mut (i + 1..len), false))
            .unwrap_or(langs[0])
    }

    fn push_fixed(&mut self, text: &str) {
        self.out
            .extend(text.split_whitespace().map(|w| Pc::W(w.to_string(), true)));
    }

    fn symbol(&mut self, i: usize, c: char) -> usize {
        let lang = self.lang_at(i);
        let prev = i.checked_sub(1).map(|k| &self.lx[k]);
        let after_term = matches!(prev, Some(Lx::Word(_) | Lx::Num(_)));
        let text = match c {
            '$' | '€' | '£' if self.num(self.gap(i + 1)).is_some() => {
                return self.number(self.gap(i + 1), None, Some(c));
            }
            '-' | '+' if !after_term && self.num(i + 1).is_some() => {
                let lead = if c == '+' {
                    "plus"
                } else {
                    pick(lang, "minus", "minus", "moins")
                };
                return self.number(i + 1, Some(lead), None);
            }
            '$' | '€' | '£' => currency_word(c),
            '%' => percent_word(lang),
            '&' => pick(lang, "und", "and", "et"),
            '±' => pick(lang, "plus minus", "plus minus", "plus ou moins"),
            '§' => pick(lang, "paragraf", "section", "paragraphe"),
            '°' => {
                let k = self.gap(i + 1);
                let scale = match self.word(k) {
                    Some("c") => "celsius",
                    Some("f") => "fahrenheit",
                    _ => "",
                };
                self.push_fixed(&format!("degree {scale}"));
                return if scale.is_empty() { i + 1 } else { k + 1 };
            }
            '-' | '\'' => {
                self.out.push(Pc::Join(c));
                return i + 1;
            }
            _ => {
                self.out.push(Pc::Break);
                return i + 1;
            }
        };
        self.push_fixed(text);
        i + 1
    }

    /// Spells the numeric expression starting at digit run `lx[i]`; returns the next index.
    fn number(&mut self, i: usize, lead: Option<&str>, currency: Option<char>) -> usize {
        let lang = self.lang_at(i);
        let (core, mut end, unit_ok) = match self.time(i, lang).or_else(|| self.date(i, lang)) {
            Some((w, e)) => (w, e, false),
            None => self.amount(i, lang, currency.is_some()),
        };
        let mut words: Vec<String> = lead.map(String::from).into_iter().chain(core).collect();
        let k = self.gap(end);
        if let Some(unit) = self.word(k).and_then(unit_abbreviation).filter(|_| unit_ok) {
            words.push(unit.to_string());
            end = k + 1;
        }
        if let Some(c) = currency {
            let k = self.gap(end);
            if let Some(m) = self.word(k).and_then(magnitude) {
                words.push(m.to_string());
                end = k + 1;
            }
            words.push(currency_word(c).to_string());
        }
        let mut entity = Vec::new();
        if lead.is_none() && currency.is_none() {
            entity.extend(
                i.checked_sub(1)
                    .and_then(|k| self.word(k))
                    .map(String::from),
            );
        }
        entity.extend(words.iter().cloned());
        if currency.is_none() {
            entity.extend(self.unit_follow(end, lang));
        }
        self.entities.push(finish(entity, self.ctx.feat));
        self.out.extend(words.into_iter().map(|w| Pc::W(w, true)));
        end.max(i + 1)
    }

    /// Integer with thousands separators, decimals and suffixes: (words, end, units allowed).
    fn amount(&self, i: usize, lang: Lang, currency: bool) -> (Vec<String>, usize, bool) {
        let (digits, mut end) = self.grouped(i, lang);
        let mut core = spell_int(&digits, lang);
        let mut fraction = false;
        while let (Some(Lx::Sym(sep)), Some(frac)) = (self.lx.get(end), self.num(end + 1)) {
            let word = match (lang, *sep) {
                (Lang::En, '.') => "point",
                (Lang::De, ',') if !fraction => "komma",
                (Lang::Fr, ',') if !fraction => "virgule",
                // "8.5 Prozent" is a decimal written English-style, "2.3" alone a version.
                (Lang::De | Lang::Fr, '.') if !fraction && self.amount_follows(end + 2, lang) => {
                    pick(lang, "komma", "", "virgule")
                }
                (Lang::De, '.') => "punkt",
                (Lang::Fr, '.') => "point",
                _ => break,
            };
            core.push(word.to_string());
            core.extend(fraction_words(frac, lang));
            end += 2;
            fraction = true;
        }
        let first = self.num(i).unwrap_or("");
        let Some(n) = parse_plain(first).filter(|_| end == i + 1) else {
            return (core, end, true);
        };
        let next = self.word(end);
        let fr_suffixes = ["er", "re", "ère", "e", "ème", "eme", "è", "ieme", "ième"];
        if (lang == Lang::En && matches!(next, Some("st" | "nd" | "rd" | "th")))
            || (lang == Lang::Fr && next.is_some_and(|w| fr_suffixes.contains(&w)))
        {
            let premiere = lang == Lang::Fr && n == 1 && matches!(next, Some("re" | "ère"));
            let w = if premiere {
                words("première")
            } else {
                ordinal(n, lang)
            };
            return (w, end + 1, false);
        }
        let glued = i.checked_sub(1).is_some_and(|k| self.word(k).is_some());
        let dot_ordinal = first.len() <= 3
            && !glued
            && self.sym(end, '.')
            && matches!(self.lx.get(end + 1), Some(Lx::Space(0 | 1)))
            && self.word(end + 2).is_some_and(|w| !is_de_starter(w));
        if lang == Lang::De && dot_ordinal {
            return (ordinal(n, lang), end + 1, false);
        }
        if lang == Lang::En
            && first.len() == 4
            && is_year(n)
            && !currency
            && !self.year_blocked(end)
        {
            return (en_year(n), end, true);
        }
        if lang == Lang::Fr && n <= 24 && self.word(self.gap(end)) == Some("h") {
            core.push("heure".to_string());
            end = self.gap(end) + 1;
            let k = self.gap(end);
            if let Some(m) = self.num_value(k, 2, 2).filter(|m| *m < 60) {
                core.extend(cardinal(m, lang).into_iter().filter(|_| m > 0));
                end = k + 1;
            }
            return (core, end, false);
        }
        match self.ampm(end).filter(|_| lang == Lang::En && n <= 24) {
            Some((ap, e)) => (core.into_iter().chain(ap).collect(), e, false),
            None => (core, end, true),
        }
    }

    /// Integer digits with thousands separators merged, and the index after them.
    fn grouped(&self, i: usize, lang: Lang) -> (String, usize) {
        let mut digits = self.num(i).unwrap_or("").to_string();
        let mut end = i + 1;
        if digits.len() > 3 {
            return (digits, end);
        }
        loop {
            let separator = match self.lx.get(end) {
                Some(Lx::Sym('.')) => lang != Lang::En,
                Some(Lx::Sym(',')) => lang == Lang::En,
                Some(Lx::Space(1)) => true,
                Some(Lx::Space(0)) => lang != Lang::En,
                _ => false,
            };
            match self.num(end + 1) {
                Some(group) if separator && group.len() == 3 => {
                    digits.push_str(group);
                    end += 2;
                }
                _ => return (digits, end),
            }
        }
    }

    fn time(&self, i: usize, lang: Lang) -> Option<(Vec<String>, usize)> {
        let h = self.num_value(i, 1, 2).filter(|h| *h <= 24)?;
        let m = self.num_value(i + 2, 2, 2).filter(|m| *m < 60)?;
        let colon = self.sym(i + 1, ':');
        if !(colon || lang == Lang::De && self.sym(i + 1, '.')) {
            return None;
        }
        let mut end = i + 3;
        if lang == Lang::De && self.word(self.gap(end)) == Some("uhr") {
            end = self.gap(end) + 1;
        } else if !colon {
            return None;
        }
        let minutes = match (lang, m) {
            (_, 0) => String::new(),
            (Lang::En, 1..=9) => format!("oh {}", en_cardinal(m)),
            _ => cardinal(m, lang).join(" "),
        };
        let hour = cardinal(h, lang).join(" ");
        let mut w = words(&format!(
            "{hour} {} {minutes}",
            pick(lang, "uhr", "", "heure")
        ));
        if let Some((ap, e)) = self.ampm(end).filter(|_| lang == Lang::En) {
            w.extend(ap);
            end = e;
        }
        Some((w, end))
    }

    /// German `15.11.2025`, or `15.11.` after a date preposition.
    fn date(&self, i: usize, lang: Lang) -> Option<(Vec<String>, usize)> {
        if lang != Lang::De || !self.sym(i + 1, '.') || !self.sym(i + 3, '.') {
            return None;
        }
        let day = self.num_value(i, 1, 2).filter(|d| (1..=31).contains(d))?;
        let month = self
            .num_value(i + 2, 1, 2)
            .filter(|m| (1..=12).contains(m))?;
        let year = self.num(i + 4).filter(|y| y.len() == 2 || y.len() == 4);
        let prev = i.checked_sub(2).and_then(|k| self.word(k));
        let preposition = matches!(
            prev,
            Some("am" | "vom" | "zum" | "bis" | "ab" | "den" | "dem" | "seit")
        );
        if year.is_none() && !preposition {
            return None;
        }
        let mut w = ordinal(day, lang);
        w.extend(ordinal(month, lang));
        w.extend(year.map(|y| spell_int(y, lang)).unwrap_or_default());
        Some((w, i + 4 + usize::from(year.is_some())))
    }

    fn ampm(&self, end: usize) -> Option<(Vec<String>, usize)> {
        let k = self.gap(end);
        let (letter, next) = match self.word(k)? {
            "am" => ("a", k + 1),
            "pm" => ("p", k + 1),
            w @ ("a" | "p") if self.sym(k + 1, '.') && self.word(k + 2) == Some("m") => {
                (w, k + 3 + usize::from(self.sym(k + 3, '.')))
            }
            _ => return None,
        };
        Some((vec![letter.to_string(), "m".to_string()], next))
    }

    fn amount_follows(&self, k: usize, lang: Lang) -> bool {
        self.word(self.gap(k)).and_then(magnitude).is_some()
            || !self.unit_follow(k, lang).is_empty()
    }

    fn year_blocked(&self, end: usize) -> bool {
        match self.lx.get(self.gap(end)) {
            Some(Lx::Sym('%' | '€' | '$' | '£' | '°')) => true,
            Some(Lx::Word(w)) => {
                (w.chars().count() >= 4 && w.ends_with('s')) || unit_abbreviation(w).is_some()
            }
            _ => false,
        }
    }

    /// Unit-like word right after a number, added to its numeric entity (not consumed).
    fn unit_follow(&self, end: usize, lang: Lang) -> Vec<String> {
        let k = self.gap(end);
        let next_word = self.word(self.gap(k + 1));
        match self.lx.get(k) {
            Some(Lx::Sym('%')) => words(percent_word(lang)),
            Some(Lx::Sym(c @ ('€' | '$' | '£'))) => words(currency_word(*c)),
            Some(Lx::Sym('°')) if next_word == Some("c") => words("degree celsius"),
            Some(Lx::Sym('°')) => words("degree"),
            Some(Lx::Word(w)) if w == "pour" && next_word == Some("cent") => words("pour cent"),
            Some(Lx::Word(w)) => {
                let canon = canon_token(w, self.ctx.feat);
                let unit = canon.len() == 1 && listed(UNIT_LIKE, &canon[0]);
                if unit {
                    canon
                } else {
                    Vec::new()
                }
            }
            _ => Vec::new(),
        }
    }
}

fn tally(votes: impl Iterator<Item = Option<Lang>>, langs: &[Lang]) -> Option<Lang> {
    let mut counts = [0usize; 3];
    for lang in votes.flatten() {
        counts[lang as usize] += 1;
    }
    let best = langs.iter().map(|l| counts[*l as usize]).max()?;
    let mut leaders = langs.iter().filter(|l| counts[**l as usize] == best);
    let lang = *leaders.next()?;
    (best > 0 && leaders.next().is_none()).then_some(lang)
}

fn pick(lang: Lang, de: &'static str, en: &'static str, fr: &'static str) -> &'static str {
    match lang {
        Lang::De => de,
        Lang::En => en,
        Lang::Fr => fr,
    }
}

fn percent_word(lang: Lang) -> &'static str {
    pick(lang, "prozent", "percent", "pour cent")
}

fn currency_word(c: char) -> &'static str {
    match c {
        '€' => "euro",
        '£' => "pound",
        _ => "dollar",
    }
}

/// Unit abbreviations, recognised only right after a number ("mm" is otherwise a filler).
fn unit_abbreviation(word: &str) -> Option<&'static str> {
    Some(match word {
        "mm" => "millimeter",
        "cm" => "centimeter",
        "km" => "kilometer",
        "kg" => "kilogram",
        "ms" => "millisecond",
        "min" => "minute",
        "tb" => "terabyte",
        "gb" => "gigabyte",
        "mb" => "megabyte",
        "kb" => "kilobyte",
        "hz" => "hertz",
        "khz" => "kilohertz",
        "mhz" => "megahertz",
        "ghz" => "gigahertz",
        _ => return None,
    })
}

/// Magnitude after a currency-prefixed amount ("$4.2m").
fn magnitude(word: &str) -> Option<&'static str> {
    Some(match word {
        "k" | "thousand" | "tausend" | "mille" => "thousand",
        "m" | "mn" | "mio" | "million" | "millions" | "millionen" => "million",
        "bn" | "billion" | "billions" => "billion",
        "mrd" | "milliarde" | "milliarden" | "milliard" | "milliards" => "milliarde",
        _ => return None,
    })
}

const UNIT_LIKE: &str = "euro dollar pound prozent percent uhr heure million milliarde billion \
    millimeter centimeter kilometer meter kilogram millisecond minute terabyte gigabyte megabyte \
    kilobyte degree quadratmeter hertz";

// MARK: Number words

// German number words are generated already umlaut-folded (German implies folding).
const DE_UNITS: &str = "null eins zwei drei vier fuenf sechs sieben acht neun zehn elf zwoelf \
    dreizehn vierzehn fuenfzehn sechzehn siebzehn achtzehn neunzehn";
const DE_TENS: &str = "- - zwanzig dreissig vierzig fuenfzig sechzig siebzig achtzig neunzig";
const EN_UNITS: &str = "zero one two three four five six seven eight nine ten eleven twelve \
    thirteen fourteen fifteen sixteen seventeen eighteen nineteen";
const EN_TENS: &str = "- - twenty thirty forty fifty sixty seventy eighty ninety";
const EN_SCALES: &str = "hundred thousand million billion";
const FR_UNITS: &str = "zéro un deux trois quatre cinq six sept huit neuf dix onze douze treize \
    quatorze quinze seize";
const FR_TENS: &str = "- - vingt trente quarante cinquante soixante";
const FR_OTHER: &str = "vingts cent cents mille million millions milliard milliards une";

fn words(text: &str) -> Vec<String> {
    text.split_whitespace().map(String::from).collect()
}

fn listed(list: &str, word: &str) -> bool {
    list.split(' ').any(|w| w == word)
}

fn nth(list: &'static str, i: u64) -> &'static str {
    list.split(' ').nth(i as usize).unwrap_or("")
}

fn index_in(list: &str, word: &str) -> Option<u64> {
    list.split(' ').position(|w| w == word).map(|p| p as u64)
}

fn parse_plain(digits: &str) -> Option<u64> {
    let leading_zero = digits.len() > 1 && digits.starts_with('0');
    if digits.is_empty() || digits.len() > 12 || leading_zero {
        return None;
    }
    digits.parse().ok()
}

/// Integer digit string → words; leading zeros and values beyond 10^12 are read digit by digit.
fn spell_int(digits: &str, lang: Lang) -> Vec<String> {
    match parse_plain(digits) {
        Some(n) => cardinal(n, lang),
        None => digit_words(digits, lang),
    }
}

fn digit_words(digits: &str, lang: Lang) -> Vec<String> {
    let digits = digits.chars().filter_map(|c| c.to_digit(10));
    digits.flat_map(|d| cardinal(d.into(), lang)).collect()
}

fn fraction_words(digits: &str, lang: Lang) -> Vec<String> {
    if digits.len() == 2 && !digits.starts_with('0') {
        spell_int(digits, lang)
    } else {
        digit_words(digits, lang)
    }
}

fn cardinal(n: u64, lang: Lang) -> Vec<String> {
    words(&match lang {
        Lang::De => de_cardinal(n),
        Lang::En => en_cardinal(n),
        Lang::Fr => fr_cardinal(n),
    })
}

fn rest(n: u64, spell: fn(u64) -> String) -> String {
    if n > 0 {
        spell(n)
    } else {
        String::new()
    }
}

/// One compound word below one million ("dreitausendvierhundertachtzig").
fn de_compound(n: u64) -> String {
    match n {
        0..=19 => nth(DE_UNITS, n).to_string(),
        20..=99 => match n % 10 {
            0 => nth(DE_TENS, n / 10).to_string(),
            1 => format!("einund{}", nth(DE_TENS, n / 10)),
            u => format!("{}und{}", nth(DE_UNITS, u), nth(DE_TENS, n / 10)),
        },
        100..=999 => {
            let head = if n < 200 {
                "ein"
            } else {
                nth(DE_UNITS, n / 100)
            };
            format!("{head}hundert{}", rest(n % 100, de_compound))
        }
        _ => {
            let head = de_compound(n / 1000 % 1000);
            let head = head
                .strip_suffix("eins")
                .map_or(head.clone(), |h| format!("{h}ein"));
            format!("{head}tausend{}", rest(n % 1000, de_compound))
        }
    }
}

fn de_cardinal(n: u64) -> String {
    let head = |k: u64| {
        if k == 1 {
            "eine".to_string()
        } else {
            de_compound(k)
        }
    };
    let (millions, rest_m) = (n / 1_000_000, n % 1_000_000);
    match n {
        0..=999_999 => de_compound(n),
        1_000_000..=999_999_999 => {
            format!("{} million {}", head(millions), rest(rest_m, de_cardinal))
        }
        _ => {
            let (billions, rest_b) = (n / 1_000_000_000, n % 1_000_000_000);
            format!("{} milliarde {}", head(billions), rest(rest_b, de_cardinal))
        }
    }
}

fn en_cardinal(n: u64) -> String {
    let (k, scale) = match n {
        0..=19 => return nth(EN_UNITS, n).to_string(),
        20..=99 => return format!("{} {}", nth(EN_TENS, n / 10), rest(n % 10, en_cardinal)),
        100..=999 => (100, "hundred"),
        1000..=999_999 => (1000, "thousand"),
        1_000_000..=999_999_999 => (1_000_000, "million"),
        _ => (1_000_000_000, "billion"),
    };
    format!(
        "{} {scale} {}",
        en_cardinal(n / k),
        rest(n % k, en_cardinal)
    )
}

fn fr_cardinal(n: u64) -> String {
    let head = |k: u64| if k > 1 { fr_cardinal(k) } else { String::new() };
    match n {
        0..=16 => nth(FR_UNITS, n).to_string(),
        17..=19 => format!("dix {}", nth(FR_UNITS, n - 10)),
        20..=69 if n % 10 == 1 => format!("{} et un", nth(FR_TENS, n / 10)),
        20..=69 => format!("{} {}", nth(FR_TENS, n / 10), rest(n % 10, fr_cardinal)),
        71 => "soixante et onze".to_string(),
        70..=79 => format!("soixante {}", fr_cardinal(n - 60)),
        80..=99 => format!("quatre vingt {}", rest(n - 80, fr_cardinal)),
        100..=999 => format!("{} cent {}", head(n / 100), rest(n % 100, fr_cardinal)),
        1000..=999_999 => format!("{} mille {}", head(n / 1000), rest(n % 1000, fr_cardinal)),
        1_000_000..=999_999_999 => {
            format!(
                "{} million {}",
                fr_cardinal(n / 1_000_000),
                rest(n % 1_000_000, fr_cardinal)
            )
        }
        _ => {
            let (billions, rest_b) = (n / 1_000_000_000, n % 1_000_000_000);
            format!(
                "{} milliard {}",
                fr_cardinal(billions),
                rest(rest_b, fr_cardinal)
            )
        }
    }
}

fn ordinal(n: u64, lang: Lang) -> Vec<String> {
    if lang == Lang::De {
        return words(&de_ordinal(n));
    }
    let mut w = cardinal(n, lang);
    if let Some(last) = w.last_mut() {
        *last = match (lang, last.as_str()) {
            (Lang::En, "one") => "first".to_string(),
            (Lang::En, "two") => "second".to_string(),
            (Lang::En, "three") => "third".to_string(),
            (Lang::En, "five") => "fifth".to_string(),
            (Lang::En, "eight") => "eighth".to_string(),
            (Lang::En, "nine") => "ninth".to_string(),
            (Lang::En, "twelve") => "twelfth".to_string(),
            (Lang::En, x) => x
                .strip_suffix('y')
                .map_or(format!("{x}th"), |s| format!("{s}ieth")),
            (_, _) if n == 1 => "premier".to_string(),
            (_, "cinq") => "cinquième".to_string(),
            (_, "neuf") => "neuvième".to_string(),
            (_, x) => format!("{}ième", x.strip_suffix('e').unwrap_or(x)),
        };
    }
    w
}

fn de_ordinal(n: u64) -> String {
    let small = n % 100;
    if n >= 1_000_000 || (n > 0 && (small == 0 || small >= 20)) {
        return format!("{}ste", de_cardinal(n).trim_end());
    }
    let head = if n >= 100 {
        de_compound(n - small)
    } else {
        String::new()
    };
    let tail = match small {
        1 => "erste".to_string(),
        3 => "dritte".to_string(),
        7 => "siebte".to_string(),
        8 => "achte".to_string(),
        _ => format!("{}te", nth(DE_UNITS, small)),
    };
    head + &tail
}

fn is_year(n: u64) -> bool {
    (1100..=2099).contains(&n) && !n.is_multiple_of(100) && !(2000..=2009).contains(&n)
}

fn en_year(n: u64) -> Vec<String> {
    let oh = if n % 100 < 10 { "oh" } else { "" };
    words(&format!(
        "{} {oh} {}",
        en_cardinal(n / 100),
        en_cardinal(n % 100)
    ))
}

#[derive(Clone, Copy, PartialEq)]
enum Pos {
    /// Before "hundert"/"tausend", where a trailing 1 is "ein".
    Head,
    Final,
}

/// Parses a folded German cardinal compound below one million.
fn parse_de(word: &str) -> Option<u64> {
    if word == "null" {
        return Some(0);
    }
    let Some((head, tail)) = word.split_once("tausend") else {
        return de_below_1000(word, Pos::Final);
    };
    let h = if head.is_empty() {
        1
    } else {
        de_below_1000(head, Pos::Head)?
    };
    let t = if tail.is_empty() {
        0
    } else {
        de_below_1000(tail, Pos::Final)?
    };
    Some(h * 1000 + t)
}

fn de_below_1000(s: &str, pos: Pos) -> Option<u64> {
    let Some((head, tail)) = s.split_once("hundert") else {
        return de_below_100(s, pos);
    };
    let h = match head {
        "" | "ein" => 1,
        _ => index_in(DE_UNITS, head).filter(|d| (2..10).contains(d))?,
    };
    let t = if tail.is_empty() {
        0
    } else {
        de_below_100(tail, pos)?
    };
    Some(h * 100 + t)
}

fn de_below_100(s: &str, pos: Pos) -> Option<u64> {
    match s {
        "ein" => return (pos == Pos::Head).then_some(1),
        "eins" => return (pos == Pos::Final).then_some(1),
        "null" => return None,
        _ => {}
    }
    if let Some(n) = index_in(DE_UNITS, s).or_else(|| index_in(DE_TENS, s).map(|t| 10 * t)) {
        return Some(n);
    }
    let (unit, tens) = s.split_once("und")?;
    let unit = match unit {
        "ein" => 1,
        _ => index_in(DE_UNITS, unit).filter(|u| (2..10).contains(u))?,
    };
    Some(10 * index_in(DE_TENS, tens)? + unit)
}

/// Value of a folded German ordinal with any inflection ending ("fuenfzehnten" → 15).
fn parse_de_ordinal(word: &str) -> Option<u64> {
    let base = ["en", "er", "es", "em", "e"]
        .iter()
        .find_map(|e| word.strip_suffix(e))?;
    let regular = base.strip_suffix("st").and_then(parse_de);
    if let Some(n) = regular.or_else(|| base.strip_suffix('t').and_then(parse_de)) {
        return Some(n);
    }
    let irregular = [
        ("erst", 1),
        ("dritt", 3),
        ("siebent", 7),
        ("siebt", 7),
        ("acht", 8),
    ];
    irregular.iter().find_map(|(suffix, value)| {
        let prefix = base.strip_suffix(suffix)?;
        let hundreds = match prefix {
            "" => 0,
            _ => parse_de(prefix).filter(|h| *h > 0 && h % 100 == 0)?,
        };
        Some(hundreds + value)
    })
}

fn is_number_word(token: &str, feat: Features) -> bool {
    let en = [EN_UNITS, EN_TENS, EN_SCALES]
        .iter()
        .any(|l| listed(l, token));
    let fr = [FR_UNITS, FR_TENS, FR_OTHER]
        .iter()
        .any(|l| listed(l, token));
    (feat.en && en) || (feat.fr && fr) || (feat.de && parse_de(&fold(token)).is_some())
}

// MARK: Tokens

fn assemble(pieces: &[Pc], feat: Features) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < pieces.len() {
        match &pieces[i] {
            Pc::W(w, true) => tokens.push(w.clone()),
            Pc::W(w, false) => {
                let mut chunks = vec![vec![w.clone()]];
                while let (Some(Pc::Join(c)), Some(Pc::W(next, false))) =
                    (pieces.get(i + 1), pieces.get(i + 2))
                {
                    match (*c, chunks.last_mut()) {
                        ('\'', Some(last)) => last.push(next.clone()),
                        _ => chunks.push(vec![next.clone()]),
                    }
                    i += 2;
                }
                tokens.extend(join_group(&chunks, feat));
            }
            _ => {}
        }
        i += 1;
    }
    tokens
}

/// Joins hyphen chunks (each a list of apostrophe-separated parts) into tokens.
fn join_group(chunks: &[Vec<String>], feat: Features) -> Vec<String> {
    let resolved: Vec<(Vec<String>, Vec<String>)> = chunks
        .iter()
        .map(|parts| resolve_apostrophes(parts, feat))
        .collect();
    let numeric = resolved
        .iter()
        .any(|(_, core)| core.iter().any(|t| is_number_word(t, feat)));
    let simple = resolved.iter().all(|(_, core)| core.len() == 1)
        && resolved
            .iter()
            .skip(1)
            .all(|(prefixes, _)| prefixes.is_empty());
    if resolved.len() > 1 && simple && !numeric {
        let mut out = resolved[0].0.clone();
        out.push(resolved.iter().map(|(_, core)| core[0].as_str()).collect());
        return out;
    }
    resolved
        .into_iter()
        .flat_map(|(p, c)| p.into_iter().chain(c))
        .collect()
}

const FR_ELISIONS: &str = "l d j qu n s c m t jusqu lorsqu puisqu quoiqu";

/// (French elision prefixes, core tokens) of one apostrophe-joined word.
fn resolve_apostrophes(parts: &[String], feat: Features) -> (Vec<String>, Vec<String>) {
    let mut idx = 0;
    while feat.fr && idx + 1 < parts.len() && listed(FR_ELISIONS, &parts[idx]) {
        idx += 1;
    }
    let (prefixes, rest) = (parts[..idx].to_vec(), &parts[idx..]);
    match rest {
        [base, suffix] if feat.en => {
            let expanded = en_contraction(base, suffix);
            (prefixes, expanded.unwrap_or_else(|| vec![rest.concat()]))
        }
        _ => (prefixes, vec![rest.concat()]),
    }
}

fn en_contraction(base: &str, suffix: &str) -> Option<Vec<String>> {
    let pair = |a: &str, b: &str| Some(vec![a.to_string(), b.to_string()]);
    match (base, suffix) {
        ("can", "t") => Some(words("cannot")),
        ("won", "t") => pair("will", "not"),
        ("shan", "t") => pair("shall", "not"),
        ("ain", "t") => pair("is", "not"),
        (_, "t") if base.len() > 1 && base.ends_with('n') => pair(&base[..base.len() - 1], "not"),
        (_, "ll") => pair(base, "will"),
        (_, "d") => pair(base, "would"),
        (_, "ve") => pair(base, "have"),
        (_, "m") => pair(base, "am"),
        (_, "re") => pair(base, "are"),
        ("let", "s") => pair("let", "us"),
        (
            "it" | "that" | "here" | "there" | "what" | "where" | "who" | "how" | "he" | "she",
            "s",
        ) => pair(base, "is"),
        _ => None,
    }
}

fn fold(s: &str) -> String {
    s.replace('ä', "ae")
        .replace('ö', "oe")
        .replace('ü', "ue")
        .replace('ß', "ss")
}

fn map_token(t: &str, feat: Features) -> Option<&'static str> {
    let general = match t {
        "millimeter" | "millimetern" | "millimeters" | "millimetre" | "millimetres"
        | "millimètre" | "millimètres" => "millimeter",
        "zentimeter" | "zentimetern" | "centimeter" | "centimeters" | "centimetre"
        | "centimetres" | "centimètre" | "centimètres" => "centimeter",
        "km" | "kilometern" | "kilometers" | "kilometre" | "kilometres" | "kilomètre"
        | "kilomètres" => "kilometer",
        "metern" | "meters" | "metre" | "metres" | "mètre" | "mètres" => "meter",
        "kilogramm" | "kilogramms" | "kilograms" | "kilogramme" | "kilogrammes" | "kilo"
        | "kilos" => "kilogram",
        "millisekunde" | "millisekunden" | "milliseconds" | "milliseconde" | "millisecondes" => {
            "millisecond"
        }
        "minuten" | "minutes" => "minute",
        "terabytes" => "terabyte",
        "gigabytes" => "gigabyte",
        "megabytes" => "megabyte",
        "kilobytes" => "kilobyte",
        "grad" | "degrees" | "degré" | "degrés" => "degree",
        "m²" | "qm" | "quadratmetern" => "quadratmeter",
        "euros" | "eur" => "euro",
        "dollars" | "usd" => "dollar",
        "pounds" => "pound",
        "millionen" | "millions" | "mio" => "million",
        "milliarden" | "milliard" | "milliards" | "mrd" => "milliarde",
        "billions" | "bn" => "billion",
        "dr" | "doktor" => "doctor",
        "mr" => "mister",
        "mrs" => "missus",
        "etc" => "et cetera",
        "vs" => "versus",
        _ => "",
    };
    if !general.is_empty() {
        return Some(general);
    }
    Some(match t {
        "bzw" if feat.de => "beziehungsweise",
        "usw" if feat.de => "und so weiter",
        "ca" if feat.de => "circa",
        "nr" if feat.de => "nummer",
        "inkl" if feat.de => "inklusive",
        "evtl" if feat.de => "eventuell",
        "ggf" if feat.de => "gegebenenfalls",
        "zb" if feat.de => "zum beispiel",
        "bspw" if feat.de => "beispielsweise",
        "pm" if feat.en => "p m",
        "vingts" if feat.fr => "vingt",
        "cents" if feat.fr => "cent",
        "heures" if feat.fr => "heure",
        _ => return None,
    })
}

fn canon_token(token: &str, feat: Features) -> Vec<String> {
    let t = if feat.de {
        fold(token)
    } else {
        token.to_string()
    };
    if let Some(mapped) = map_token(&t, feat) {
        return words(mapped);
    }
    if feat.de {
        if let Some(n) = parse_de(&t) {
            return vec![de_compound(n)];
        }
        if let Some(n) = parse_de_ordinal(&t).filter(|n| *n < 1_000_000) {
            return vec![de_ordinal(n)];
        }
    }
    vec![t]
}

fn is_filler(w: &str, feat: Features) -> bool {
    listed("äh ähm aeh aehm öh öhm oeh oehm hm hmm uh uhm erm euh", w)
        || (feat.en && (w == "mm" || w == "umm" || (w == "um" && !feat.de)))
        || (feat.fr && (w == "hum" || w == "heu"))
}

/// Per-token canonicalisation, multi-token abbreviations, English "and" in numbers, fillers.
fn finish(raw: Vec<String>, feat: Features) -> Vec<String> {
    let t: Vec<String> = raw.iter().flat_map(|w| canon_token(w, feat)).collect();
    let mut paired = Vec::with_capacity(t.len());
    let mut i = 0;
    while i < t.len() {
        let replacement = match (t[i].as_str(), t.get(i + 1).map(String::as_str)) {
            ("z", Some("b")) if feat.de => "zum beispiel",
            ("d", Some("h")) if feat.de => "das heisst",
            ("e", Some("g")) if feat.en => "for example",
            ("i", Some("e")) if feat.en => "that is",
            ("per", Some("cent")) if feat.en => "percent",
            _ => "",
        };
        if replacement.is_empty() {
            paired.push(t[i].clone());
            i += 1;
        } else {
            paired.extend(words(replacement));
            i += 2;
        }
    }
    let mut out = Vec::with_capacity(paired.len());
    for (k, w) in paired.iter().enumerate() {
        let prev = k.checked_sub(1).map(|p| paired[p].as_str());
        let next = paired.get(k + 1).map(String::as_str);
        let number_and = w == "and"
            && prev.is_some_and(|p| listed(EN_SCALES, p))
            && next.is_some_and(|n| listed(EN_UNITS, n) || listed(EN_TENS, n));
        if (feat.en && number_and) || is_filler(w, feat) {
            continue;
        }
        let one =
            matches!(w.as_str(), "ein" | "eins") && matches!(next, Some("million" | "milliarde"));
        out.push(if feat.de && one {
            "eine".to_string()
        } else {
            w.clone()
        });
    }
    out
}

/// Case- and punctuation-preserving tokens for the formatted error rate.
pub fn formatted_tokens(text: &str) -> Vec<String> {
    let chars: Vec<char> = text
        .nfc()
        .map(|c| if matches!(c, '’' | 'ʼ') { '\'' } else { c })
        .collect();
    let mut out = Vec::new();
    let mut current = String::new();
    for (k, &c) in chars.iter().enumerate() {
        let prev = k.checked_sub(1).map(|p| chars[p]);
        let next = chars.get(k + 1).copied();
        let between =
            |f: fn(&char) -> bool| prev.as_ref().is_some_and(f) && next.as_ref().is_some_and(f);
        let split = match c {
            '.' | ',' | ':' => !between(char::is_ascii_digit),
            '\'' => !between(|c| c.is_alphanumeric()),
            _ => "?!;…\"“”„«»‹›‘()[]{}–—".contains(c),
        };
        if (c.is_whitespace() || split) && !current.is_empty() {
            out.push(std::mem::take(&mut current));
        }
        if split {
            out.push(c.to_string());
        } else if !c.is_whitespace() {
            current.push(c);
        }
    }
    out.extend(Some(current).filter(|c| !c.is_empty()));
    out
}

// MARK: Language guess

const DE_STOP: &str = "aber alle als auch auf aus bei bin bis bitte da damit danke dann dass dem \
    den der die diese dieser dir doch ein eine einen einem einer er es etwas euch für geht gibt \
    gleich haben hat heute hier ich ihr ihre ihnen im ist ja jetzt kann kannst kein keine mehr \
    mich mir mit morgen nach nein nicht noch nur ob oder schon sehr sein seit sich sie sind über \
    uhr und uns unser unsere vom von vor war weil wenn werden wie wir wird wurde zu zum zur";
const EN_STOP: &str = "about after all and any are at be because been before but by can could \
    did do does for from get got had has have he his how i if into is it its just let my not of \
    only or our out please she should than thanks that the their them then there these they this \
    those to up us was we were what when where which while who why with would yes you your";
const FR_STOP: &str = "à au aussi aux avant avec beaucoup bien bonjour c ce ces cette comme ça \
    dans de demain elle elles est et été être fait il ils j je l la le les leur lui ma mais merci \
    mes mon ne nos notre nous où ou oui par pas pour qu que qui sa se ses sont sur très un une \
    votre vous y";
/// Words that start a new sentence after a number ("Woche 41. Zuerst"), so no dot ordinal.
const DE_STARTERS: &str = "zuerst danach außerdem insgesamt deshalb trotzdem also so in an \
    allerdings gestern das was wo";

fn stopword_lang(word: &str, langs: &[Lang]) -> Option<Lang> {
    let mut hits = langs
        .iter()
        .filter(|l| listed(pick(**l, DE_STOP, EN_STOP, FR_STOP), word));
    let lang = *hits.next()?;
    hits.next().is_none().then_some(lang)
}

fn is_de_starter(word: &str) -> bool {
    listed(DE_STOP, word) || listed(DE_STARTERS, word)
}

/// Stopword-based language guess among candidates (all three when empty); None when undecidable.
pub fn guess_language(text: &str, candidates: &[Lang]) -> Option<Lang> {
    let all = [Lang::De, Lang::En, Lang::Fr];
    let candidates = if candidates.is_empty() {
        &all[..]
    } else {
        candidates
    };
    let lowered = text.nfc().collect::<String>().to_lowercase();
    let words = lowered
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty());
    tally(words.map(|w| stopword_lang(w, candidates)), candidates)
}

// MARK: Alignment

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Op {
    #[serde(rename = "=")]
    Match,
    #[serde(rename = "S")]
    Sub,
    #[serde(rename = "D")]
    Del,
    #[serde(rename = "I")]
    Ins,
}

/// One alignment step; serialises as `[op, reference, hypothesis]`.
#[derive(Clone, Debug, PartialEq)]
pub struct AlignedPair {
    pub op: Op,
    pub reference: Option<String>,
    pub hypothesis: Option<String>,
}

impl Serialize for AlignedPair {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        (&self.op, &self.reference, &self.hypothesis).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for AlignedPair {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let (op, reference, hypothesis) = Deserialize::deserialize(deserializer)?;
        Ok(AlignedPair {
            op,
            reference,
            hypothesis,
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorCounts {
    pub reference_words: usize,
    pub substitutions: usize,
    pub deletions: usize,
    pub insertions: usize,
}

impl ErrorCounts {
    pub fn errors(&self) -> usize {
        self.substitutions + self.deletions + self.insertions
    }

    /// Errors per reference unit; None for an empty reference.
    pub fn rate(&self) -> Option<f64> {
        (self.reference_words > 0).then(|| self.errors() as f64 / self.reference_words as f64)
    }

    pub fn add(&mut self, other: &ErrorCounts) {
        self.reference_words += other.reference_words;
        self.substitutions += other.substitutions;
        self.deletions += other.deletions;
        self.insertions += other.insertions;
    }
}

/// Above this many DP cells `align` returns counts only (empty alignment) to bound memory.
const MAX_ALIGNMENT_CELLS: usize = 64_000_000;

/// Minimum-edit-distance alignment (S/D/I cost 1); ties prefer Match, Sub, Del, then Ins.
pub fn align(reference: &[String], hypothesis: &[String]) -> (ErrorCounts, Vec<AlignedPair>) {
    let (n, width) = (reference.len(), hypothesis.len() + 1);
    if (n + 1).saturating_mul(width) > MAX_ALIGNMENT_CELLS {
        return (edit_counts(reference, hypothesis), Vec::new());
    }
    // Back-pointers: 0 diagonal, 1 deletion, 2 insertion.
    let mut dir = vec![0u8; (n + 1) * width];
    dir[1..width].fill(2);
    let mut prev: Vec<usize> = (0..width).collect();
    let mut cur = vec![0usize; width];
    for i in 1..=n {
        cur[0] = i;
        dir[i * width] = 1;
        for j in 1..width {
            let diag = prev[j - 1] + usize::from(reference[i - 1] != hypothesis[j - 1]);
            let (del, ins) = (prev[j] + 1, cur[j - 1] + 1);
            (cur[j], dir[i * width + j]) = if diag <= del && diag <= ins {
                (diag, 0)
            } else if del <= ins {
                (del, 1)
            } else {
                (ins, 2)
            };
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    let mut counts = ErrorCounts {
        reference_words: n,
        ..Default::default()
    };
    let mut pairs = Vec::new();
    let (mut i, mut j) = (n, width - 1);
    while i > 0 || j > 0 {
        let step = dir[i * width + j];
        i -= usize::from(step != 2);
        j -= usize::from(step != 1);
        let r = (step != 2).then(|| reference[i].clone());
        let h = (step != 1).then(|| hypothesis[j].clone());
        let op = match step {
            0 if r == h => Op::Match,
            0 => Op::Sub,
            1 => Op::Del,
            _ => Op::Ins,
        };
        match op {
            Op::Sub => counts.substitutions += 1,
            Op::Del => counts.deletions += 1,
            Op::Ins => counts.insertions += 1,
            Op::Match => {}
        }
        pairs.push(AlignedPair {
            op,
            reference: r,
            hypothesis: h,
        });
    }
    pairs.reverse();
    (counts, pairs)
}

/// Same counts and tie-breaking as `align`, in O(len(hypothesis)) memory.
fn edit_counts<T: PartialEq>(reference: &[T], hypothesis: &[T]) -> ErrorCounts {
    // Cells are [cost, substitutions, deletions, insertions].
    let mut prev: Vec<[usize; 4]> = (0..=hypothesis.len()).map(|j| [j, 0, 0, j]).collect();
    let mut cur = prev.clone();
    for (r, a) in reference.iter().enumerate() {
        cur[0] = [r + 1, 0, r + 1, 0];
        for (j, b) in hypothesis.iter().enumerate() {
            let sub = usize::from(a != b);
            let (diag, up, left) = (prev[j], prev[j + 1], cur[j]);
            cur[j + 1] = if diag[0] + sub <= up[0] + 1 && diag[0] + sub <= left[0] + 1 {
                [diag[0] + sub, diag[1] + sub, diag[2], diag[3]]
            } else if up[0] <= left[0] {
                [up[0] + 1, up[1], up[2] + 1, up[3]]
            } else {
                [left[0] + 1, left[1], left[2], left[3] + 1]
            };
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    let [_, substitutions, deletions, insertions] = prev[hypothesis.len()];
    ErrorCounts {
        reference_words: reference.len(),
        substitutions,
        deletions,
        insertions,
    }
}

/// Character-level counts over the tokens joined with single spaces.
pub fn character_error_counts(reference: &[String], hypothesis: &[String]) -> ErrorCounts {
    let r: Vec<char> = reference.join(" ").chars().collect();
    let h: Vec<char> = hypothesis.join(" ").chars().collect();
    edit_counts(&r, &h)
}

/// Makes hypothesis compounds follow the reference segmentation. First a hypothesis token equal to
/// 2–4 consecutive reference tokens joined is split into them; then 2–4 consecutive hypothesis
/// tokens (longest first, left to right) whose concatenation is a reference token are merged,
/// unless every part is itself a reference token.
fn reconcile(reference: &[String], hypothesis: Vec<String>) -> Vec<String> {
    let in_ref: HashSet<&str> = reference.iter().map(String::as_str).collect();
    let mut splits: HashMap<String, &[String]> = HashMap::new();
    for k in 2..=4 {
        for window in reference.windows(k) {
            let joined = window.concat();
            if !in_ref.contains(joined.as_str()) {
                splits.entry(joined).or_insert(window);
            }
        }
    }
    let mut split = Vec::with_capacity(hypothesis.len());
    for token in hypothesis {
        match splits.get(&token) {
            Some(parts) if !in_ref.contains(token.as_str()) => split.extend(parts.iter().cloned()),
            _ => split.push(token),
        }
    }
    let mut out = Vec::with_capacity(split.len());
    let mut j = 0;
    while j < split.len() {
        let merge = (2..=4).rev().find_map(|k| {
            let parts = split.get(j..j + k)?;
            let joined = parts.concat();
            let known = parts.iter().all(|p| in_ref.contains(p.as_str()));
            (in_ref.contains(joined.as_str()) && !known).then_some((joined, k))
        });
        let (token, k) = merge.unwrap_or_else(|| (split[j].clone(), 1));
        out.push(token);
        j += k;
    }
    out
}

// MARK: Scoring

/// Complete scoring of one hypothesis against one reference.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairScore {
    pub normalised: ErrorCounts,
    pub formatted: ErrorCounts,
    pub characters: ErrorCounts,
    pub alignment: Vec<AlignedPair>,
    pub reference_tokens: Vec<String>,
    pub hypothesis_tokens: Vec<String>,
    pub numbers_correct: usize,
    pub numbers_total: usize,
    pub insertion_burst: usize,
    pub repetition_loop: bool,
}

/// Scores a hypothesis. With segments, the union of `langs` and the segment languages is used for
/// the hypothesis and for the token rules of every segment (numbers use the segment language).
pub fn score(reference: &ReferenceText, hypothesis: &str, langs: &[Lang]) -> PairScore {
    let mut all = langs.to_vec();
    if let ReferenceText::Segments(segments) = reference {
        all.extend(segments.iter().map(|(lang, _)| *lang));
    }
    let ctx = Ctx::new(&all);
    let ((reference_tokens, entities), reference_raw) = match reference {
        ReferenceText::Plain(text) => (normalise_ctx(text, &ctx), text.to_string()),
        ReferenceText::Segments(segments) => {
            let raw: Vec<&str> = segments.iter().map(|(_, text)| text.as_str()).collect();
            (segments_ctx(segments, ctx.feat), raw.join(" "))
        }
    };
    let hypothesis_raw = normalise_ctx(hypothesis, &ctx).0;
    let numbers_correct = entities
        .iter()
        .filter(|e| contains_run(&hypothesis_raw, e))
        .count();
    let hypothesis_tokens = reconcile(&reference_tokens, hypothesis_raw);
    let (normalised, alignment) = align(&reference_tokens, &hypothesis_tokens);
    PairScore {
        normalised,
        formatted: edit_counts(
            &formatted_tokens(&reference_raw),
            &formatted_tokens(hypothesis),
        ),
        characters: character_error_counts(&reference_tokens, &hypothesis_tokens),
        insertion_burst: max_insertion_burst(&alignment),
        repetition_loop: has_repetition_loop(&hypothesis_tokens),
        alignment,
        reference_tokens,
        hypothesis_tokens,
        numbers_correct,
        numbers_total: entities.len(),
    }
}

fn contains_run(haystack: &[String], needle: &[String]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|w| w == needle)
}

/// Term recall: case-insensitive, NFC, umlaut-sensitive; hyphen, space or nothing between parts.
pub fn term_found(term: &str, hypothesis: &str) -> bool {
    let parts = |s: &str| -> Vec<String> {
        let lowered = s.nfc().collect::<String>().to_lowercase();
        let parts = lowered
            .split(|c: char| !c.is_alphanumeric())
            .filter(|p| !p.is_empty());
        parts.map(String::from).collect()
    };
    let target = parts(term).concat();
    let words = parts(hypothesis);
    let found_at = |start: usize| {
        let mut joined = String::new();
        let stopped = words[start..].iter().any(|word| {
            joined.push_str(word);
            joined.len() >= target.len() || !target.starts_with(&joined)
        });
        stopped && joined == target
    };
    !target.is_empty() && (0..words.len()).any(found_at)
}

/// Numeric entities typed with digits in the reference found in the hypothesis: (correct, total).
pub fn number_accuracy(reference: &str, hypothesis: &str, langs: &[Lang]) -> (usize, usize) {
    let ctx = Ctx::new(langs);
    let entities = normalise_ctx(reference, &ctx).1;
    let hypothesis = normalise_ctx(hypothesis, &ctx).0;
    (
        entities
            .iter()
            .filter(|e| contains_run(&hypothesis, e))
            .count(),
        entities.len(),
    )
}

pub fn max_insertion_burst(alignment: &[AlignedPair]) -> usize {
    let mut run = 0;
    let runs = alignment.iter().map(|pair| {
        run = if pair.op == Op::Ins { run + 1 } else { 0 };
        run
    });
    runs.max().unwrap_or(0)
}

/// An n-gram (n = 1..=6) repeated at least 4 times back to back.
pub fn has_repetition_loop(tokens: &[String]) -> bool {
    (1..=6).any(|n| {
        let periodic = |s: usize| (0..3 * n).all(|k| tokens[s + k] == tokens[s + k + n]);
        tokens.len() >= 4 * n && (0..=tokens.len() - 4 * n).any(periodic)
    })
}

// MARK: Statistics

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ClipErrors {
    pub errors: usize,
    pub reference_words: usize,
}

/// Σerrors / Σreference words; None when there are no reference words.
pub fn pooled_rate(clips: &[ClipErrors]) -> Option<f64> {
    let errors: usize = clips.iter().map(|c| c.errors).sum();
    let words: usize = clips.iter().map(|c| c.reference_words).sum();
    (words > 0).then(|| errors as f64 / words as f64)
}

/// Mean per-clip rate over clips with reference words.
pub fn macro_rate(clips: &[ClipErrors]) -> Option<f64> {
    let usable = clips.iter().filter(|c| c.reference_words > 0);
    let rates: Vec<f64> = usable
        .map(|c| c.errors as f64 / c.reference_words as f64)
        .collect();
    (!rates.is_empty()).then(|| rates.iter().sum::<f64>() / rates.len() as f64)
}

struct SplitMix64(u64);

impl SplitMix64 {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform index below `n`.
    fn below(&mut self, n: usize) -> usize {
        ((self.next() as u128 * n as u128) >> 64) as usize
    }
}

/// `iterations` bootstrap resamples (with replacement) of `n` clip indices, each mapped by `rate`.
fn resample(n: usize, iterations: usize, seed: u64, rate: impl Fn(&[usize]) -> f64) -> Vec<f64> {
    let mut rng = SplitMix64(seed);
    let mut sample = vec![0; n];
    let mut draw = || {
        sample.iter_mut().for_each(|s| *s = rng.below(n));
        rate(&sample)
    };
    (0..iterations).map(|_| draw()).collect()
}

fn pooled_at(clips: &[ClipErrors], idx: &[usize]) -> f64 {
    let picked: Vec<ClipErrors> = idx.iter().map(|&i| clips[i]).collect();
    pooled_rate(&picked).unwrap_or(0.0)
}

/// 95% percentile bootstrap CI of the pooled rate (clips resampled with replacement).
pub fn bootstrap_ci(clips: &[ClipErrors], iterations: usize, seed: u64) -> Option<(f64, f64)> {
    let usable: Vec<ClipErrors> = clips
        .iter()
        .copied()
        .filter(|c| c.reference_words > 0)
        .collect();
    if usable.len() < 2 || iterations == 0 {
        return None;
    }
    let rates = resample(usable.len(), iterations, seed, |idx| {
        pooled_at(&usable, idx)
    });
    Some((percentile(&rates, 2.5)?, percentile(&rates, 97.5)?))
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairedComparison {
    pub rate_a: f64,
    pub rate_b: f64,
    pub difference: f64,
    pub ci95: (f64, f64),
    pub p_value: f64,
}

/// Paired bootstrap of pooled(a) − pooled(b), where a[i] and b[i] are the same clip.
pub fn paired_bootstrap(
    a: &[ClipErrors],
    b: &[ClipErrors],
    iterations: usize,
    seed: u64,
) -> Option<PairedComparison> {
    if a.len() != b.len() || iterations == 0 {
        return None;
    }
    let usable = a
        .iter()
        .zip(b)
        .filter(|(x, y)| x.reference_words > 0 && y.reference_words > 0);
    let (a, b): (Vec<ClipErrors>, Vec<ClipErrors>) = usable.map(|(x, y)| (*x, *y)).unzip();
    if a.len() < 2 {
        return None;
    }
    let diffs = resample(a.len(), iterations, seed, |idx| {
        pooled_at(&a, idx) - pooled_at(&b, idx)
    });
    let at_most_zero = diffs.iter().filter(|d| **d <= 0.0).count();
    let at_least_zero = diffs.iter().filter(|d| **d >= 0.0).count();
    let total = iterations as f64;
    let (rate_a, rate_b) = (pooled_rate(&a)?, pooled_rate(&b)?);
    Some(PairedComparison {
        rate_a,
        rate_b,
        difference: rate_a - rate_b,
        ci95: (percentile(&diffs, 2.5)?, percentile(&diffs, 97.5)?),
        p_value: (2.0 * at_most_zero.min(at_least_zero) as f64 / total).clamp(1.0 / total, 1.0),
    })
}

/// Linear-interpolated percentile, p in [0, 100]; NaN values are ignored.
pub fn percentile(values: &[f64], p: f64) -> Option<f64> {
    let mut sorted: Vec<f64> = values.iter().copied().filter(|v| !v.is_nan()).collect();
    if sorted.is_empty() || p.is_nan() {
        return None;
    }
    sorted.sort_by(f64::total_cmp);
    let rank = p.clamp(0.0, 100.0) / 100.0 * (sorted.len() - 1) as f64;
    let (lo, hi) = (rank.floor() as usize, rank.ceil() as usize);
    Some(sorted[lo] + (sorted[hi] - sorted[lo]) * (rank - lo as f64))
}

#[cfg(test)]
mod tests {
    use super::*;
    use Lang::{De, En, Fr};

    fn joined(text: &str, langs: &[Lang]) -> String {
        normalise(text, langs).join(" ")
    }

    fn rows(table: &str) -> impl Iterator<Item = (&str, &str)> {
        let lines = table.lines().map(str::trim).filter(|l| !l.is_empty());
        lines.map(|l| l.split_once(" | ").unwrap_or((l, "")))
    }

    /// Every `reference | hypothesis` row scores 0 normalised errors.
    fn same(langs: &[Lang], table: &str) {
        for (reference, hypothesis) in rows(table) {
            let s = score(&ReferenceText::Plain(reference), hypothesis, langs);
            let (r, h) = (&s.reference_tokens, &s.hypothesis_tokens);
            assert_eq!(
                s.normalised.errors(),
                0,
                "{reference:?} vs {hypothesis:?}\n{r:?}\n{h:?}"
            );
        }
    }

    /// Every `text | expected tokens` row normalises as expected.
    fn norm(langs: &[Lang], table: &str) {
        for (text, expected) in rows(table) {
            assert_eq!(joined(text, langs), expected, "{text:?}");
        }
    }

    #[test]
    fn lang_codes() {
        for (code, lang) in [
            ("de-DE", De),
            ("Deutsch", De),
            ("English", En),
            ("Français", Fr),
        ] {
            assert_eq!(Lang::from_code(code), Some(lang));
        }
        assert_eq!(
            (Lang::from_code("fr_CA"), Lang::from_code("mixed")),
            (Some(Fr), None)
        );
        assert_eq!(Fr.code(), "fr");
        assert_eq!(serde_json::to_string(&De).unwrap(), "\"de\"");
    }

    #[test]
    fn german_numbers() {
        same(
            &[De],
            "Die Rechnung über 3.480 Euro ist am 15. November fällig. | die rechnung über \
                dreitausendvierhundertachtzig euro ist am fünfzehnten november fällig
            um 14 Uhr | um vierzehn Uhr
            8,5 Prozent | acht Komma fünf Prozent
            8,5 Prozent | 8,5 %
            8,5 Prozent | 8.5%
            Version 2.3 | Version zwei Punkt drei
            plus minus 0,05 Millimetern | ±0,05 mm
            1,2 Millionen Euro | 1,2 Mio. Euro
            1,2 Millionen Euro | eins Komma zwei Millionen Euro
            1 Million | eine Million
            1.250.000 | eine Million zweihundertfünfzigtausend
            15.000 Euro | 15 000 Euro
            Q1 | Q eins
            p95 | P 95
            Größe | Groesse
            zweihundertfünfundvierzig | 245
            hundert Euro | 100 Euro
            am 3. Dezember | am dritten Dezember
            der 1. Platz | der erste Platz
            im 95. Perzentil | im fünfundneunzigsten Perzentil
            um 9:30 | um 9.30 Uhr
            um 9:30 Uhr | um neun Uhr dreißig
            -4 Grad | minus vier Grad
            2,3 TB | zwei Komma drei Terabyte",
        );
        norm(
            &[De],
            "20.000 | zwanzigtausend
            1.250.000 | eine million zweihundertfuenfzigtausend
            1,75 Millionen | eins komma fuenfundsiebzig million
            bei 15. Dann | bei fuenfzehn dann
            auf der A9. Fangt an | auf der a neun fangt an
            am 15.11.2025 | am fuenfzehnte elfte zweitausendfuenfundzwanzig
            Version 2.3. | version zwei punkt drei
            2.001 | zweitausendeins
            101.000 | einhunderteintausend
            0151 | null eins fuenf eins
            hundertachte | einhundertachte
            beachten Hunderte | beachten hunderte
            dritten ersten | dritte erste
            fünfzehnten fünfzehnter fünfzehntes | fuenfzehnte fuenfzehnte fuenfzehnte",
        );
        let week = joined("Kalenderwoche 41.\n\nZuerst", &[De]);
        assert_eq!(week, "kalenderwoche einundvierzig zuerst");
        assert_eq!(normalise("1234567890123", &[De]).len(), 13);
    }

    #[test]
    fn german_words() {
        same(
            &[De],
            "Pull Request | Pull-Request
            Kundenzufriedenheitsumfrage | Kunden Zufriedenheitsumfrage
            Change-of-Control-Klauseln | Change of Control Klauseln
            Milch-Frischprodukte | Milchfrischprodukte
            z. B. heute | zum Beispiel heute
            z.B. heute | zB heute
            Frau Dr. Weißgerber | Frau Doktor Weissgerber
            d.h. bzw. usw. ca. Nr. | das heißt beziehungsweise und so weiter circa Nummer",
        );
        norm(
            &[De],
            "EBITDA-Marge | ebitdamarge
            Change-of-Control-Klauseln | changeofcontrolklauseln
            äh ich ähm komme hm | ich komme
            um 9 | um neun
            das war's | das wars",
        );
        assert_eq!(joined("um 9", &[De, En]), "um neun");
    }

    #[test]
    fn english() {
        same(
            &[En],
            "1,250 units at 18.40 dollars each comes to 23,000 dollars | one thousand two hundred \
                fifty units at eighteen point forty dollars each comes to twenty three thousand dollars
            1,250 units at 18.40 dollars each comes to 23,000 dollars | 1,250 units at $18.40 each \
                comes to $23,000
            at 3 pm | at 3 p.m.
            at 3 pm | at three PM
            at 3 pm | at 3:00 pm
            I'd like | I would like
            105 degrees Celsius | 105°C
            5 percent | 5%
            5 per cent | 5 %
            $4.2 million | 4.2 million dollars
            one hundred and five | 105
            twenty-six | 26
            the 95th percentile | the ninety-fifth percentile
            800 ms | 800 milliseconds
            e.g. this | for example this
            Dr. Smith | Doctor Smith",
        );
        norm(
            &[En],
            "2:13 pm | two thirteen p m
            9:05 | nine oh five
            don't can't won't it's let's today's we've | do not cannot will not it is let us todays we have
            salt and pepper | salt and pepper
            24th 1st 2nd 3rd | twenty fourth first second third
            in 2026 we | in twenty twenty six we
            2005 | two thousand five
            1250 units | one thousand two hundred fifty units
            version 4.12 | version four point twelve
            0.1 percent | zero point one percent
            500ms | five hundred millisecond
            uh I um think mm | i think",
        );
    }

    #[test]
    fn french() {
        same(
            &[Fr],
            "La commande porte sur 2 500 pièces à 3,20 euros l'unité, soit 8 000 euros. | la commande \
                porte sur 2500 pièces à 3,20 € l'unité soit 8000 €
            quatre-vingt-dix | 90
            soixante et onze | 71
            quatre-vingts | 80
            quatre-vingt-un | 81
            deux cents | 200
            vingt-et-un | 21
            à 14 heures | à 14h
            à 14h30 | à 14 heures 30
            7 pour cent | 7 %
            600 000 euros | 600\u{a0}000 euros
            600 000 euros | 600\u{202f}000 euros",
        );
        norm(
            &[Fr],
            "4,6 millions d'euros | quatre virgule six million d euro
            l'unité d'ici | l unité d ici
            1er 1re 3ème 2e | premier première troisième deuxième
            euh 21e | vingt et unième",
        );
        let s = score(&ReferenceText::Plain("le marché"), "le marche", &[Fr]);
        assert_eq!(s.normalised.substitutions, 1);
    }

    #[test]
    fn mixed() {
        let (de, en) = (
            "Ich bin heute bis 16 Uhr im Büro.",
            "After that, I'm only reachable at 5 pm.",
        );
        let text = format!("{de} {en}");
        let tokens = normalise(&text, &[De, En]);
        for expected in ["sechzehn", "five", "buero", "am"] {
            assert!(
                tokens.contains(&expected.to_string()),
                "{expected} {tokens:?}"
            );
        }
        let segments = vec![(De, de.to_string()), (En, en.to_string())];
        assert_eq!(normalise_segments(&segments), tokens);
        let s = score(&ReferenceText::Segments(&segments), &text, &[]);
        assert_eq!(
            (s.normalised.errors(), s.numbers_correct, s.numbers_total),
            (0, 2, 2)
        );
        let spoken =
            "Ich bin heute bis sechzehn Uhr im Büro. After that I am only reachable at five p.m.";
        let s = score(&ReferenceText::Segments(&segments), spoken, &[De, En]);
        assert_eq!(s.normalised.errors(), 0);
        let text = "Merci beaucoup, 3 fois. Danke, 3 mal.";
        assert_eq!(
            joined(text, &[De, Fr]),
            "merci beaucoup trois fois danke drei mal"
        );
    }

    #[test]
    fn alignment() {
        let (r, h) = (words("a b c d"), words("a x c d e"));
        let (c, pairs) = align(&r, &h);
        assert_eq!(
            (
                c.substitutions,
                c.deletions,
                c.insertions,
                c.reference_words
            ),
            (1, 0, 1, 4)
        );
        assert_eq!((pairs.len(), edit_counts(&r, &h)), (5, c));
        let (c, _) = align(&r, &words("b c"));
        assert_eq!((c.substitutions, c.deletions, c.insertions), (0, 2, 0));
        let pair = |op, r: Option<&str>, h: Option<&str>| AlignedPair {
            op,
            reference: r.map(String::from),
            hypothesis: h.map(String::from),
        };
        let (c, pairs) = align(&[], &words("a"));
        assert_eq!(
            (c.insertions, c.rate(), pairs),
            (1, None, vec![pair(Op::Ins, None, Some("a"))])
        );
        let (c, _) = align(&r, &[]);
        assert_eq!((c.deletions, c.rate()), (4, Some(1.0)));
        let swapped = align(&words("a b"), &words("b a"));
        assert_eq!(swapped, align(&words("a b"), &words("b a")));
        assert_eq!(
            swapped.1.iter().map(|p| p.op).collect::<Vec<_>>(),
            [Op::Sub, Op::Sub]
        );
        let all = vec![
            pair(Op::Sub, Some("a"), Some("b")),
            pair(Op::Del, Some("a"), None),
            pair(Op::Ins, None, Some("b")),
            pair(Op::Match, Some("a"), Some("a")),
        ];
        let json = serde_json::to_string(&all).unwrap();
        assert_eq!(
            json,
            r#"[["S","a","b"],["D","a",null],["I",null,"b"],["=","a","a"]]"#
        );
        assert_eq!(
            serde_json::from_str::<Vec<AlignedPair>>(&json).unwrap(),
            all
        );
        let mut total = ErrorCounts::default();
        total.add(&c);
        total.add(&c);
        assert_eq!(serde_json::to_value(total).unwrap()["referenceWords"], 8);
        let chars = character_error_counts(&words("abc"), &words("abd"));
        assert_eq!((chars.reference_words, chars.substitutions), (3, 1));
    }

    #[test]
    fn formatted() {
        let tokens = |t: &str| formatted_tokens(t).join(" ");
        let cases = [
            (
                "Hallo Frau Özdemir, vielen Dank.",
                "Hallo Frau Özdemir , vielen Dank .",
            ),
            ("8,5 % um 9:30, 3.480 €", "8,5 % um 9:30 , 3.480 €"),
            (
                "I don’t know «oui» l'unité?",
                "I don't know « oui » l'unité ?",
            ),
            ("EBITDA-Marge (2.3)", "EBITDA-Marge ( 2.3 )"),
        ];
        for (text, expected) in cases {
            assert_eq!(tokens(text), expected);
        }
        let s = score(&ReferenceText::Plain("Hallo, Welt."), "hallo Welt", &[De]);
        assert_eq!((s.normalised.errors(), s.formatted.errors()), (0, 3));
    }

    #[test]
    fn terms() {
        assert!(term_found("Pull Request", "der pull-request ist fertig"));
        assert!(term_found("Pull Request", "der PullRequest ist fertig"));
        assert!(term_found("EBITDA-Marge", "die ebitda marge"));
        assert!(term_found("Lyon", "à Lyon."));
        assert!(term_found("chiffre d'affaires", "le chiffre d’affaires"));
        assert!(!term_found("Lyon", "lyonnais"));
        assert!(!term_found("Göttingen", "Goettingen"));
        assert!(!term_found("", "anything"));
    }

    #[test]
    fn numbers() {
        let de = "Die Rechnung über 3.480 Euro ist am 15. November fällig.";
        let hyp = "dreitausendvierhundertachtzig euro am fünfzehnten";
        assert_eq!(number_accuracy(de, hyp, &[De]), (2, 2));
        assert_eq!(
            number_accuracy(de, "3.480 Euro am 16. November", &[De]),
            (1, 2)
        );
        let tags = "Q1 und p95 bei 8,5 Prozent";
        assert_eq!(
            number_accuracy(tags, "Q eins und P 95 bei 8,5 %", &[De]),
            (3, 3)
        );
        assert_eq!(number_accuracy("um 14 Uhr", "um vier Uhr", &[De]), (0, 1));
        let en = "1,250 units at $18.40";
        assert_eq!(
            number_accuracy(en, "1250 units at 18.40 dollars", &[En]),
            (2, 2)
        );
        let fr = "2 500 pièces à 3,20 euros";
        assert_eq!(number_accuracy(fr, "2500 pièces à 3,20 €", &[Fr]), (2, 2));
        assert_eq!(number_accuracy("Ja, passt so.", "ja", &[De]), (0, 0));
        assert_eq!(number_accuracy("zwanzig Teile", "20 Teile", &[De]), (0, 0));
    }

    #[test]
    fn loops_and_bursts() {
        let looped = |t: &str| has_repetition_loop(&normalise(t, &[De]));
        assert!(looped("danke danke danke danke"));
        assert!(looped(
            "ich weiß nicht ich weiß nicht ich weiß nicht ich weiß nicht"
        ));
        assert!(!looped("danke danke danke, das war sehr nett"));
        assert!(!has_repetition_loop(&[]));
        let (_, pairs) = align(&words("a b"), &words("a x y z b q"));
        assert_eq!(max_insertion_burst(&pairs), 3);
        let s = score(
            &ReferenceText::Plain("danke"),
            "danke danke danke danke",
            &[De],
        );
        assert!(s.repetition_loop && s.insertion_burst == 3);
    }

    #[test]
    fn language_guess() {
        let all = [De, En, Fr];
        assert_eq!(
            guess_language("Bitte ruf mich morgen früh zurück.", &all),
            Some(De)
        );
        assert_eq!(
            guess_language("Can you send me the slides before the call?", &[]),
            Some(En)
        );
        assert_eq!(
            guess_language("Ils livreront les pièces mardi prochain.", &all),
            Some(Fr)
        );
        assert_eq!(
            guess_language("D'accord, ça marche pour moi.", &[]),
            Some(Fr)
        );
        assert_eq!(guess_language("Okay.", &all), None);
        assert_eq!(guess_language("Der Pull Request", &[En, Fr]), None);
    }

    #[test]
    fn statistics() {
        let clip = |errors, reference_words| ClipErrors {
            errors,
            reference_words,
        };
        let clips = [clip(1, 10), clip(3, 10), clip(0, 0)];
        assert_eq!(
            (pooled_rate(&clips), macro_rate(&clips)),
            (Some(0.2), Some(0.2))
        );
        assert_eq!(macro_rate(&[clip(1, 2), clip(0, 8)]), Some(0.25));
        assert_eq!((pooled_rate(&[]), macro_rate(&[])), (None, None));
        let toy: Vec<ClipErrors> = (0..20).map(|i| clip(i % 5, 10 + i % 3)).collect();
        let ci = bootstrap_ci(&toy, 2000, 7).unwrap();
        let point = pooled_rate(&toy).unwrap();
        assert_eq!(Some(ci), bootstrap_ci(&toy, 2000, 7));
        assert!(ci.0 < point && point < ci.1, "{ci:?} {point}");
        assert_eq!(bootstrap_ci(&toy[..1], 100, 1), None);
        let identical = paired_bootstrap(&toy, &toy, 1000, 3).unwrap();
        assert_eq!((identical.difference, identical.p_value), (0.0, 1.0));
        let better: Vec<ClipErrors> = (0..30).map(|i| clip(i % 2, 20)).collect();
        let worse: Vec<ClipErrors> = (0..30).map(|i| clip(3 + i % 3, 20)).collect();
        let cmp = paired_bootstrap(&better, &worse, 2000, 11).unwrap();
        assert!(
            cmp.difference < 0.0 && cmp.p_value < 0.05 && cmp.ci95.1 < 0.0,
            "{cmp:?}"
        );
        assert_eq!(paired_bootstrap(&better, &worse[..29], 100, 1), None);
        assert_eq!(percentile(&[3.0, 1.0, 2.0, f64::NAN], 50.0), Some(2.0));
        assert_eq!(percentile(&[1.0, 2.0, 3.0, 4.0], 50.0), Some(2.5));
        assert!((percentile(&[0.0, 10.0], 95.0).unwrap() - 9.5).abs() < 1e-9);
        let ramp: Vec<f64> = (1..=21).map(f64::from).collect();
        assert!((percentile(&ramp, 95.0).unwrap() - 20.0).abs() < 1e-9);
        assert_eq!(percentile(&[], 50.0), None);
    }

    #[test]
    fn never_panics_on_odd_input() {
        let odd = "- | $ | €€ 12. | 1.2.3.4.5 | 9:99 | 99:30 | 1,,2 | ° | 's | l' | -'- | x-'y | 0.0.0 \
            | 12345678901234567890 | q1-q2 | İstanbul 3ème | ½ ² | a.m. | 15.13.2020 | $ 5k | 3 h 99 \
            | n't | 1.000.000.000.000.000";
        for text in odd.split(" | ").chain(["", "   "]) {
            for langs in [&[De][..], &[En], &[Fr], &[De, En, Fr], &[]] {
                let tokens = normalise(text, langs);
                let clean = tokens.iter().all(|t| !t.is_empty() && !t.contains(' '));
                assert!(clean, "{text:?} {tokens:?}");
                score(&ReferenceText::Plain(text), "irgendwas 1", langs);
            }
        }
    }

    #[test]
    fn corpus_scripts() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../docs/asr-benchmark-scripts.json"
        );
        let Ok(raw) = std::fs::read_to_string(path) else {
            eprintln!("skipping corpus test: {path} not found");
            return;
        };
        let json: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let no_digits = |t: &[String]| t.iter().all(|t| !t.contains(|c: char| c.is_ascii_digit()));
        for script in json["scripts"].as_array().unwrap() {
            let id = script["id"].as_str().unwrap();
            let text = script["text"].as_str().unwrap_or("");
            let codes = script["languages"].as_array().cloned().unwrap_or_default();
            let langs: Vec<Lang> = codes
                .iter()
                .filter_map(|c| Lang::from_code(c.as_str()?))
                .collect();
            let tokens = normalise(text, &langs);
            assert!(script["language"] == "none" || !tokens.is_empty(), "{id}");
            assert!(no_digits(&tokens), "{id}: {tokens:?}");
            let s = score(&ReferenceText::Plain(text), text, &langs);
            let errors = s.normalised.errors() + s.formatted.errors() + s.characters.errors();
            assert_eq!((errors, s.numbers_correct), (0, s.numbers_total), "{id}");
            assert_eq!(
                text.contains(|c: char| c.is_ascii_digit()),
                s.numbers_total > 0,
                "{id}"
            );
            let Some(segments) = script["segments"].as_array() else {
                continue;
            };
            let segments: Vec<(Lang, String)> = segments
                .iter()
                .filter_map(|s| {
                    Some((
                        Lang::from_code(s["lang"].as_str()?)?,
                        s["text"].as_str()?.into(),
                    ))
                })
                .collect();
            assert!(no_digits(&normalise_segments(&segments)), "{id}");
            let s = score(&ReferenceText::Segments(&segments), text, &langs);
            let errors: Vec<_> = s.alignment.iter().filter(|p| p.op != Op::Match).collect();
            assert!(errors.is_empty(), "{id}: {errors:?}");
        }
    }
}
