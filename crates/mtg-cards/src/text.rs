//! Phrase-level oracle text parsing: normalization, numbers, object
//! filters, target specs, keyword lists. Precision-first: any unknown word
//! fails the parse and lets the caller downgrade coverage instead of
//! guessing.

use mtg_ir::{
    CardTypes, Cmp, ColorSet, KeywordSet, ObjFilter, PlayerFilter, SpellFilter, Supertypes,
    TargetCount, TargetSpec, TargetWhat, ValueExpr, Whose,
};

/// Normalize a face's oracle text: strip reminders, self-name to "~",
/// lowercase, tidy whitespace and unicode punctuation.
pub fn normalize(oracle_text: &str, card_name: &str, face_name: &str) -> String {
    let mut t = crate::compiler::strip_reminders(oracle_text);
    // Replace self references, longest first. "Krenko, Mob Boss" also goes
    // by "Krenko" in its own text.
    let mut names: Vec<String> = vec![card_name.to_string(), face_name.to_string()];
    if let Some((short, _)) = card_name.split_once(',') {
        names.push(short.to_string());
    }
    if let Some((short, _)) = face_name.split_once(',') {
        names.push(short.to_string());
    }
    names.sort_by_key(|n| std::cmp::Reverse(n.len()));
    names.dedup();
    for n in &names {
        if !n.is_empty() {
            t = t.replace(n.as_str(), "~");
        }
    }
    let t = t
        .replace('\u{2212}', "-") // minus sign on planeswalkers
        .replace('\u{2014}', "-") // em dash on escape and ability words
        .replace('\u{2022}', "*") // modal bullets
        .replace("this creature", "~")
        .replace("this permanent", "~")
        .replace("this spell", "~")
        .replace("this card", "~")
        .replace("this land", "~")
        .replace("this artifact", "~")
        .replace("this enchantment", "~")
        .replace("this planeswalker", "~");
    let mut out = String::with_capacity(t.len());
    for (i, line) in t.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if i > 0 || !out.is_empty() {
            out.push('\n');
        }
        let mut last_space = false;
        for c in line.chars() {
            if c.is_whitespace() {
                if !last_space {
                    out.push(' ');
                }
                last_space = true;
            } else {
                out.extend(c.to_lowercase());
                last_space = false;
            }
        }
    }
    out
}

/// "a", "three", "12", "x" and friends.
pub fn parse_count_word(w: &str) -> Option<ValueExpr> {
    Some(match w {
        "a" | "an" | "one" => ValueExpr::Fixed(1),
        "two" => ValueExpr::Fixed(2),
        "three" => ValueExpr::Fixed(3),
        "four" => ValueExpr::Fixed(4),
        "five" => ValueExpr::Fixed(5),
        "six" => ValueExpr::Fixed(6),
        "seven" => ValueExpr::Fixed(7),
        "eight" => ValueExpr::Fixed(8),
        "nine" => ValueExpr::Fixed(9),
        "ten" => ValueExpr::Fixed(10),
        "eleven" => ValueExpr::Fixed(11),
        "twelve" => ValueExpr::Fixed(12),
        "thirteen" => ValueExpr::Fixed(13),
        "twenty" => ValueExpr::Fixed(20),
        "x" => ValueExpr::X,
        _ => ValueExpr::Fixed(w.parse::<i32>().ok()?),
    })
}

pub fn fixed_count(w: &str) -> Option<u8> {
    match parse_count_word(w)? {
        ValueExpr::Fixed(n) if (0..=250).contains(&n) => Some(n as u8),
        _ => None,
    }
}

fn color_word(w: &str) -> Option<ColorSet> {
    Some(match w {
        "white" => ColorSet::W,
        "blue" => ColorSet::U,
        "black" => ColorSet::B,
        "red" => ColorSet::R,
        "green" => ColorSet::G,
        _ => return None,
    })
}

fn type_word(w: &str) -> Option<CardTypes> {
    Some(match w.trim_end_matches('s') {
        "artifact" => CardTypes::ARTIFACT,
        "battle" => CardTypes::BATTLE,
        "creature" => CardTypes::CREATURE,
        "enchantment" => CardTypes::ENCHANTMENT,
        "instant" => CardTypes::INSTANT,
        "sorcerie" | "sorcery" => CardTypes::SORCERY,
        "land" => CardTypes::LAND,
        "planeswalker" => CardTypes::PLANESWALKER,
        _ => return None,
    })
}

/// A word that reads as a keyword in "with/has/gains" lists.
pub fn keyword_phrase(s: &str) -> Option<KeywordSet> {
    KeywordSet::from_scryfall(s).or(match s {
        "can't block" => Some(KeywordSet::CANT_BLOCK),
        "can't be blocked" => Some(KeywordSet::UNBLOCKABLE),
        _ => None,
    })
}

/// Parse "flying and trample", "flying, first strike, and lifelink".
pub fn parse_keyword_list(s: &str) -> Option<KeywordSet> {
    let mut out = KeywordSet::empty();
    let s = s.trim().trim_end_matches('.');
    for chunk in s.split(',') {
        for part in chunk.split(" and ") {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            out |= keyword_phrase(part)?;
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// Known subtypes we allow in filters without a vocabulary of every
/// creature type: any single word is accepted as a subtype when it sits
/// directly before a type word. These two get special casing because they
/// stand alone.
fn standalone_subtype(w: &str) -> Option<(CardTypes, &'static str)> {
    match w.trim_end_matches('s') {
        "aura" => Some((CardTypes::ENCHANTMENT, "aura")),
        "equipment" => Some((CardTypes::ARTIFACT, "equipment")),
        // Basic land type words: "search ... for a mountain or island card".
        "plain" => Some((CardTypes::LAND, "plains")),
        "island" => Some((CardTypes::LAND, "island")),
        "swamp" => Some((CardTypes::LAND, "swamp")),
        "mountain" => Some((CardTypes::LAND, "mountain")),
        "forest" => Some((CardTypes::LAND, "forest")),
        "waste" => Some((CardTypes::LAND, "wastes")),
        _ => None,
    }
}

/// Parse an object noun phrase into a filter: "another attacking creature",
/// "nonland permanent an opponent controls", "goblin creature you control",
/// "artifact or enchantment", "basic land card".
///
/// Returns the filter and whether the phrase referenced "card" (a zone
/// object rather than a permanent).
pub fn parse_obj_phrase(phrase: &str) -> Option<(ObjFilter, bool)> {
    let mut f = ObjFilter::default();
    let mut is_card = false;
    let mut saw_type = false;
    let phrase = phrase.trim().trim_end_matches('.');

    // Possession suffixes.
    let mut body = phrase;
    for (suffix, whose) in [
        (" you control", Whose::You),
        (" you don't control", Whose::Opponents),
        (" an opponent controls", Whose::Opponents),
        (" your opponents control", Whose::Opponents),
        (" you own", Whose::You),
    ] {
        if let Some(b) = body.strip_suffix(suffix) {
            body = b;
            f.controller = whose;
            break;
        }
    }

    // "with/without <keywords>" and value clauses.
    loop {
        if let Some(idx) = body.rfind(" with ") {
            let clause = &body[idx + 6..];
            if let Some(kw) = parse_keyword_list(clause) {
                f.with_keywords |= kw;
                body = &body[..idx];
                continue;
            }
            if let Some((field, cmp, n)) = parse_value_clause(clause) {
                match field {
                    ValueField::ManaValue => f.mana_value = Some((cmp, n)),
                    ValueField::Power => f.power = Some((cmp, n)),
                    ValueField::Toughness => f.toughness = Some((cmp, n)),
                }
                body = &body[..idx];
                continue;
            }
            return None;
        }
        if let Some(idx) = body.rfind(" without ") {
            let clause = &body[idx + 9..];
            if let Some(kw) = parse_keyword_list(clause) {
                f.without_keywords |= kw;
                body = &body[..idx];
                continue;
            }
            return None;
        }
        break;
    }

    // Alternative types joined by "or": union.
    for alt in body.split(" or ") {
        let mut words: Vec<&str> = alt.split_whitespace().collect();
        // A trailing "card"/"cards" marks zone objects; "token" marks tokens;
        // "permanent" is a wildcard.
        while let Some(&last) = words.last() {
            match last.trim_end_matches('s') {
                "card" => {
                    is_card = true;
                    words.pop();
                }
                "token" => {
                    f.is_token = Some(true);
                    words.pop();
                }
                "permanent" => {
                    saw_type = true;
                    words.pop();
                }
                "spell" => return None,
                _ => break,
            }
        }
        let mut i = 0;
        while i < words.len() {
            let w = words[i];
            let rest_has_type = words[i + 1..].iter().any(|w| type_word(w).is_some());
            match w {
                "another" | "other" => f.other_than_self = true,
                "each" | "all" | "a" | "an" | "the" => {}
                "tapped" => f.tapped = Some(true),
                "untapped" => f.tapped = Some(false),
                "attacking" => {
                    if words.get(i + 1) == Some(&"or") {
                        // handled by the or-split; treat as attacking here
                    }
                    f.attacking = Some(true)
                }
                "blocking" => f.blocking = Some(true),
                "basic" => f.supertypes |= Supertypes::BASIC,
                "legendary" => f.supertypes |= Supertypes::LEGENDARY,
                "snow" => f.supertypes |= Supertypes::SNOW,
                "nonbasic" => f.not_supertypes |= Supertypes::BASIC,
                "nonlegendary" => f.not_supertypes |= Supertypes::LEGENDARY,
                "nontoken" => f.is_token = Some(false),
                "nonland" => f.not_types |= CardTypes::LAND,
                "noncreature" => f.not_types |= CardTypes::CREATURE,
                "nonartifact" => f.not_types |= CardTypes::ARTIFACT,
                "nonwhite" => f.not_colors |= ColorSet::W,
                "nonblue" => f.not_colors |= ColorSet::U,
                "nonblack" => f.not_colors |= ColorSet::B,
                "nonred" => f.not_colors |= ColorSet::R,
                "nongreen" => f.not_colors |= ColorSet::G,
                _ => {
                    if let Some(c) = color_word(w) {
                        f.colors_any |= c;
                    } else if let Some(t) = type_word(w) {
                        f.types |= t;
                        saw_type = true;
                    } else if let Some((t, sub)) = standalone_subtype(w) {
                        f.types |= t;
                        f.subtypes_any.push(sub.into());
                        saw_type = true;
                    } else if rest_has_type && w.chars().all(|c| c.is_ascii_alphabetic()) {
                        // Word before a type word reads as a subtype.
                        for v in subtype_variants(w) {
                            f.subtypes_any.push(v);
                        }
                    } else if i == words.len() - 1
                        && !saw_type
                        && w.chars().all(|c| c.is_ascii_alphabetic())
                    {
                        // Bare tribal group: "other merfolk", "goblins you
                        // control". Reads as creatures of that subtype; an
                        // unknown word matches nothing, which is harmless.
                        f.types |= CardTypes::CREATURE;
                        for v in subtype_variants(w) {
                            f.subtypes_any.push(v);
                        }
                        saw_type = true;
                    } else {
                        return None;
                    }
                }
            }
            i += 1;
        }
    }
    if !saw_type && f.types.is_empty() && f.subtypes_any.is_empty() {
        return None;
    }
    Some((f, is_card))
}

/// Singular and plural forms so "elves" matches the oracle subtype "elf".
fn subtype_variants(w: &str) -> Vec<Box<str>> {
    let w = w.trim_end_matches('\'');
    let mut out: Vec<Box<str>> = vec![w.into()];
    if let Some(stem) = w.strip_suffix("ves") {
        out.push(format!("{stem}f").into()); // elves -> elf, wolves -> wolf
    }
    if let Some(stem) = w.strip_suffix("ies") {
        out.push(format!("{stem}y").into()); // zombies handled by s-strip too
    }
    if let Some(stem) = w.strip_suffix('s') {
        out.push(stem.into());
    }
    out.dedup();
    out
}

pub enum ValueField {
    ManaValue,
    Power,
    Toughness,
}

/// "mana value 3 or less", "power 4 or greater".
pub fn parse_value_clause(s: &str) -> Option<(ValueField, Cmp, i32)> {
    let s = s.trim().trim_end_matches('.');
    let (field, rest) = if let Some(r) = s.strip_prefix("mana value ") {
        (ValueField::ManaValue, r)
    } else if let Some(r) = s.strip_prefix("power ") {
        (ValueField::Power, r)
    } else if let Some(r) = s.strip_prefix("toughness ") {
        (ValueField::Toughness, r)
    } else {
        return None;
    };
    let words: Vec<&str> = rest.split_whitespace().collect();
    let n: i32 = words.first()?.parse().ok()?;
    let cmp = match (words.get(1), words.get(2)) {
        (Some(&"or"), Some(&"less")) => Cmp::Le,
        (Some(&"or"), Some(&"greater")) => Cmp::Ge,
        (None, None) => Cmp::Eq,
        _ => return None,
    };
    Some((field, cmp, n))
}

/// Parse a target phrase at the start of `s`. Returns the spec and the rest
/// of the sentence after the phrase.
pub fn parse_target_phrase(s: &str) -> Option<(TargetSpec, &str)> {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix("any target") {
        return Some((TargetSpec::one(TargetWhat::AnyDamageable), rest));
    }
    let (count, s) = if let Some(rest) = s.strip_prefix("up to ") {
        let mut it = rest.splitn(2, ' ');
        let n = fixed_count(it.next()?)?;
        (TargetCount::UpTo(n), it.next()?.trim_start())
    } else {
        (TargetCount::Exactly(1), s)
    };
    let rest = s.strip_prefix("target ").or_else(|| s.strip_prefix("targets "))?;

    // Player targets.
    if let Some(r) = rest.strip_prefix("player or planeswalker") {
        return Some((TargetSpec { count, what: TargetWhat::PlayerOrPlaneswalker }, r));
    }
    if let Some(r) = rest.strip_prefix("player") {
        return Some((TargetSpec { count, what: TargetWhat::Player(PlayerFilter::Any) }, r));
    }
    if let Some(r) = rest.strip_prefix("opponent") {
        return Some((TargetSpec { count, what: TargetWhat::Player(PlayerFilter::Opponent) }, r));
    }
    // Spell targets.
    if let Some(spell_end) = rest.find("spell") {
        let before = &rest[..spell_end];
        if before.split_whitespace().all(|w| {
            matches!(w, "noncreature" | "instant" | "sorcery" | "creature" | "artifact" | "enchantment" | "planeswalker" | "or" | "a" | "an")
        }) {
            let mut sf = SpellFilter::default();
            for w in before.split_whitespace() {
                match w {
                    "noncreature" => sf.not_types |= CardTypes::CREATURE,
                    "instant" => sf.types |= CardTypes::INSTANT,
                    "sorcery" => sf.types |= CardTypes::SORCERY,
                    "creature" => sf.types |= CardTypes::CREATURE,
                    "artifact" => sf.types |= CardTypes::ARTIFACT,
                    "enchantment" => sf.types |= CardTypes::ENCHANTMENT,
                    "planeswalker" => sf.types |= CardTypes::PLANESWALKER,
                    _ => {}
                }
            }
            let after = &rest[spell_end + 5..];
            return Some((TargetSpec { count, what: TargetWhat::SpellOnStack(sf) }, after));
        }
    }

    // Object targets: find the end of the noun phrase. Cut at known
    // boundary words so the caller keeps the rest of the sentence.
    let boundaries = [". ", ", ", " to ", " until ", " unless ", " and ", " if ", " gets ", " gains ", " gain ", " deals ", " you control", " you don't control", " an opponent controls"];
    // Possession is part of the phrase: try longest prefix first.
    let mut best: Option<(TargetSpec, &str)> = None;
    let mut cut_positions: Vec<usize> = vec![rest.len()];
    for b in boundaries {
        let mut from = 0;
        while let Some(i) = rest[from..].find(b) {
            let pos = from + i;
            let end = if b.starts_with(' ') { pos } else { pos + 1 };
            cut_positions.push(end);
            // Possession suffixes belong to the phrase.
            if b.contains("control") {
                cut_positions.push(pos + b.len());
            }
            from = pos + b.len().max(1);
        }
    }
    cut_positions.sort_unstable();
    cut_positions.dedup();
    // Longest phrase that parses wins.
    for &end in cut_positions.iter().rev() {
        let phrase = rest[..end].trim().trim_end_matches(&['.', ','][..]);
        if phrase.is_empty() {
            continue;
        }
        // Graveyard targets.
        if let Some(idx) = phrase.find(" from your graveyard") {
            if let Some((f, _)) = parse_obj_phrase(&phrase[..idx]) {
                best = Some((
                    TargetSpec { count, what: TargetWhat::CardInGraveyard(f, Whose::You) },
                    &rest[end.min(rest.len())..],
                ));
                break;
            }
        }
        if let Some(idx) = phrase.find(" from a graveyard") {
            if let Some((f, _)) = parse_obj_phrase(&phrase[..idx]) {
                best = Some((
                    TargetSpec { count, what: TargetWhat::CardInGraveyard(f, Whose::Any) },
                    &rest[end.min(rest.len())..],
                ));
                break;
            }
        }
        if let Some((f, is_card)) = parse_obj_phrase(phrase) {
            if !is_card {
                best = Some((
                    TargetSpec { count, what: TargetWhat::Permanent(f) },
                    &rest[end.min(rest.len())..],
                ));
                break;
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn obj_phrases() {
        let (f, _) = parse_obj_phrase("another attacking creature").unwrap();
        assert!(f.other_than_self);
        assert_eq!(f.attacking, Some(true));
        assert!(f.types.contains(CardTypes::CREATURE));

        let (f, _) = parse_obj_phrase("nonland permanent an opponent controls").unwrap();
        assert!(f.not_types.contains(CardTypes::LAND));
        assert_eq!(f.controller, Whose::Opponents);

        let (f, _) = parse_obj_phrase("goblin creature you control").unwrap();
        assert_eq!(f.subtypes_any, vec!["goblin".into()]);

        assert!(parse_obj_phrase("frobnicated whatsit").is_none());
    }

    #[test]
    fn target_phrases() {
        let (spec, rest) = parse_target_phrase("target creature gets +2/+2").unwrap();
        assert!(matches!(spec.what, TargetWhat::Permanent(_)));
        assert!(rest.trim_start().starts_with("gets"));

        let (spec, _) = parse_target_phrase("any target.").unwrap();
        assert!(matches!(spec.what, TargetWhat::AnyDamageable));

        let (spec, _) = parse_target_phrase("up to two target creatures").unwrap();
        assert!(matches!(spec.count, TargetCount::UpTo(2)));

        let (spec, _) =
            parse_target_phrase("target creature card from your graveyard to your hand").unwrap();
        assert!(matches!(spec.what, TargetWhat::CardInGraveyard(..)));
    }

    #[test]
    fn keyword_lists() {
        let kw = parse_keyword_list("flying, first strike, and lifelink").unwrap();
        assert!(kw.contains(KeywordSet::FLYING));
        assert!(kw.contains(KeywordSet::FIRST_STRIKE));
        assert!(kw.contains(KeywordSet::LIFELINK));
        assert!(parse_keyword_list("flying and bananas").is_none());
    }
}
