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
tolaria fetch                                  # pull card data and the meta
tolaria run --deck my_deck.txt --format modern # headless gauntlet
tolaria                                        # the TUI
```

Simulations are deterministic per seed. Same seed, same carnage.

## Data sources and thanks

Card data from [Scryfall](https://scryfall.com). Tournament decklists from
the MTGO decklist cache projects (Badaro, Jiliac, fbettega). Commander data
from [EDHREC](https://edhrec.com). Archetype rules from MTGOFormatData.

## License

GPLv3. Unofficial fan project. Magic: The Gathering is the property of
Wizards of the Coast. Tolaria is not affiliated with or endorsed by WotC.
