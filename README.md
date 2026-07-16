# Tolaria

A terminal laboratory for Magic: The Gathering decks. Hand it your decklist,
and it plays a few thousand games against the actual meta while you're still
deciding whether to keep a two-lander.

Named for the island of rigorous wizard academia and at least one catastrophic
timestream incident. Tolaria runs your matches in a time bubble: a thousand
games happen in seconds, nobody has to shuffle, and the only thing that leaves
the bubble is a win-rate table.

## What it does

- Ingests your deck as a plain text file. MTGA exports, MTGO exports, or a
  bare `4x Lightning Bolt` list all work.
- Pulls real tournament decklists for Standard, Pioneer, Modern, Legacy,
  Vintage, and Pauper from public MTGO and paper event archives, plus EDHREC
  data for Commander. No screen scraping, no API keys, no accounts.
- Computes the metagame from actual archetype frequency in recent events,
  then builds a gauntlet of the decks people genuinely play.
- Simulates full games in its own Rust rules engine: priority, the stack,
  combat damage ordering, state-based actions, the London mulligan. Judge
  not included, but rule 704 is on call.
- Reports each matchup with win rates and confidence intervals, on the play
  and on the draw, so you can see exactly where your deck feasts and where
  it folds. All of it in a terminal UI.

## The honesty clause

Magic has around 35,000 unique cards and Tolaria will accept every one of
them, but reading the card does not always fully explain the card. Each card
compiles to a coverage tier:

| Tier | Meaning |
|------|---------|
| Full | Faithfully modeled. |
| Partial | Main effect modeled, listed riders dropped. |
| Proxy | Correct body and keywords, text treated as flavor. |
| Unplayable | Sits in the deck looking pretty. |

Deck coverage is shown next to every result, so a 62% win rate built on
Proxy-tier jank announces itself. Separately, the pilot is a solid
curve-out heuristic: it attacks well, blocks sensibly, and will absolutely
not execute your seven-card storm line with judge-level precision. Combo and
control decks get flagged with a pilot-fidelity warning rather than a
quietly wrong number.

## Quickstart

```
tolaria fetch                                  # pull card data
tolaria run --deck my_deck.txt --format modern # headless gauntlet vs the meta
tolaria                                        # the TUI
```

Simulations are deterministic per seed. Same seed, same carnage.

## Commands

Card data downloads automatically on first use and refreshes when Scryfall
publishes a new bulk (about every 12 hours). Tournament data syncs at most
once every six hours. Everything caches under the platform data directory
(`%LOCALAPPDATA%\modusimagery\Tolaria` on Windows).

Formats: `standard`, `pioneer`, `modern`, `legacy`, `vintage`, `pauper`,
`commander` (also `edh`).

### Game counts, early stopping, and auto

`--games` takes a number or `auto`. A number is a ceiling, not a quota:
after a 200-game floor, a matchup stops as soon as its 95% confidence
interval clears 50%, since more games cannot change the verdict. The output
says when and why this happened.

- `--games 5000` plays up to 5000, stopping early once decided
- `--games 5000 --no-early-stop` plays exactly 5000
- `--games auto --precision 0.5` ignores fixed counts and plays until the
  CI half-width is 0.5 percentage points (1000-game floor, million-game
  ceiling)

### tolaria / tolaria tui

Bare `tolaria` launches the terminal UI. Bare `tolaria` with run flags is
shorthand for `tolaria run`: `tolaria --deck x.txt --format vintage` runs
the gauntlet headless with all the options listed under `run`.

| Option | Default | Meaning |
|---|---|---|
| `--deck <file>` | none | preload a decklist path |
| `--format <name>` | `modern` | preload a format |
| `--games <n>` | `1000` | games per matchup |

### tolaria fetch

Download or refresh the Scryfall card database.

| Option | Default | Meaning |
|---|---|---|
| `--force` | off | recheck the manifest even if the local cache is fresh |

### tolaria card <name>

Print a card's oracle data (faces, types, text, legality). Multiple words
work without quotes. Unresolved names get closest-match suggestions.

### tolaria compile <name>

Compile one card and print its coverage tier, dropped clauses, and parsed
behaviors. The debugging window into the honesty clause.

### tolaria coverage

Compile the entire card pool and print the tier histogram.

### tolaria fetch-meta

Sync tournament data and print the computed metagame without simulating.

| Option | Default | Meaning |
|---|---|---|
| `--format <name>` | `modern` | which meta to compute |
| `--days <n>` | `60` | trailing tournament window |
| `--top <n>` | `12` | archetypes to keep |

### tolaria run

Your deck against the format's meta gauntlet: syncs tournament data,
classifies archetypes, builds consensus lists, simulates every matchup, and
prints the worst-first table with confidence intervals and coverage.

| Option | Default | Meaning |
|---|---|---|
| `--deck <file>` | required | your decklist |
| `--format <name>` | `modern` | gauntlet format |
| `--games <n\|auto>` | `1000` | per-matchup games, see above |
| `--precision <pp>` | `1.0` | auto mode CI half-width, percentage points |
| `--days <n>` | `60` | trailing tournament window |
| `--top <n>` | `12` | gauntlet size |
| `--seed <n>` | `0x544f4c41524941` | master seed ("TOLARIA") |
| `--json <file>` | none | write full results as JSON |
| `--no-early-stop` | off | play every requested game |

### tolaria duel

One deck against another, both from files.

| Option | Default | Meaning |
|---|---|---|
| `--deck <file>` | required | your decklist |
| `--vs <file>` | required | the opposing decklist |
| `--games <n\|auto>` | `1000` | see above |
| `--precision <pp>` | `1.0` | auto mode CI half-width |
| `--seed <n>` | `0x544f4c41524941` | master seed |
| `--no-early-stop` | off | play every requested game |
| `--all-hands` | off | exhaustive opening-hand sweep, see below |
| `--per-hand <n>` | `50` | continuations per hand in sweep mode |

`--all-hands` enumerates every distinct opening seven your deck can be
dealt, weights each by its exact hypergeometric probability, and plays
`--per-hand` continuations per hand. Full deck-order enumeration is not a
thing any computer will ever finish (a 60-card deck has on the order of
10^63 orderings); the opener is where deal variance lives, and the opener
is exactly enumerable. Output includes the weighted win rate with a
stratified confidence interval plus your best and worst opening hands.
Singleton decks exceed the sweep limit by design; use `--games auto` there.

### tolaria pod

Four-player Commander pods: you plus three opponents sampled from the
EDHREC meta by share, every game. An even pod baseline is 25%.

| Option | Default | Meaning |
|---|---|---|
| `--deck <file>` | required | your Commander decklist |
| `--games <n>` | `250` | pods to play |
| `--top <n>` | `10` | commander meta pool size |
| `--seed <n>` | `0x544f4c41524941` | master seed |

The commander comes from a `Commander` section in the decklist, or the
first card when no section names one.

### Decklist formats

MTGA exports (`4 Lightning Bolt (M11) 133` with `Deck`, `Sideboard`,
`Commander`, `Companion` sections), MTGO text (blank line starts the
sideboard), and plain lists (`4x Lightning Bolt`). Comment lines start with
`#` or `//`. Sideboards are parsed and ignored: simulations are game one.

## Data sources and thanks

Card data from [Scryfall](https://scryfall.com). Tournament decklists from
the MTGO decklist cache projects (Badaro, Jiliac, fbettega). Commander data
from [EDHREC](https://edhrec.com). Archetype rules from MTGOFormatData.

## License

GPLv3. Unofficial fan project. Magic: The Gathering is the property of
Wizards of the Coast. Tolaria is not affiliated with or endorsed by WotC.
