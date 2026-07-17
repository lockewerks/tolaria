# Tolaria

A desktop laboratory for Magic: The Gathering decks. Hand it your decklist
and it plays a few thousand games against the actual meta while you're
still pile-shuffling and telling yourself the two-lander has "good
texture." It does not have good texture. Tolaria has receipts.

Named for the island of rigorous wizard academia and at least one
catastrophic timestream incident. Tolaria runs your matches in a time
bubble: thousands of games happen in seconds, nobody has to shuffle, nobody
gets salty, and the only thing that leaves the bubble is a win-rate table
with confidence intervals. The table does not care about your feelings.
That's the point. That's the whole product.

## What it does

- Ingests your deck as a plain text file. MTGA exports, MTGO exports, or a
  bare `4x Lightning Bolt` list scrawled at 2am all work.
- Pulls real tournament decklists for Standard, Pioneer, Modern, Legacy,
  Vintage, and Pauper from public MTGO and paper event archives, plus
  EDHREC data for Commander. No screen scraping, no API keys, no accounts,
  no "please verify you are not a robot." I am a robot. It's fine.
- Computes the metagame from actual archetype frequency in recent events,
  then builds a gauntlet of decks people genuinely play, so when you lose
  to Tron eleven hundred times, it's the real Tron. You conceded to lands.
  Statistically.
- Simulates full games in its own Rust rules engine: priority, the stack,
  combat damage ordering, state-based actions, the London mulligan. Judge
  not included, but rule 704 is on call and deathtouch-trample math is
  resolved correctly, which is more than can be said for most kitchen
  tables.
- Reports each matchup with win rates and Wilson confidence intervals, on
  the play and on the draw, plus how hard the beatings were in both
  directions. Whether you're a Timmy who wants the overkill numbers, a
  Johnny goldfishing a masterpiece, or a Spike hunting half a percent, the
  data's there. Your deck is probably a pile. Now it's a pile with error
  bars.

## The honesty clause

Magic has around 35,000 unique cards and Tolaria accepts every single one
of them, but reading the card does not always fully explain the card. Each
card compiles to a coverage tier:

| Tier | Meaning |
|------|---------|
| Full | Faithfully modeled. |
| Partial | Main effect modeled, listed riders dropped. |
| Proxy | Correct body and keywords, text treated as flavor. |
| Unplayable | Sits in the deck looking pretty. |

A Proxy-tier card is a bear with delusions of grandeur: right power,
right toughness, and the rules text might as well say "when this enters,
gain 1 vibe." Deck coverage is stapled to every result, so a 62% win rate
built on Proxy jank announces itself instead of quietly lying to you.

Separately: the pilot is a solid curve-out heuristic. It hits land drops,
attacks well, blocks sensibly, bolts the bird, and will absolutely not
execute your seven-card storm line with judge-level precision. Combo and
control decks get flagged with a pilot-fidelity warning rather than a
confidently wrong number. If your win condition requires holding up three
counterspells and a smug expression, read those matchups with squinted
eyes.

## Performance

The simulation engine is fully multi-threaded: games fan out across every
core you own via work-stealing parallelism, each game on its own seeded
RNG stream. Measured on the dev box, a frankly irresponsible 32-thread
workstation:

- Duels: ~35,000 games/second
- Meta gauntlets: ~15,000 games/second
- All-hands sweeps: ~31,000 games/second (3.5 million games in under two
  minutes)
- Goldfish: ~64,000 games/second, because the opponent is a pile of
  Wastes piloted by nobody

Your mileage will vary. A laptop does less. A potato still does thousands
per second, which is more games of Magic than you have played in your
life, per second, on a potato.

Two promises about the randomness, because someone always asks: the
shuffler is a true Fisher-Yates over a seeded PCG stream, and simulations
are deterministic per seed. Same seed, same carnage, byte for byte. When
you flood out five games straight, that is not the shuffler being rigged.
That's just Magic. Lands clump. Take it up with variance.

## Quickstart

```
tolaria fetch                                  # pull card data
tolaria run --deck my_deck.txt --format modern # headless gauntlet vs the meta
tolaria                                        # the desktop app
```

## Desktop app (the default)

The Tauri v2 desktop UI in `crates/tolaria-desktop` is the primary
interface: deck manager with per-card coverage inspector, format-fit
analysis with a best-fit recommendation, run configurator for all five
modes, live progress with cancel, results with charts and per-matchup
drill-ins (game length, end reasons, mulligans, victory margins), meta
browser, persisted run history, dwell tooltips on every term, and a
glossary for anything cryptic. Build it with:

```
cd crates/tolaria-desktop/ui && npm install && npm run build
cargo build --release -p tolaria-desktop
target\release\tolaria-desktop.exe
```

Requires Node 18+ for the frontend build and WebView2 (ships with Windows
11). Bare `tolaria` launches the desktop app when the two binaries sit next
to each other. The CLI below is the headless interface.

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

### tolaria

Bare `tolaria` launches the desktop app when `tolaria-desktop.exe` sits
next to it. Bare `tolaria` with run flags is shorthand for `tolaria run`:
`tolaria --deck x.txt --format vintage` runs the gauntlet headless with all
the options listed under `run`.

### tolaria goldfish

The deck against a passive opponent that never acts: pure speed and
consistency, zero interaction, any deck size. Reports average kill turn,
kill-by-turn percentages, and mulligan rates. The purest form of Magic:
the opponent does nothing and you win anyway. EDH players know.

| Option | Default | Meaning |
|---|---|---|
| `--deck <file>` | required | your decklist |
| `--games <n>` | `1000` | games to play |
| `--seed <n>` | random | master seed; omitted, a fresh one is rolled and printed |

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
| `--top <n\|all>` | `12` | gauntlet size; `all` takes every eligible archetype |
| `--random` | off | draw the gauntlet at random from the eligible universe |

The output reports the archetype universe: how many archetypes the window
saw, how many are eligible (3 or more lists behind the consensus), and how
many decks classified.

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
| `--top <n\|all>` | `12` | gauntlet size; `all` fights every eligible archetype |
| `--random` | off | draw the gauntlet at random from the eligible universe |
| `--seed <n>` | random | master seed; omitted, a fresh one is rolled and printed |
| `--json <file>` | none | write full results as JSON |
| `--no-early-stop` | off | play every requested game |

### tolaria duel

One deck against another, both from files. Settle the argument.

| Option | Default | Meaning |
|---|---|---|
| `--deck <file>` | required | your decklist |
| `--vs <file>` | required | the opposing decklist |
| `--games <n\|auto>` | `1000` | see above |
| `--precision <pp>` | `1.0` | auto mode CI half-width |
| `--seed <n>` | random | master seed; omitted, a fresh one is rolled and printed |
| `--no-early-stop` | off | play every requested game |
| `--all-hands` | off | exhaustive opening-hand sweep, see below |
| `--per-hand <n>` | `50` | continuations per hand in sweep mode |

`--all-hands` enumerates every distinct opening seven your deck can be
dealt, weights each by its exact hypergeometric probability, and plays
`--per-hand` continuations per hand. Full deck-order enumeration is not a
thing any computer will ever finish (a 60-card deck has on the order of
10^63 orderings, which is more shuffles than the universe has time for);
the opener is where deal variance lives, and the opener is exactly
enumerable. Output includes the weighted win rate with a stratified
confidence interval plus your best and worst opening hands, so you can
finally see, with math, that the hand you always keep is the hand that
kills you.

### tolaria pod

Four-player Commander pods: you plus three opponents sampled from the
EDHREC meta by share, every game. An even pod baseline is 25%, and the AI
holds no grudges from last week, which makes it politically superior to
your actual playgroup.

| Option | Default | Meaning |
|---|---|---|
| `--deck <file>` | required | your Commander decklist |
| `--games <n>` | `250` | pods to play |
| `--top <n>` | `10` | commander meta pool size |
| `--seed <n>` | random | master seed; omitted, a fresh one is rolled and printed |

The commander comes from a `Commander` section in the decklist, or the
first card when no section names one.

### Decklist formats

MTGA exports (`4 Lightning Bolt (M11) 133` with `Deck`, `Sideboard`,
`Commander`, `Companion` sections), MTGO text (blank line starts the
sideboard), and plain lists (`4x Lightning Bolt`). Comment lines start with
`#` or `//`. Sideboards are parsed and ignored: simulations are game one,
where the honest decks live.

## Releasing

Push a `v*` tag and GitHub Actions builds, signs (Azure Trusted Signing,
publisher Specter Point Intelligence, LLC), and publishes the NSIS
installer plus standalone signed binaries with SHA256 sums. The workflow
needs three repository secrets for the signing service principal:
`AZURE_TENANT_ID`, `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`. Validate
without publishing via the workflow's manual dispatch. Local signing: set
the same three environment variables, then `scripts\sign.ps1 <file>`;
local installer: `ui\node_modules\.bin\tauri.cmd build` from
`crates\tolaria-desktop` (set `TOLARIA_SKIP_SIGN=1` to build unsigned).

## Tip jar

If Tolaria talked you out of sleeving up a 46% pile the night before FNM,
or into keeping the deck everyone said dies to removal (everything dies to
removal, that's what removal is), the tip jar is open:

**[buymeacoffee.com/turboextreme](https://buymeacoffee.com/turboextreme)**

Coffee converts directly into card coverage. No pressure; the sims run
just as fast either way.

## Data sources and thanks

Card data from [Scryfall](https://scryfall.com), the load-bearing pillar
of every Magic project on the internet. Tournament decklists from the MTGO
decklist cache projects (Badaro, Jiliac, fbettega), who do the unglamorous
work so the rest of us can have nice things. Commander data from
[EDHREC](https://edhrec.com). Archetype rules from MTGOFormatData.

## License

GPLv3. Unofficial fan project. Magic: The Gathering is the property of
Wizards of the Coast; Tolaria is not affiliated with or endorsed by WotC,
and no cards were harmed in the simulation of several hundred million
games. Losses to Tron were, regrettably, real.
