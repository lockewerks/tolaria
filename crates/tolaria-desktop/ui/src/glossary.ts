// Single source of truth for term explanations: feeds both the dwell
// tooltips and the Glossary page.

export interface GlossaryEntry {
  title: string;
  text: string;
  group: "Modes" | "Setup" | "Results" | "Coverage";
}

export const GLOSSARY: Record<string, GlossaryEntry> = {
  "mode-gauntlet": {
    title: "Gauntlet",
    text: "Your deck against the format's computed metagame: real tournament decklists from the trailing window, one consensus list per archetype, weighted by how often each archetype actually shows up.",
    group: "Modes",
  },
  "mode-duel": {
    title: "Duel",
    text: "Your deck against one specific saved deck, head to head.",
    group: "Modes",
  },
  "mode-sweep": {
    title: "All hands (sweep)",
    text: "Enumerates every distinct opening seven your deck can be dealt, weights each by its exact hypergeometric probability, and plays a fixed number of continuations per hand. Exact where exactness is possible; full deck-order enumeration is mathematically impossible (about 10^63 orderings for a 60-card deck).",
    group: "Modes",
  },
  "mode-pod": {
    title: "Commander pods",
    text: "Four-player games: you plus three opponents sampled from the EDHREC commander meta by share. An even pod baseline is a 25% seat win rate.",
    group: "Modes",
  },
  "mode-goldfish": {
    title: "Goldfish",
    text: "Your deck against a passive opponent that never acts, blocks, or wins. Measures the deck as it stands: kill-turn distribution, consistency, and mulligans, with zero interaction. Accepts any deck size. Named for practicing against a goldfish, which famously does not block.",
    group: "Modes",
  },
  "games-cap": {
    title: "Games per matchup",
    text: "A ceiling, not a quota. With early stopping on, a matchup ends as soon as the result is statistically decided; without it, every game is played.",
    group: "Setup",
  },
  "early-stop": {
    title: "Early stopping",
    text: "After a 200-game floor, a matchup stops when the 95% confidence interval no longer includes 50%: the verdict cannot change, only sharpen. Saves the budget for close matchups.",
    group: "Setup",
  },
  "auto-precision": {
    title: "Auto (precision mode)",
    text: "Ignores fixed game counts and keeps playing until the confidence interval half-width shrinks to the target, up to a million-game ceiling. The matchup's own variance decides the sample size.",
    group: "Setup",
  },
  seed: {
    title: "Seed",
    text: "The master random seed. Every shuffle and decision derives from it deterministically: the same seed with the same decks reproduces identical results, game for game.",
    group: "Setup",
  },
  window: {
    title: "Window (days)",
    text: "How far back the tournament data reaches. Shorter windows track the current meta faster; longer windows smooth out weekend noise.",
    group: "Setup",
  },
  archetypes: {
    title: "Archetypes",
    text: "How many of the most-played archetypes make the gauntlet, ranked by tournament frequency in the window.",
    group: "Setup",
  },
  "per-hand": {
    title: "Continuations per hand",
    text: "In a sweep, how many games each distinct opening hand plays. More continuations shrink per-hand noise; the hand probabilities themselves are exact.",
    group: "Setup",
  },
  "win-rate": {
    title: "Win rate",
    text: "Wins plus half of draws, over finished games. Both pilots are the same greedy agent, so this measures decks, not players.",
    group: "Results",
  },
  ci: {
    title: "95% confidence interval",
    text: "The Wilson score interval: the range the true win rate sits in with 95% confidence given the sample. It narrows as games accumulate. If it excludes 50%, the matchup verdict is settled.",
    group: "Results",
  },
  share: {
    title: "Meta share",
    text: "The fraction of classified tournament decks in the window that are this archetype. Also this matchup's weight in the field win rate.",
    group: "Results",
  },
  weighted: {
    title: "Weighted field win rate",
    text: "The expected win rate against the field: each matchup's win rate weighted by its meta share. The single number that answers how the deck does out there.",
    group: "Results",
  },
  "on-play": {
    title: "On the play",
    text: "Win rate in games where you went first (skipping your first draw). The gap between play and draw shows how tempo-sensitive the deck is.",
    group: "Results",
  },
  "on-draw": {
    title: "On the draw",
    text: "Win rate in games where the opponent went first and you drew on turn one.",
    group: "Results",
  },
  "opp-cov": {
    title: "Opponent coverage",
    text: "How much of the opponent's list the rules compiler faithfully models (Full plus Partial). Low coverage inflates your win rate: a card the engine cannot play is a dead slot for them.",
    group: "Results",
  },
  pilot: {
    title: "Pilot fidelity",
    text: "The built-in pilot plays a solid curve-out game but cannot execute combo chains or control finesse. Archetypes flagged here lose more than they should, so treat those win rates as optimistic for you.",
    group: "Results",
  },
  "dealt-prob": {
    title: "Dealt probability",
    text: "The exact chance of being dealt this opening hand, from the hypergeometric distribution over your list.",
    group: "Results",
  },
  "kill-turn": {
    title: "Kill turn",
    text: "The turn of yours on which the passive opponent died. Goldfish kill turns measure raw speed with zero interaction, so real games land a turn or two later.",
    group: "Results",
  },
  "turn-hist": {
    title: "Game length",
    text: "Distribution of total turns per game (both players' turns counted). Divide by two for rounds.",
    group: "Results",
  },
  "end-reasons": {
    title: "End reasons",
    text: "How games ended: life reaching zero, poison, decking (drawing from an empty library), or 21 commander damage. Reveals whether the deck wins by racing or grinding.",
    group: "Results",
  },
  mulligan: {
    title: "Mulligans (London)",
    text: "Each mulligan redraws seven, then bottoms one card per mulligan taken. The agent keeps hands with two to five lands and a castable early spell.",
    group: "Results",
  },
  panics: {
    title: "Panics",
    text: "Games where a card interaction crashed the engine. They are isolated, excluded from win rates, and counted here so reliability is visible. Zero is the expectation.",
    group: "Results",
  },
  coverage: {
    title: "Coverage",
    text: "How much of a list the rules compiler faithfully models. Every card is accepted, but not every card is fully modeled; coverage is the honesty metric that says how much to trust a result.",
    group: "Coverage",
  },
  "tier-full": {
    title: "Full",
    text: "Faithfully modeled: the card does in the engine what it does on paper.",
    group: "Coverage",
  },
  "tier-partial": {
    title: "Partial",
    text: "The main effect is modeled; listed rider clauses are dropped and disclosed on the card row.",
    group: "Coverage",
  },
  "tier-proxy": {
    title: "Proxy",
    text: "Correct body, cost, and keywords; the rules text is treated as flavor. A 2/2 that says something clever is just a 2/2 here.",
    group: "Coverage",
  },
  "tier-unplayable": {
    title: "Unplayable",
    text: "The engine cannot model casting it, so it never gets cast. It still occupies a deck slot, like a very committed art card.",
    group: "Coverage",
  },
  playable: {
    title: "Playable coverage",
    text: "Full plus Partial as a fraction of the list: the share of the deck that actually functions in simulation.",
    group: "Coverage",
  },
  curve: {
    title: "Mana curve",
    text: "Nonland cards by mana value. The shape drives how reliably the deck spends its mana each turn.",
    group: "Results",
  },
  "avg-mv": {
    title: "Average mana value",
    text: "Mean mana value of nonland cards. Lower is faster; higher demands more lands and more patience.",
    group: "Results",
  },
  lands: {
    title: "Lands",
    text: "Total land count. Alongside the curve, the main knob for consistency.",
    group: "Results",
  },
  "format-fit": {
    title: "Format fit",
    text: "Per format: the fraction of the list that is legal there, plus whether the deck meets size rules (60 minimum for constructed, 100 with a commander for Commander; no maximum anywhere).",
    group: "Results",
  },
  recommended: {
    title: "Recommended format",
    text: "The most restrictive format where the whole list is legal and the size rules pass. A Standard-legal deck is legal everywhere, so Standard is its truest home.",
    group: "Results",
  },
};
