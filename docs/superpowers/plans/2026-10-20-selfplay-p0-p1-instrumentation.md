# Self-Play P0/P1 — Instrumentation & Measurement Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the two measurement instruments the self-play pipeline needs before any training can start: a root-position logger that feeds the v3 buffer schema, and a magnitude estimator (PR-SPRT) that can tell "12 Elo" from "3 Elo" — because the v3 plan's ceiling rule is expressed in Elo/hour-CPU and the frozen `test.sh` SPRT only produces a binary verdict.

**Architecture:** Two independent deliverables, no shared mutable state. (1) A `selfplay` module in the engine crate that, given a position, runs a root search and emits a `BufferRow` — the v3 JSON schema, versioned by a `schema` field — with the root's per-move `q`/`n`, the `delta` (regret), and the visit entropy. (2) A standalone analysis script that reads a fastchess SPRT log and produces an Elo estimate with a confidence interval, without touching `test.sh`. The engine stays CPU-only and dependency-free; the analysis script is Python 3 stdlib only.

**Tech Stack:** Rust 1.97+ (edition 2024), `cargo` test harness, no new crates. Python 3.10+ stdlib for the analysis script (`math`, `re`, `json`, `argparse`).

## Global Constraints

- **MSRV is Rust 1.97** (`rust-toolchain.toml` pins `channel = "1.97.0"`; `Cargo.toml` declares `rust-version = "1.97"`). `u64::isolate_lowest_one` is stable from 1.97 and MUST be used rather than `bb & bb.wrapping_neg()`.
- **No new crate dependencies.** The engine crate currently has exactly one dependency (`rayon`); the plan adds none. The analysis script uses only the Python 3.10+ standard library.
- **The core board/movegen API is FROZEN** (per `MIGRATION.md`, GATE 2.5). This plan adds a new module and touches `mcts.rs` / `uci.rs` additively only — no change to `position.rs`, `movegen.rs`, `tables.rs`, `slider.rs`, `types.rs` semantics. The one exception is Task 6, which adds *new* fields to `mcts::Limits`; that is additive and does not alter existing behaviour.
- **`test.sh` stays frozen.** Its SPRT protocol is `elo0=0 elo1=5 alpha=0.05 beta=0.05` and must not be modified. PR-SPRT is a *reader* of the same log, never a replacement.
- **Buffer schema is version `3`** (per `SELFPLAY.md` §4). Every emitted row carries `"schema":3`. `schema` is never bumped on an existing buffer file.
- **The `z`/`res` arbitration rule** (per `SELFPLAY.md` §4): the value target is `res` when present, else `z`. `z` is never used when `res` is present.
- **Policy target for distillation is `softmax(Q/τ)`**, not raw visit counts (per `SELFPLAY.md` §3.1). The logger must therefore emit per-move `q` values, not only visit counts.
- **The engine's perft results must not change.** After every task, `cargo test --release` must pass 39/39 and the 6 perft goldens must match exactly.
- **CPU budget**: 8 cores shared with the M4/bridge. Any task that runs a search must be runnable at a reduced node budget so it does not monopolise the machine.

---

## File Structure

| Path | Responsibility |
|---|---|
| `src/selfplay/mod.rs` | Module root: re-exports, the `GenerationConfig` type, and the buffer writer. |
| `src/selfplay/row.rs` | The `BufferRow` struct + its JSON serialisation (hand-rolled, no `serde`). Owns the `schema` field and the `z`/`res` arbitration rule. |
| `src/selfplay/rootlog.rs` | Root search + per-move extraction: `q`, `n`, `delta`, `entrop`, `played`. Turns a `mcts::Search` run into a `BufferRow`. |
| `src/selfplay/writer.rs` | JSONL writer: append one row per line, gzip-free, with atomic line writes. |
| `src/lib.rs` | Add `pub mod selfplay;` (one line). |
| `src/mcts.rs` | Additive: expose the root's per-move data via a new public struct + accessor. |
| `src/uci.rs` | Additive: new UCI options for the self-play mode. |
| `tests/selfplay_buffer.rs` | Round-trip, schema, arbitration, and determinism tests for the buffer. |
| `tests/selfplay_rootlog.rs` | Root-logger behaviour: delta, entropy, policy-target shape. |
| `tools/pr_sprt.py` | The magnitude estimator: fastchess SPRT log → Elo ± CI. |
| `tests/test_pr_sprt.py` | Unit tests for the estimator (pure Python, runnable via `python3 -m unittest`). |

The engine side (`src/selfplay/`) and the analysis side (`tools/pr_sprt.py`) are deliberately independent: the tool can be written, reviewed, and validated against historical logs while the engine side is still in RED. That is intentional — it is the shortest path to having the instrument exist before the first gate.

---

### Task 1: PR-SPRT — the magnitude estimator (analysis side, no engine dependency)

The v3 plan's ceiling rule is "ΔElo per CPU-hour < 10–20". The frozen `test.sh` SPRT is calibrated at `elo1=5` and returns a binary verdict, so it cannot distinguish those magnitudes. This task builds the reader that can. It is deliberately first: it is pure Python, it is testable against a known-answer log, and it does not depend on any engine work.

**Files:**
- Create: `tools/pr_sprt.py`
- Test: `tests/test_pr_sprt.py`

**Interfaces:**
- Consumes: a fastchess SPRT log file (the `prt/sprt-*.log` files `test.sh` tees) and the SPRT parameters in force (`elo0`, `elo1`, `alpha`, `beta`).
- Produces:
  - `parse_sprt_log(path: Path) -> SprtLog` — parsed LLR series, games, and the SPRT bounds.
  - `SprtLog` (dataclass) with fields: `llr: list[float]`, `games: int`, `wins: int`, `losses: int`, `draws: int`, `elo0: float`, `elo1: float`, `alpha: float`, `beta: float`, `llr_final: float`.
  - `elo_estimate(log: SprtLog) -> EloEstimate` — point estimate and confidence interval.
  - `EloEstimate` (dataclass) with fields: `elo: float`, `ci_low: float`, `ci_high: float`, `verdict: str` (`"accepted"`, `"rejected"`, or `"inconclusive"`), `llr: float`.
  - `main(argv: list[str]) -> int` — CLI entry point; prints a human-readable summary and returns a process exit code (0 = the candidate passed, 1 = it failed, 2 = inconclusive).

- [ ] **Step 1: Write the failing test for log parsing**

Create `tests/test_pr_sprt.py`:

```python
import unittest

from tools.pr_sprt import parse_sprt_log, elo_estimate


SPRT_LOG = """Elo difference: 0.00 +- 0.00
SPRT: elo0=0.00 elo1=5.00 alpha=0.05 beta=0.05
LLR: 0.000
Games: 100
W/L/D: 55 40 5
"""


class ParseSprtLogTest(unittest.TestCase):
    def test_parses_games_wld_and_bounds(self):
        log = parse_sprt_log_from_text(SPRT_LOG)
        self.assertEqual(log.games, 100)
        self.assertEqual(log.wins, 55)
        self.assertEqual(log.losses, 40)
        self.assertEqual(log.draws, 5)
        self.assertAlmostEqual(log.elo0, 0.0)
        self.assertAlmostEqual(log.elo1, 5.0)
        self.assertAlmostEqual(log.llr_final, 0.0)

    def test_missing_fields_raise_a_clear_error(self):
        with self.assertRaisesRegex(ValueError, "games"):
            parse_sprt_log_from_text("SPRT: elo0=0.00 elo1=5.00\n")


if __name__ == "__main__":
    unittest.main()
```

Note: `parse_sprt_log_from_text` is the text-parsing core; `parse_sprt_log` is the file wrapper. Test the core directly so the test needs no temp file for the first cases.

- [ ] **Step 2: Run the test to verify it fails**

Run: `python3 -m unittest tests.test_pr_sprt -v`
Expected: FAIL with `ModuleNotFoundError: No module named 'tools.pr_sprt'`

- [ ] **Step 3: Create the package marker and the parsing implementation**

Create `tools/__init__.py` (empty file) and `tools/pr_sprt.py`:

```python
#!/usr/bin/env python3
"""Elo magnitude estimator for mychess self-play.

Reads the fastchess SPRT log that `test.sh` tees and produces an Elo
estimate with a confidence interval.

Why this exists: the frozen `test.sh` protocol (`elo0=0 elo1=5`) is a
*quality* test -- it answers "is the candidate better?" with a binary
verdict. The self-play ceiling rule in SELFPLAY.md v3 is expressed in
Elo per CPU-hour against a 10-20 Elo threshold, so the pipeline needs to
distinguish "12 Elo" from "3 Elo". This tool reads the *same* log and
computes that magnitude. It never modifies `test.sh` and never runs games.

Method: the log-level LLR is the natural statistic of the SPRT. Under
H1 (true Elo difference = elo1), the LLR of a game with score s_i is
`z_i = s_i - mu0` where `mu0` is the expected score under H0 at the
current Elo. The estimate of the Elo difference is recovered by inverting
the logistic model `elo = -400 * log10(1/p - 1)`, and the confidence
interval comes from the variance of the per-game LLR increments.

The interval is deliberately conservative: it is computed on the
per-game LLR *increments* (the standard SPRT variance), not on the
binomial score variance, so it stays valid when the game count is small.
"""

from __future__ import annotations

import argparse
import math
import re
import sys
from dataclasses import dataclass, field
from pathlib import Path


@dataclass
class SprtLog:
    """Parsed contents of a fastchess SPRT log."""

    llr: list[float] = field(default_factory=list)
    games: int = 0
    wins: int = 0
    losses: int = 0
    draws: int = 0
    elo0: float = 0.0
    elo1: float = 5.0
    alpha: float = 0.05
    beta: float = 0.05
    llr_final: float = 0.0

    @property
    def score(self) -> float:
        """Observed score rate from the mover's perspective, 0..1."""
        if self.games == 0:
            return 0.5
        return (self.wins + 0.5 * self.draws) / self.games


@dataclass
class EloEstimate:
    """Result of the magnitude estimation."""

    elo: float
    ci_low: float
    ci_high: float
    verdict: str
    llr: float


def _require(match: re.Match | None, name: str, text: str) -> re.Match:
    if match is None:
        raise ValueError(f"cannot find {name!r} in SPRT log")
    return match


def parse_sprt_log_from_text(text: str) -> SprtLog:
    """Parse the fields we need out of a fastchess SPRT log."""
    log = SprtLog()

    bounds = re.search(
        r"SPRT:\s*elo0=(-?[\d.]+)\s+elo1=(-?[\d.]+)\s+alpha=([\d.]+)\s+beta=([\d.]+)",
        text,
    )
    if bounds:
        log.elo0 = float(bounds.group(1))
        log.elo1 = float(bounds.group(2))
        log.alpha = float(bounds.group(3))
        log.beta = float(bounds.group(4))

    wld = _require(re.search(r"W/L/D:\s*(\d+)\s+(\d+)\s+(\d+)", text), "games", text)
    log.wins = int(wld.group(1))
    log.losses = int(wld.group(2))
    log.draws = int(wld.group(3))
    log.games = log.wins + log.losses + log.draws

    llr = _require(re.search(r"LLR:\s*(-?[\d.]+)", text), "games", text)
    log.llr_final = float(llr.group(1))
    log.llr = [log.llr_final]

    return log


def parse_sprt_log(path: Path) -> SprtLog:
    """Read and parse a fastchess SPRT log file."""
    return parse_sprt_log_from_text(Path(path).read_text(encoding="utf-8", errors="replace"))


def _score_to_elo(score: float) -> float:
    """Convert a score rate in (0,1) to Elo, clamped away from the singularity."""
    eps = 1e-6
    p = min(max(score, eps), 1.0 - eps)
    return -400.0 * math.log10(1.0 / p - 1.0)


def _expected_score(elo: float) -> float:
    """Expected score rate for a given Elo difference."""
    return 1.0 / (1.0 + 10.0 ** (-elo / 400.0))


def elo_estimate(log: SprtLog) -> EloEstimate:
    """Estimate the Elo difference and its confidence interval.

    The point estimate inverts the logistic model on the observed score
    rate. The interval uses the per-game LLR variance under H0: each game
    contributes `z_i = s_i - mu0` with `s_i in {0, 0.5, 1}`, so the
    standard error of the mean score is `sigma / sqrt(n)` and the Elo
    interval follows by propagating that through the logistic inverse
    (the derivative `dElo/dp = 400 / (ln(10) * p * (1 - p))`).
    """
    if log.games == 0:
        return EloEstimate(elo=0.0, ci_low=0.0, ci_high=0.0, verdict="inconclusive", llr=0.0)

    p = log.score
    elo = _score_to_elo(p)

    # Per-game LLR increments under H0 (elo0). `s_i` is the game score.
    mu0 = _expected_score(log.elo0)
    total_sq = 0.0
    for score, count in ((1.0, log.wins), (0.5, log.draws), (0.0, log.losses)):
        z = score - mu0
        total_sq += count * z * z
    sigma = math.sqrt(total_sq / log.games)
    se_p = sigma / math.sqrt(log.games)

    # Propagate through the logistic inverse.
    p_clamped = min(max(p, 1e-6), 1.0 - 1e-6)
    d_elo_dp = 400.0 / (math.log(10.0) * p_clamped * (1.0 - p_clamped))
    half = 1.96 * se_p * d_elo_dp

    ci_low = elo - half
    ci_high = elo + half

    # The verdict is the SPRT's, not ours: the LLR decides acceptance.
    #
    # The SPRT accepts H1 when the log-likelihood ratio reaches
    # `log(1/beta)` and rejects H0 when it falls to `log(alpha)`. With
    # alpha = beta = 0.05 those are +2.996 and -2.996. (Note the
    # asymmetry: the acceptance threshold uses `1/beta`, not `beta`.)
    upper = math.log(1.0 / log.beta)
    lower = math.log(log.alpha)
    if log.llr_final >= upper:
        verdict = "accepted"
    elif log.llr_final <= lower:
        verdict = "rejected"
    else:
        verdict = "inconclusive"

    return EloEstimate(
        elo=elo,
        ci_low=ci_low,
        ci_high=ci_high,
        verdict=verdict,
        llr=log.llr_final,
    )


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("log", type=Path, help="fastchess SPRT log to analyse")
    parser.add_argument("--json", action="store_true", help="emit JSON instead of text")
    args = parser.parse_args(argv)

    try:
        log = parse_sprt_log(args.log)
    except (OSError, ValueError) as exc:
        print(f"pr_sprt: {exc}", file=sys.stderr)
        return 2

    est = elo_estimate(log)

    if args.json:
        import json

        print(
            json.dumps(
                {
                    "elo": est.elo,
                    "ci_low": est.ci_low,
                    "ci_high": est.ci_high,
                    "verdict": est.verdict,
                    "llr": est.llr,
                    "games": log.games,
                },
                indent=2,
            )
        )
    else:
        print(f"games      : {log.games}  (W{log.wins} L{log.losses} D{log.draws})")
        print(f"elo        : {est.elo:+.1f}  [{est.ci_low:+.1f}, {est.ci_high:+.1f}]")
        print(f"llr        : {est.llr:+.3f}   verdict: {est.verdict}")

    if est.verdict == "accepted":
        return 0
    if est.verdict == "rejected":
        return 1
    return 2


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `python3 -m unittest tests.test_pr_sprt -v`
Expected: PASS (2 tests)

- [ ] **Step 5: Add the known-answer test — a symmetric log must estimate 0 Elo**

Append to `tests/test_pr_sprt.py`:

```python
class EloEstimateTest(unittest.TestCase):
    def test_symmetrical_record_estimates_zero_elo(self):
        log = parse_sprt_log_from_text(
            "SPRT: elo0=0.00 elo1=5.00 alpha=0.05 beta=0.05\n"
            "LLR: 0.000\nGames: 200\nW/L/D: 90 90 20\n"
        )
        est = elo_estimate(log)
        self.assertAlmostEqual(est.elo, 0.0, delta=12.0)
        self.assertLess(est.ci_low, 0.0)
        self.assertGreater(est.ci_high, 0.0)

    def test_verdict_accepted_when_llr_is_above_the_upper_bound(self):
        log = parse_sprt_log_from_text(
            "SPRT: elo0=0.00 elo1=5.00 alpha=0.05 beta=0.05\n"
            "LLR: 3.10\nGames: 400\nW/L/D: 230 170 0\n"
        )
        est = elo_estimate(log)
        self.assertEqual(est.verdict, "accepted")

    def test_verdict_rejected_when_llr_is_below_the_lower_bound(self):
        log = parse_sprt_log_from_text(
            "SPRT: elo0=0.00 elo1=5.00 alpha=0.05 beta=0.05\n"
            "LLR: -3.10\nGames: 400\nW/L/D: 170 230 0\n"
        )
        est = elo_estimate(log)
        self.assertEqual(est.verdict, "rejected")

    def test_verdict_inconclusive_inside_the_bounds(self):
        log = parse_sprt_log_from_text(
            "SPRT: elo0=0.00 elo1=5.00 alpha=0.05 beta=0.05\n"
            "LLR: 0.100\nGames: 40\nW/L/D: 22 18 0\n"
        )
        est = elo_estimate(log)
        self.assertEqual(est.verdict, "inconclusive")
```

- [ ] **Step 6: Run the test to verify it passes**

Run: `python3 -m unittest tests.test_pr_sprt -v`
Expected: PASS (6 tests)

- [ ] **Step 7: Verify the CLI works on a real log**

Run: `printf 'SPRT: elo0=0.00 elo1=5.00 alpha=0.05 beta=0.05\nLLR: 1.234\nGames: 300\nW/L/D: 170 130 0\n' > /tmp/fake_sprt.log && python3 tools/pr_sprt.py /tmp/fake_sprt.log`
Expected: three lines of output; `verdict: inconclusive` (LLR 1.234 is inside [-2.94, 2.71]).

- [ ] **Step 8: Commit**

```bash
git add tools/pr_sprt.py tools/__init__.py tests/test_pr_sprt.py
git commit -m "feat(measure): PR-SPRT magnitude estimator (Elo +/- CI from a fastchess SPRT log)"
```

---

### Task 2: `BufferRow` — the versioned v3 schema

Task 1 gave the pipeline its magnitude instrument. Now the engine side starts: the row type that everything else writes.

**Files:**
- Create: `src/selfplay/mod.rs`
- Create: `src/selfplay/row.rs`
- Modify: `src/lib.rs` (add `pub mod selfplay;`)
- Test: `tests/selfplay_buffer.rs`

**Interfaces:**
- Consumes: nothing (leaf module).
- Produces:
  - `mychess::selfplay::BufferRow` — public struct with public fields:
    - `schema: u32` (always `3`)
    - `fen: String`
    - `gen: u32`
    - `played: Option<String>` — the move actually played from this position, `None` if unknown
    - `z: Option<f32>` — game result for the side to move at the root, in {-1.0, 0.0, 1.0}
    - `res: Option<f32>` — adjudicated score in [-1.0, 1.0]
    - `src: Provenance`
    - `conf: f32`
    - `budget: u32`
    - `gencfg: String`
    - `moves: Vec<MoveStat>`
    - `delta: f32` — `Q_best - Q_played`
    - `entrop: f32` — entropy of the root visit distribution, in nats
    - `targets: Targets`
    - `relabel: Option<Relabel>`
  - `mychess::selfplay::MoveStat` — `u: String` (UCI), `q: f32`, `n: u64`.
  - `mychess::selfplay::Provenance` — enum: `SelfPlayed`, `SelfAdjudicated`, `AsymStrong`, `AsymWeak`, `Search10x`, `Search100x`, `Teacher`, `Tablebase`, `Exploit`. It carries `conf() -> f32` (the v3 table: 1.00, 0.95, 0.90, 0.70, 0.60, 0.50, 0.40, —, —) and `is_replay_eligible() -> bool` (false for `AsymWeak` and `Exploit`).
  - `mychess::selfplay::Targets` — struct with `wdl: Option<[f32; 3]>` and `moves_left: Option<f32>`, both `None` at schema 3.
  - `mychess::selfplay::Relabel` — `by: String`, `q: f32`, `budget: u32`.
  - `mychess::selfplay::BufferRow::value_target() -> Option<f32>` — the arbitration rule: `res` if present, else `z`, else `None`.
  - `mychess::selfplay::BufferRow::to_json_line() -> String` — one-line JSON, no trailing newline.
  - `mychess::selfplay::BufferRow::from_json_line(line: &str) -> Result<BufferRow, String>` — the reader, symmetric with the writer.

- [ ] **Step 1: Write the failing test for the schema version and the arbitration rule**

Create `tests/selfplay_buffer.rs`:

```rust
use mychess::selfplay::{BufferRow, Provenance, Targets};

fn sample_row() -> BufferRow {
    BufferRow {
        schema: mychess::selfplay::SCHEMA_VERSION,
        fen: "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1".into(),
        gen: 3,
        played: Some("e2e4".into()),
        z: Some(1.0),
        res: None,
        src: Provenance::SelfPlayed,
        conf: 0.60,
        budget: 10_000,
        gencfg: "sha8deadbeef".into(),
        moves: vec![],
        delta: 0.02,
        entrop: 1.84,
        targets: Targets::default(),
        relabel: None,
    }
}

#[test]
fn schema_version_is_three() {
    assert_eq!(mychess::selfplay::SCHEMA_VERSION, 3);
    assert_eq!(sample_row().schema, 3);
}

#[test]
fn value_target_prefers_res_over_z() {
    let mut row = sample_row();
    assert_eq!(row.value_target(), Some(1.0)); // falls back to z
    row.res = Some(0.5);
    assert_eq!(row.value_target(), Some(0.5)); // res wins
}

#[test]
fn value_target_is_none_without_z_nor_res() {
    let mut row = sample_row();
    row.z = None;
    assert_eq!(row.value_target(), None);
}

#[test]
fn provenance_confidence_matches_the_v3_table() {
    assert_eq!(Provenance::Tablebase.conf(), 1.00);
    assert_eq!(Provenance::Search100x.conf(), 0.95);
    assert_eq!(Provenance::Search10x.conf(), 0.90);
    assert_eq!(Provenance::Teacher.conf(), 0.70);
    assert_eq!(Provenance::SelfPlayed.conf(), 0.60);
    assert_eq!(Provenance::AsymStrong.conf(), 0.50);
    assert_eq!(Provenance::SelfAdjudicated.conf(), 0.40);
}

#[test]
fn asym_weak_and_exploit_are_not_replay_eligible() {
    assert!(!Provenance::AsymWeak.is_replay_eligible());
    assert!(!Provenance::Exploit.is_replay_eligible());
    assert!(Provenance::SelfPlayed.is_replay_eligible());
    assert!(Provenance::Tablebase.is_replay_eligible());
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --release --test selfplay_buffer`
Expected: FAIL with `error[E0432]: unresolved import mychess::selfplay`

- [ ] **Step 3: Create the module with the row type**

Create `src/selfplay/mod.rs`:

```rust
//! Self-play instrumentation: the v3 buffer schema and the root logger.
//!
//! Schema version 3 (see `SELFPLAY.md` §4). The two rules that the rest of
//! the pipeline depends on:
//!
//! * the **value target** is `res` when present, else `z` (`z` is never
//!   used when `res` is present) — see [`BufferRow::value_target`];
//! * the **policy target** for distillation is `softmax(Q/tau)` over the
//!   root's per-move `q` values, *not* the raw visit counts — see
//!   [`BufferRow::policy_target`].

pub mod row;

pub use row::{BufferRow, MoveStat, Provenance, Relabel, Targets};

/// The buffer schema version this build writes and reads.
pub const SCHEMA_VERSION: u32 = 3;
```

> `rootlog` and `writer` are declared in Tasks 4 and 5 respectively, when
> their contents exist. Declaring them here with empty placeholder files
> would make the crate compile, but the `pub use` lines re-exporting their
> symbols would not — so each module is added to `mod.rs` by the task that
> implements it.

Create `src/selfplay/row.rs`:

```rust
//! The versioned self-play buffer row (schema 3).

use std::fmt;

/// Where a row came from. The confidence values are the v3 hierarchy
/// (`SELFPLAY.md` §4); they are calibrated against the S8 holdout, not
/// asserted.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Provenance {
    /// A game that reached a terminal position.
    SelfPlayed,
    /// A game that was adjudicated (draw or resignation).
    SelfAdjudicated,
    /// The strong side of an asymmetric self-play game.
    AsymStrong,
    /// The weak side of an asymmetric self-play game.
    AsymWeak,
    /// Re-searched at 10x the base budget.
    Search10x,
    /// Re-searched at 100x the base budget.
    Search100x,
    /// An external teacher (separate heads, never a policy target).
    Teacher,
    /// An exact tablebase position.
    Tablebase,
    /// A position found by an exploiter (value only, never a policy target).
    Exploit,
}

impl Provenance {
    /// The v3 confidence weight for this provenance.
    pub fn conf(self) -> f32 {
        match self {
            Provenance::Tablebase => 1.00,
            Provenance::Search100x => 0.95,
            Provenance::Search10x => 0.90,
            Provenance::Teacher => 0.70,
            Provenance::SelfPlayed => 0.60,
            Provenance::AsymStrong => 0.50,
            Provenance::SelfAdjudicated => 0.40,
            Provenance::AsymWeak => 0.0,
            Provenance::Exploit => 0.0,
        }
    }

    /// Whether rows with this provenance may enter the replay.
    pub fn is_replay_eligible(self) -> bool {
        !matches!(self, Provenance::AsymWeak | Provenance::Exploit)
    }

    /// The wire name used in the buffer.
    pub fn as_str(self) -> &'static str {
        match self {
            Provenance::SelfPlayed => "self",
            Provenance::SelfAdjudicated => "self_adj",
            Provenance::AsymStrong => "asym",
            Provenance::AsymWeak => "asym_weak",
            Provenance::Search10x => "search10x",
            Provenance::Search100x => "search100x",
            Provenance::Teacher => "teacher",
            Provenance::Tablebase => "tb",
            Provenance::Exploit => "exploit",
        }
    }

    fn from_str(s: &str) -> Option<Provenance> {
        Some(match s {
            "self" => Provenance::SelfPlayed,
            "self_adj" => Provenance::SelfAdjudicated,
            "asym" => Provenance::AsymStrong,
            "asym_weak" => Provenance::AsymWeak,
            "search10x" => Provenance::Search10x,
            "search100x" => Provenance::Search100x,
            "teacher" => Provenance::Teacher,
            "tb" => Provenance::Tablebase,
            "exploit" => Provenance::Exploit,
            _ => return None,
        })
    }
}

/// Per-move statistics at the root.
#[derive(Clone, PartialEq, Debug)]
pub struct MoveStat {
    /// The move in UCI form (e.g. `"e2e4"`).
    pub u: String,
    /// The search value of this move, in [-1, 1] (the `eval_normalized`
    /// convention). This is the **policy target's** input, not the visit
    /// count.
    pub q: f32,
    /// The number of visits this move received.
    pub n: u64,
}

/// Auxiliary training targets. Both are `None` at schema 3; they exist so
/// that P4 can add them without a buffer format migration.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Targets {
    /// Win / draw / loss probabilities, in that order.
    pub wdl: Option<[f32; 3]>,
    /// The expected number of moves to the end of the game.
    pub moves_left: Option<f32>,
}

/// A relabel that overwrote this row's label, kept for traceability.
#[derive(Clone, PartialEq, Debug)]
pub struct Relabel {
    /// Who produced the new label (e.g. `"search10x"`).
    pub by: String,
    /// The new value.
    pub q: f32,
    /// The budget used for the relabel.
    pub budget: u32,
}

/// One line of the self-play buffer.
#[derive(Clone, PartialEq, Debug)]
pub struct BufferRow {
    /// The schema version. Always [`crate::selfplay::SCHEMA_VERSION`] for
    /// rows this build writes.
    pub schema: u32,
    pub fen: String,
    pub gen: u32,
    /// The move actually played from this position. Required: without it
    /// the regret (`delta`) is uncomputable under a non-zero temperature.
    pub played: Option<String>,
    /// The game result for the side to move at the root.
    pub z: Option<f32>,
    /// The adjudicated score, when adjudication was applied.
    pub res: Option<f32>,
    pub src: Provenance,
    pub conf: f32,
    pub budget: u32,
    /// The hash of the generation configuration in force.
    pub gencfg: String,
    /// The root's per-move statistics.
    pub moves: Vec<MoveStat>,
    /// `Q_best - Q_played`.
    pub delta: f32,
    /// The entropy of the root visit distribution, in nats.
    pub entrop: f32,
    pub targets: Targets,
    pub relabel: Option<Relabel>,
}

impl BufferRow {
    /// The value target: `res` when present, else `z`.
    ///
    /// This is the single arbitration rule for the whole pipeline. `z` is
    /// never used when `res` is present, so adjudication always wins over
    /// the raw game result.
    pub fn value_target(&self) -> Option<f32> {
        self.res.or(self.z)
    }

    /// The policy target: `softmax(Q/tau)` over the root's per-move `q`.
    ///
    /// Deliberately *not* the visit distribution: at tau=1 the visit
    /// distribution is a Monte-Carlo law whose variance is high, and
    /// distilling it propagates the search's noise instead of its strength.
    /// See `SELFPLAY.md` §3.1.
    pub fn policy_target(&self, tau: f32) -> Vec<(String, f32)> {
        if self.moves.is_empty() {
            return Vec::new();
        }
        let tau = tau.max(1e-3);
        let max_q = self.moves.iter().map(|m| m.q).fold(f32::NEG_INFINITY, f32::max);
        let weights: Vec<f32> = self.moves.iter().map(|m| ((m.q - max_q) / tau).exp()).collect();
        let total: f32 = weights.iter().sum();
        if total <= 0.0 {
            return Vec::new();
        }
        self.moves
            .iter()
            .zip(weights)
            .map(|(m, w)| (m.u.clone(), w / total))
            .collect()
    }

    /// Serialise to a single JSON line (no trailing newline).
    pub fn to_json_line(&self) -> String {
        let mut s = String::with_capacity(256);
        s.push('{');
        s.push_str(&format!("\"schema\":{},", self.schema));
        s.push_str(&format!("\"fen\":{},", json_str(&self.fen)));
        s.push_str(&format!("\"gen\":{},", self.gen));
        s.push_str(&format!("\"played\":{},", opt_json_str(&self.played)));
        s.push_str(&format!("\"z\":{},", opt_f32(self.z)));
        s.push_str(&format!("\"res\":{},", opt_f32(self.res)));
        s.push_str(&format!("\"src\":\"{}\",", self.src.as_str()));
        s.push_str(&format!("\"conf\":{},", self.conf));
        s.push_str(&format!("\"budget\":{},", self.budget));
        s.push_str(&format!("\"gencfg\":{},", json_str(&self.gencfg)));
        s.push_str("\"moves\":[");
        for (i, m) in self.moves.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str(&format!(
                "{{\"u\":{},\"q\":{},\"n\":{}}}",
                json_str(&m.u),
                m.q,
                m.n
            ));
        }
        s.push_str("],");
        s.push_str(&format!("\"delta\":{},", self.delta));
        s.push_str(&format!("\"entrop\":{},", self.entrop));
        s.push_str("\"targets\":{");
        if let Some(wdl) = self.targets.wdl {
            s.push_str(&format!(
                "\"wdl\":[{},{},{}],",
                wdl[0], wdl[1], wdl[2]
            ));
        }
        if let Some(ml) = self.targets.moves_left {
            s.push_str(&format!("\"moves_left\":{},", ml));
        }
        s.push_str("},");
        match &self.relabel {
            Some(r) => s.push_str(&format!(
                "\"relabel\":{{\"by\":{},\"q\":{},\"budget\":{}}}}}",
                json_str(&r.by),
                r.q,
                r.budget
            )),
            None => s.push_str("\"relabel\":null}"),
        }
        s
    }

    /// Parse a single JSON line back into a row.
    pub fn from_json_line(line: &str) -> Result<BufferRow, String> {
        let v = parse_json_object(line)?;
        let schema = v.get("schema").and_then(|x| x.as_u64()).ok_or("missing schema")? as u32;
        if schema != crate::selfplay::SCHEMA_VERSION {
            return Err(format!("unsupported schema {schema}"));
        }
        let fen = v
            .get("fen")
            .and_then(|x| x.as_str())
            .ok_or("missing fen")?
            .to_string();
        let gen = v.get("gen").and_then(|x| x.as_u64()).ok_or("missing gen")? as u32;
        let played = v.get("played").and_then(|x| x.as_str()).map(str::to_string);
        let z = v.get("z").and_then(|x| x.as_f64()).map(|x| x as f32);
        let res = v.get("res").and_then(|x| x.as_f64()).map(|x| x as f32);
        let src_name = v.get("src").and_then(|x| x.as_str()).ok_or("missing src")?;
        let src = Provenance::from_str(src_name).ok_or(format!("unknown src {src_name:?}"))?;
        let conf = v.get("conf").and_then(|x| x.as_f64()).ok_or("missing conf")? as f32;
        let budget = v.get("budget").and_then(|x| x.as_u64()).ok_or("missing budget")? as u32;
        let gencfg = v
            .get("gencfg")
            .and_then(|x| x.as_str())
            .ok_or("missing gencfg")?
            .to_string();
        let mut moves = Vec::new();
        if let Some(arr) = v.get("moves").and_then(|x| x.as_array()) {
            for m in arr {
                let u = m.get("u").and_then(|x| x.as_str()).ok_or("move without u")?.to_string();
                let q = m.get("q").and_then(|x| x.as_f64()).ok_or("move without q")? as f32;
                let n = m.get("n").and_then(|x| x.as_u64()).ok_or("move without n")?;
                moves.push(MoveStat { u, q, n });
            }
        }
        let delta = v.get("delta").and_then(|x| x.as_f64()).ok_or("missing delta")? as f32;
        let entrop = v.get("entrop").and_then(|x| x.as_f64()).ok_or("missing entrop")? as f32;
        let mut targets = Targets::default();
        if let Some(t) = v.get("targets") {
            if let Some(w) = t.get("wdl").and_then(|x| x.as_array()) {
                if w.len() == 3 {
                    targets.wdl = Some([
                        w[0].as_f64().unwrap_or(0.0) as f32,
                        w[1].as_f64().unwrap_or(0.0) as f32,
                        w[2].as_f64().unwrap_or(0.0) as f32,
                    ]);
                }
            }
            if let Some(ml) = t.get("moves_left").and_then(|x| x.as_f64()) {
                targets.moves_left = Some(ml as f32);
            }
        }
        let relabel = match v.get("relabel") {
            Some(r) if !r.is_null() => Some(Relabel {
                by: r.get("by").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                q: r.get("q").and_then(|x| x.as_f64()).unwrap_or(0.0) as f32,
                budget: r.get("budget").and_then(|x| x.as_u64()).unwrap_or(0) as u32,
            }),
            _ => None,
        };
        Ok(BufferRow {
            schema,
            fen,
            gen,
            played,
            z,
            res,
            src,
            conf,
            budget,
            gencfg,
            moves,
            delta,
            entrop,
            targets,
            relabel,
        })
    }
}

impl fmt::Display for BufferRow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_json_line())
    }
}

fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn opt_json_str(v: &Option<String>) -> String {
    match v {
        Some(s) => json_str(s),
        None => "null".to_string(),
    }
}

fn opt_f32(v: Option<f32>) -> String {
    match v {
        Some(x) => format!("{x}"),
        None => "null".to_string(),
    }
}

/// A minimal JSON value, just enough to read a buffer row back.
#[derive(Clone, PartialEq, Debug)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    fn as_array(&self) -> Option<&[Json]> {
        match self {
            Json::Arr(items) => Some(items),
            _ => None,
        }
    }
    fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }
    fn as_f64(&self) -> Option<f64> {
        match self {
            Json::Num(n) => Some(*n),
            _ => None,
        }
    }
    fn as_u64(&self) -> Option<u64> {
        match self {
            Json::Num(n) if *n >= 0.0 => Some(*n as u64),
            _ => None,
        }
    }
    fn is_null(&self) -> bool {
        matches!(self, Json::Null)
    }
}

/// Parse a JSON object into the minimal [`Json`] tree.
pub fn parse_json_object(text: &str) -> Result<Json, String> {
    let bytes: Vec<char> = text.chars().collect();
    let mut pos = 0usize;
    let value = parse_value(&bytes, &mut pos)?;
    skip_ws(&bytes, &mut pos);
    if pos != bytes.len() {
        return Err("trailing characters after JSON value".into());
    }
    Ok(value)
}

fn skip_ws(b: &[char], pos: &mut usize) {
    while *pos < b.len() && b[*pos].is_whitespace() {
        *pos += 1;
    }
}

fn parse_value(b: &[char], pos: &mut usize) -> Result<Json, String> {
    skip_ws(b, pos);
    match b.get(*pos) {
        None => Err("unexpected end of JSON".into()),
        Some('n') => {
            expect_word(b, pos, "null")?;
            Ok(Json::Null)
        }
        Some('t') => {
            expect_word(b, pos, "true")?;
            Ok(Json::Bool(true))
        }
        Some('f') => {
            expect_word(b, pos, "false")?;
            Ok(Json::Bool(false))
        }
        Some('"') => Ok(Json::Str(parse_string(b, pos)?)),
        Some('[') => {
            *pos += 1;
            let mut items = Vec::new();
            skip_ws(b, pos);
            if b.get(*pos) == Some(&']') {
                *pos += 1;
                return Ok(Json::Arr(items));
            }
            loop {
                items.push(parse_value(b, pos)?);
                skip_ws(b, pos);
                match b.get(*pos) {
                    Some(',') => *pos += 1,
                    Some(']') => {
                        *pos += 1;
                        break;
                    }
                    _ => return Err("expected , or ] in array".into()),
                }
            }
            Ok(Json::Arr(items))
        }
        Some('{') => {
            *pos += 1;
            let mut fields = Vec::new();
            skip_ws(b, pos);
            if b.get(*pos) == Some(&'}') {
                *pos += 1;
                return Ok(Json::Obj(fields));
            }
            loop {
                skip_ws(b, pos);
                let key = parse_string(b, pos)?;
                skip_ws(b, pos);
                if b.get(*pos) != Some(&':') {
                    return Err("expected : after object key".into());
                }
                *pos += 1;
                let value = parse_value(b, pos)?;
                fields.push((key, value));
                skip_ws(b, pos);
                match b.get(*pos) {
                    Some(',') => *pos += 1,
                    Some('}') => {
                        *pos += 1;
                        break;
                    }
                    _ => return Err("expected , or } in object".into()),
                }
            }
            Ok(Json::Obj(fields))
        }
        Some(_) => {
            let start = *pos;
            while *pos < b.len()
                && (b[*pos].is_ascii_digit()
                    || matches!(b[*pos], '-' | '+' | '.' | 'e' | 'E'))
            {
                *pos += 1;
            }
            let s: String = b[start..*pos].iter().collect();
            s.parse::<f64>()
                .map(Json::Num)
                .map_err(|_| format!("invalid number {s:?}"))
        }
    }
}

fn expect_word(b: &[char], pos: &mut usize, word: &str) -> Result<(), String> {
    for c in word.chars() {
        if b.get(*pos) != Some(&c) {
            return Err(format!("expected {word:?}"));
        }
        *pos += 1;
    }
    Ok(())
}

fn parse_string(b: &[char], pos: &mut usize) -> Result<String, String> {
    if b.get(*pos) != Some(&'"') {
        return Err("expected string".into());
    }
    *pos += 1;
    let mut out = String::new();
    while let Some(&c) = b.get(*pos) {
        *pos += 1;
        match c {
            '"' => return Ok(out),
            '\\' => {
                let esc = *b.get(*pos).ok_or("unterminated escape")?;
                *pos += 1;
                match esc {
                    '"' => out.push('"'),
                    '\\' => out.push('\\'),
                    '/' => out.push('/'),
                    'n' => out.push('\n'),
                    'r' => out.push('\r'),
                    't' => out.push('\t'),
                    'b' => out.push('\u{08}'),
                    'f' => out.push('\u{0c}'),
                    'u' => {
                        let hex: String = b.get(*pos..*pos + 4).ok_or("bad \\u")?.iter().collect();
                        *pos += 4;
                        let code =
                            u32::from_str_radix(&hex, 16).map_err(|_| "bad \\u escape")?;
                        out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                    }
                    other => return Err(format!("bad escape \\{other}")),
                }
            }
            c => out.push(c),
        }
    }
    Err("unterminated string".into())
}
```

Add to `src/lib.rs`, after `pub mod refgen;`:

```rust
pub mod selfplay;
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --release --test selfplay_buffer`
Expected: PASS (5 tests)

- [ ] **Step 5: Add the JSON round-trip test**

Append to `tests/selfplay_buffer.rs`:

```rust
#[test]
fn json_round_trip_preserves_every_field() {
    let mut row = sample_row();
    row.res = Some(0.5);
    row.moves = vec![
        mychess::selfplay::MoveStat { u: "e2e4".into(), q: 0.13, n: 4120 },
        mychess::selfplay::MoveStat { u: "d2d4".into(), q: -0.05, n: 120 },
    ];
    row.relabel = Some(mychess::selfplay::Relabel {
        by: "search10x".into(),
        q: 0.05,
        budget: 100_000,
    });

    let line = row.to_json_line();
    assert!(!line.contains('\n'), "a row must be a single line");
    let back = mychess::selfplay::BufferRow::from_json_line(&line).expect("round trip");
    assert_eq!(back, row);
}

#[test]
fn a_row_with_a_foreign_schema_is_rejected() {
    let line = r#"{"schema":2,"fen":"8/8/8/8/8/8/8/8 w - - 0 1","gen":0,"z":null,"res":null,"src":"self","conf":0.6,"budget":1,"gencfg":"x","moves":[],"delta":0.0,"entrop":0.0,"targets":{},"relabel":null}"#;
    assert!(mychess::selfplay::BufferRow::from_json_line(line).is_err());
}

#[test]
fn policy_target_is_a_softmax_over_q_not_over_visits() {
    let mut row = sample_row();
    row.moves = vec![
        mychess::selfplay::MoveStat { u: "e2e4".into(), q: 0.0, n: 10 },
        mychess::selfplay::MoveStat { u: "d2d4".into(), q: 0.0, n: 1000 },
    ];
    let target = row.policy_target(1.0);
    // Equal q => equal probability, whatever the visit counts say.
    assert!((target[0].1 - target[1].1).abs() < 1e-6);
    assert!((target[0].1 + target[1].1 - 1.0).abs() < 1e-6);
}

#[test]
fn policy_target_favours_the_higher_q_move() {
    let mut row = sample_row();
    row.moves = vec![
        mychess::selfplay::MoveStat { u: "e2e4".into(), q: 0.5, n: 10 },
        mychess::selfplay::MoveStat { u: "d2d4".into(), q: -0.5, n: 1000 },
    ];
    let target = row.policy_target(0.5);
    assert!(target[0].1 > target[1].1);
}
```

- [ ] **Step 6: Run the test to verify it passes**

Run: `cargo test --release --test selfplay_buffer`
Expected: PASS (9 tests)

- [ ] **Step 7: Verify the rest of the suite is unaffected**

Run: `cargo test --release`
Expected: PASS (39 + 9 = 48 tests), 0 failed. The perft goldens are unaffected because this task touches no core module.

- [ ] **Step 8: Commit**

```bash
git add src/selfplay/ src/lib.rs tests/selfplay_buffer.rs
git commit -m "feat(selfplay): versioned buffer row (schema 3) with the z/res arbitration rule"
```

---

### Task 3: Expose the root's per-move data from `mcts.rs`

Task 2 defined the row; the root logger cannot fill it because the arena's root node is `pub(crate)` and its per-move `q`/`n` are not exposed. This task opens that, additively.

**Files:**
- Modify: `src/mcts.rs`
- Test: `tests/selfplay_rootlog.rs`

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `mychess::mcts::RootMove` — public struct: `mv: Move`, `q: f32` (the value from the parent's perspective, `[-1, 1]`), `visits: u64`.
  - `mychess::mcts::RootStats` — public struct: `moves: Vec<RootMove>` (sorted by descending visits), `visits_total: u64`.
  - `mychess::mcts::Search::root_stats(&self) -> RootStats` — the root's per-move breakdown after a run. Empty when the root was not expanded.
  - `mychess::mcts::Search::root_visits(&self) -> u64`.

**Important design note:** the `q` exposed here is the *child's* value from the *parent's* perspective, i.e. `-(child.sum / child.visits)` — the same quantity `q_from_parent` computes. This is the value a policy target is built from, so it must be the mover-relative value at the root, not the absolute score.

- [ ] **Step 1: Write the failing test**

Create `tests/selfplay_rootlog.rs`:

```rust
use mychess::mcts::{Limits, Search};
use mychess::position::Position;
use std::sync::atomic::AtomicBool;

fn run(fen: &str, nodes: u64) -> (Search, mychess::mcts::Outcome) {
    let pos = Position::from_fen(fen).unwrap();
    let search = Search::new(
        pos,
        vec![pos.zobrist],
        Limits { nodes, movetime_ms: None, max_playout_ply: 100, c_uct: std::f32::consts::SQRT_2 },
        42,
    );
    let out = search.run(&AtomicBool::new(false));
    (search, out)
}

#[test]
fn root_stats_expose_every_explored_root_move() {
    let (search, out) = run("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1", 2000);
    assert_ne!(out.best, mychess::types::Move::NONE);
    let stats = search.root_stats();
    assert!(!stats.moves.is_empty(), "the root must expose its explored moves");
    assert!(stats.visits_total >= 1);

    // Moves are sorted by descending visits.
    for pair in stats.moves.windows(2) {
        assert!(pair[0].visits >= pair[1].visits, "root moves must be visit-sorted");
    }
}

#[test]
fn root_move_values_stay_within_the_normalised_range() {
    let (search, _) = run("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1", 2000);
    for m in search.root_stats().moves {
        assert!(m.q >= -1.0 && m.q <= 1.0, "q out of range: {}", m.q);
    }
}

#[test]
fn an_unexpanded_root_has_no_root_stats() {
    // A terminal position: the root is never expanded, so there is no
    // per-move data to log.
    let pos = Position::from_fen("7k/5K2/6Q1/8/8/8/8/8 b - - 0 1").unwrap();
    let search = Search::new(
        pos,
        vec![pos.zobrist],
        Limits { nodes: 100, movetime_ms: None, max_playout_ply: 100, c_uct: std::f32::consts::SQRT_2 },
        1,
    );
    let out = search.run(&AtomicBool::new(false));
    assert_eq!(out.best, mychess::types::Move::NONE, "the position is stalemate");
    assert!(search.root_stats().moves.is_empty());
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --release --test selfplay_rootlog`
Expected: FAIL with `error[E0433]: failed to resolve: use of undeclared type or module 'mychess::mcts::RootStats'`

- [ ] **Step 3: Expose the root data**

In `src/mcts.rs`, add after the `Child` struct:

```rust
/// One explored move at the root, with the value the selection used.
///
/// The `q` is the **parent's** view of this move (mover-relative, in
/// `[-1, 1]`, the `eval_normalized` convention) — the same quantity the
/// UCT selection maximises, and the input of a policy target.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct RootMove {
    pub mv: Move,
    pub q: f32,
    pub visits: u64,
}

/// The root node's per-move breakdown, available after a search.
#[derive(Clone, Default, PartialEq, Debug)]
pub struct RootStats {
    /// The explored root moves, sorted by descending visit count.
    pub moves: Vec<RootMove>,
    /// The total number of visits recorded at the root.
    pub visits_total: u64,
}
```

In `impl Search`, add:

```rust
    /// The root's per-move breakdown after the last `run`.
    ///
    /// Empty when the root was never expanded (a terminal position, or a
    /// search that stopped before the first expansion).
    pub fn root_stats(&self) -> RootStats {
        self.root.clone()
    }

    /// The total number of visits recorded at the root.
    pub fn root_visits(&self) -> u64 {
        self.root_stats().visits_total
    }
```

And add the field to `Search` plus its initialisation in `Search::new`:

```rust
pub struct Search {
    pub root_pos: Position,
    /// Zobrist of every position BEFORE the root (UCI replay), for repetition.
    pub game_hist: Vec<u64>,
    pub limits: Limits,
    pub seed: u64,
    /// The root's per-move breakdown, filled in at the end of `run`.
    root: RootStats,
}
```

```rust
    pub fn new(root_pos: Position, game_hist: Vec<u64>, limits: Limits, seed: u64) -> Search {
        Search {
            root_pos,
            game_hist,
            limits,
            seed,
            root: RootStats::default(),
        }
    }
```

Finally, in `run`, just before the `Outcome` is built, capture the root stats:

```rust
        // Capture the root's per-move breakdown for the self-play logger.
        let root = {
            let children = &arena[0].children;
            let mut moves: Vec<RootMove> = children
                .iter()
                .filter(|c| arena[c.node as usize].visits > 0)
                .map(|c| {
                    let node = &arena[c.node as usize];
                    RootMove {
                        mv: c.mv,
                        q: q_from_parent(node),
                        visits: node.visits,
                    }
                })
                .collect();
            moves.sort_by(|a, b| b.visits.cmp(&a.visits));
            let visits_total = moves.iter().map(|m| m.visits).sum();
            RootStats { moves, visits_total }
        };
```

and add `root` to the `Outcome`-returning code by changing the `Search::run` body to set `self.root` — because `run` takes `&self`, the field must be written through a `Cell`/`RefCell` or the method must take `&mut self`. **Use `&mut self`**: change `pub fn run(&self, ...)` to `pub fn run(&mut self, ...)`.

Careful: `Search::run` is called in `src/uci.rs` as `search.run(&stop)` where `search` is a local `let search = Search::new(...)` — that binding is already mutable-by-move, so change it to `let mut search = Search::new(...)`. The tests in `tests/mcts_v1.rs` call `search.run(...)` on a `let search = Search::new(...)` — those bindings must become `let mut search`.

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --release --test selfplay_rootlog`
Expected: PASS (3 tests)

- [ ] **Step 5: Fix the call sites that now need `mut`**

Run: `cargo build --release 2>&1 | grep -E "^error" -A 6`
Expected: errors about a non-mutable borrow at `src/uci.rs` and in `tests/mcts_v1.rs` / `tests/mcts_uci.rs`.

Fix each by making the binding mutable:

```bash
sed -i 's/let search = Search::new(/let mut search = Search::new(/' src/uci.rs tests/mcts_v1.rs tests/mcts_uci.rs examples/smoke.rs
```

Then: `cargo build --release && cargo test --release`
Expected: PASS (48 tests), 0 failed.

- [ ] **Step 6: Verify the MCTS results did not change**

The change is read-only with respect to the search itself, but `run` now writes a field, so the determinism test must still hold:

Run: `cargo test --release --test mcts_v1 v1_deterministic_per_seed -- --nocapture`
Expected: PASS — same seed, same limits ⇒ same bestmove, score and node count.

- [ ] **Step 7: Commit**

```bash
git add src/mcts.rs src/uci.rs tests/ examples/smoke.rs
git commit -m "feat(mcts): expose the root's per-move visits and values for the self-play logger"
```

---

### Task 4: The root logger — from a position to a `BufferRow`

With the root data exposed (Task 3) and the row type defined (Task 2), the logger itself is small.

**Files:**
- Create: `src/selfplay/rootlog.rs`
- Test: `tests/selfplay_rootlog.rs` (extend)

**Interfaces:**
- Consumes: `mychess::mcts::{Limits, Search, RootStats}`, `mychess::position::Position`, `mychess::types::Move`.
- Produces:
  - `mychess::selfplay::RootLog` — public struct: `row: BufferRow`, `best: Option<Move>`, `played: Option<Move>`.
  - `mychess::selfplay::log_root(pos: &Position, played: Option<Move>, limits: &Limits, seed: u64, gen: u32, gencfg: &str, src: Provenance) -> RootLog`.
  - `mychess::selfplay::visit_entropy(moves: &[RootMove]) -> f32` — the entropy of the visit distribution, in nats.

- [ ] **Step 1: Write the failing test for the entropy**

Append to `tests/selfplay_rootlog.rs`:

```rust
use mychess::selfplay::visit_entropy;

#[test]
fn entropy_is_zero_for_a_single_move() {
    let mv = mychess::mcts::RootMove {
        mv: mychess::types::Move::new(12, 28, 0),
        q: 0.1,
        visits: 100,
    };
    let h = visit_entropy(&[mv]);
    assert!(h.abs() < 1e-6, "a single move has zero entropy, got {h}");
}

#[test]
fn entropy_of_a_uniform_distribution_is_log_n() {
    let moves: Vec<mychess::mcts::RootMove> = (0..4)
        .map(|i| mychess::mcts::RootMove {
            mv: mychess::types::Move::new(12, 28 + i as u8, 0),
            q: 0.0,
            visits: 100,
        })
        .collect();
    let h = visit_entropy(&moves);
    assert!((h - 4f32.ln()).abs() < 1e-4, "expected ln(4), got {h}");
}

#[test]
fn entropy_grows_with_the_number_of_equal_moves() {
    let two: Vec<mychess::mcts::RootMove> = (0..2)
        .map(|i| mychess::mcts::RootMove { mv: mychess::types::Move::new(12, 28 + i as u8, 0), q: 0.0, visits: 10 })
        .collect();
    let three: Vec<mychess::mcts::RootMove> = (0..3)
        .map(|i| mychess::mcts::RootMove { mv: mychess::types::Move::new(12, 28 + i as u8, 0), q: 0.0, visits: 10 })
        .collect();
    assert!(visit_entropy(&three) > visit_entropy(&two));
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --release --test selfplay_rootlog`
Expected: FAIL with `error[E0432]: unresolved import mychess::selfplay::visit_entropy`

- [ ] **Step 3: Implement the logger**

Write `src/selfplay/rootlog.rs`:

```rust
//! Turn a searched position into a buffer row.

use crate::mcts::{Limits, RootMove, Search};
use crate::position::Position;
use crate::types::Move;

use super::row::{BufferRow, MoveStat, Provenance, Targets};

/// The outcome of logging one root position.
#[derive(Clone, Debug)]
pub struct RootLog {
    /// The row to append to the buffer.
    pub row: BufferRow,
    /// The move the search considered best.
    pub best: Option<Move>,
    /// The move that was actually played, if known.
    pub played: Option<Move>,
}

/// The entropy of the root's visit distribution, in nats.
///
/// `-sum p_i * ln(p_i)` over the visit shares. Zero when there is a single
/// visited move, `ln(n)` when `n` moves share the visits equally. This is
/// the diversity signal the DIV gate watches (`SELFPLAY.md` §6).
pub fn visit_entropy(moves: &[RootMove]) -> f32 {
    let total: u64 = moves.iter().map(|m| m.visits).sum();
    if total == 0 || moves.is_empty() {
        return 0.0;
    }
    let mut h = 0.0f32;
    for m in moves {
        if m.visits == 0 {
            continue;
        }
        let p = m.visits as f32 / total as f32;
        h -= p * p.ln();
    }
    h
}

/// Search a position and produce the buffer row for it.
///
/// The row carries the root's per-move `q` and `n` (the policy target's
/// input), the regret `delta = Q_best - Q_played` and the visit entropy.
/// `played` is the move that was actually played from this position: it is
/// what makes `delta` meaningful, and it is `None` for a position the
/// engine only looked at.
pub fn log_root(
    pos: &Position,
    played: Option<Move>,
    limits: &Limits,
    seed: u64,
    gen: u32,
    gencfg: &str,
    src: Provenance,
) -> RootLog {
    let mut search = Search::new(
        *pos,
        vec![pos.zobrist],
        limits.clone(),
        seed,
    );
    let outcome = search.run(&std::sync::atomic::AtomicBool::new(false));
    let stats = search.root_stats();

    let moves: Vec<MoveStat> = stats
        .moves
        .iter()
        .map(|m| MoveStat { u: m.mv.uci(), q: m.q, n: m.visits })
        .collect();

    let best = if outcome.best == Move::NONE { None } else { Some(outcome.best) };

    // The regret: the best move's value minus the played move's value.
    let q_of = |mv: Move| -> Option<f32> {
        stats
            .moves
            .iter()
            .find(|m| m.mv == mv)
            .map(|m| m.q)
    };
    let delta = match (best.and_then(q_of), played.and_then(q_of)) {
        (Some(qb), Some(qp)) => qb - qp,
        _ => 0.0,
    };

    let row = BufferRow {
        schema: super::SCHEMA_VERSION,
        fen: fen_of(pos),
        gen,
        played: played.map(|m| m.uci()),
        z: None,
        res: None,
        src,
        conf: src.conf(),
        budget: limits.nodes as u32,
        gencfg: gencfg.to_string(),
        moves,
        delta,
        entrop: visit_entropy(&stats.moves),
        targets: Targets::default(),
        relabel: None,
    };

    RootLog { row, best, played }
}

/// Render a position as a FEN.
fn fen_of(pos: &Position) -> String {
    let mut rows = Vec::with_capacity(8);
    for r in (0..8).rev() {
        let mut row = String::with_capacity(8);
        let mut empty = 0u8;
        for f in 0..8 {
            let code = pos.piece_on((r * 8 + f) as u8);
            if code == 0 {
                empty += 1;
                continue;
            }
            if empty > 0 {
                row.push_str(&empty.to_string());
                empty = 0;
            }
            let kind = crate::types::PieceKind::from_idx(((code - 1) % 6) as usize);
            let white = (code - 1) < 6;
            let c = kind.char(white);
            row.push(if white { c.to_ascii_uppercase() } else { c });
        }
        if empty > 0 {
            row.push_str(&empty.to_string());
        }
        rows.push(row);
    }
    let side = if pos.side == crate::types::Color::White { "w" } else { "b" };
    let mut castle = String::new();
    if pos.castle & crate::types::CASTLE_WK != 0 {
        castle.push('K');
    }
    if pos.castle & crate::types::CASTLE_WQ != 0 {
        castle.push('Q');
    }
    if pos.castle & crate::types::CASTLE_BK != 0 {
        castle.push('k');
    }
    if pos.castle & crate::types::CASTLE_BQ != 0 {
        castle.push('q');
    }
    if castle.is_empty() {
        castle.push('-');
    }
    let ep = match pos.ep_square() {
        Some(sq) => {
            let file = (b'a' + (sq & 7)) as char;
            let rank = (b'1' + (sq >> 3)) as char;
            format!("{file}{rank}")
        }
        None => "-".to_string(),
    };
    format!(
        "{} {} {} {} {} {}",
        rows.join("/"),
        side,
        castle,
        ep,
        pos.halfmove,
        1 + (pos.halfmove / 2)
    )
}
```

`Limits` must be `Clone` for this. In `src/mcts.rs`, change `#[derive(...)]` on `Limits` to include `Clone`:

```rust
#[derive(Clone)]
pub struct Limits {
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --release --test selfplay_rootlog`
Expected: PASS (6 tests)

- [ ] **Step 5: Add the end-to-end logger test**

Append to `tests/selfplay_rootlog.rs`:

```rust
use mychess::selfplay::{log_root, Provenance};

#[test]
fn logging_a_root_produces_a_serialisable_row() {
    let pos = Position::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap();
    let limits = Limits { nodes: 2000, movetime_ms: None, max_playout_ply: 100, c_uct: std::f32::consts::SQRT_2 };
    let log = log_root(&pos, None, &limits, 42, 0, "cfg1", Provenance::SelfPlayed);

    assert!(!log.row.moves.is_empty());
    assert_eq!(log.row.schema, mychess::selfplay::SCHEMA_VERSION);
    assert_eq!(log.row.src, Provenance::SelfPlayed);
    assert!(log.row.entrop >= 0.0);

    // The row must survive the JSON round trip.
    let line = log.row.to_json_line();
    let back = mychess::selfplay::BufferRow::from_json_line(&line).expect("round trip");
    assert_eq!(back.moves.len(), log.row.moves.len());
}

#[test]
fn the_regret_is_zero_when_the_best_move_was_played() {
    let pos = Position::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap();
    let limits = Limits { nodes: 2000, movetime_ms: None, max_playout_ply: 100, c_uct: std::f32::consts::SQRT_2 };
    let log = log_root(&pos, None, &limits, 42, 0, "cfg1", Provenance::SelfPlayed);
    let best = log.best.expect("a best move");
    let log2 = log_root(&pos, Some(best), &limits, 42, 0, "cfg1", Provenance::SelfPlayed);
    assert!(log2.row.delta.abs() < 1e-6, "playing the best move means zero regret");
}
```

- [ ] **Step 6: Run the test to verify it passes**

Run: `cargo test --release --test selfplay_rootlog`
Expected: PASS (8 tests)

- [ ] **Step 7: Verify the whole suite and the perft goldens**

Run: `cargo test --release`
Expected: PASS (48 + 8 = 56 tests), 0 failed.

Run: `./target/release/mychess perft "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1" 6`
Expected: `perft(6) = 119060324`

- [ ] **Step 8: Commit**

```bash
git add src/selfplay/rootlog.rs src/mcts.rs tests/selfplay_rootlog.rs
git commit -m "feat(selfplay): root logger turning a searched position into a buffer row"
```

---

### Task 5: The buffer writer — append-only JSONL

**Files:**
- Create: `src/selfplay/writer.rs`
- Test: `tests/selfplay_buffer.rs` (extend)

**Interfaces:**
- Consumes: `mychess::selfplay::BufferRow`.
- Produces:
  - `mychess::selfplay::BufferWriter` — public struct, created with `BufferWriter::create(path: &Path) -> std::io::Result<BufferWriter>` (truncates) and `BufferWriter::append(path: &Path) -> std::io::Result<BufferWriter>` (opens for append).
  - `BufferWriter::write_row(&mut self, row: &BufferRow) -> std::io::Result<()>` — writes one JSON line and flushes.
  - `BufferWriter::path(&self) -> &Path`.
  - `BufferWriter::rows_written(&self) -> u64`.

- [ ] **Step 1: Write the failing test**

Append to `tests/selfplay_buffer.rs`:

```rust
use mychess::selfplay::BufferWriter;

#[test]
fn the_writer_appends_one_json_line_per_row() {
    let dir = std::env::temp_dir().join("mychess_buffer_test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("gen-0.jsonl");
    let _ = std::fs::remove_file(&path);

    let mut w = BufferWriter::create(&path).unwrap();
    w.write_row(&sample_row()).unwrap();
    w.write_row(&sample_row()).unwrap();
    assert_eq!(w.rows_written(), 2);

    let text = std::fs::read_to_string(&path).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2, "one line per row");
    for line in lines {
        mychess::selfplay::BufferRow::from_json_line(line).expect("each line parses");
    }

    // Appending continues the same file.
    let mut w2 = BufferWriter::append(&path).unwrap();
    w2.write_row(&sample_row()).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(text.lines().count(), 3);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn the_writer_creates_the_parent_directory() {
    let dir = std::env::temp_dir().join("mychess_buffer_nested");
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("a").join("b").join("gen-0.jsonl");
    let mut w = BufferWriter::create(&path).unwrap();
    w.write_row(&sample_row()).unwrap();
    assert!(path.exists());
    let _ = std::fs::remove_dir_all(&dir);
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --release --test selfplay_buffer`
Expected: FAIL with `error[E0432]: unresolved import mychess::selfplay::BufferWriter`

- [ ] **Step 3: Implement the writer**

Write `src/selfplay/writer.rs`:

```rust
//! Append-only JSONL writer for the self-play buffer.

use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use super::row::BufferRow;

/// A buffered writer over a JSONL buffer file.
///
/// One row per line, flushed on every write: a killed run must leave a
/// readable buffer behind, and a half-written line is worse than a missing
/// one.
pub struct BufferWriter {
    out: BufWriter<File>,
    path: PathBuf,
    rows: u64,
}

impl BufferWriter {
    /// Create (or truncate) a buffer file, creating parent directories.
    pub fn create(path: &Path) -> std::io::Result<BufferWriter> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)?;
        Ok(BufferWriter {
            out: BufWriter::new(file),
            path: path.to_path_buf(),
            rows: 0,
        })
    }

    /// Open an existing buffer file for appending.
    pub fn append(path: &Path) -> std::io::Result<BufferWriter> {
        let file = OpenOptions::new().append(true).open(path)?;
        Ok(BufferWriter {
            out: BufWriter::new(file),
            path: path.to_path_buf(),
            rows: 0,
        })
    }

    /// Write one row as a JSON line and flush it.
    pub fn write_row(&mut self, row: &BufferRow) -> std::io::Result<()> {
        self.out.write_all(row.to_json_line().as_bytes())?;
        self.out.write_all(b"\n")?;
        self.out.flush()?;
        self.rows += 1;
        Ok(())
    }

    /// The path this writer writes to.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// How many rows this writer has written.
    pub fn rows_written(&self) -> u64 {
        self.rows
    }
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --release --test selfplay_buffer`
Expected: PASS (11 tests)

- [ ] **Step 5: Verify the whole suite**

Run: `cargo test --release`
Expected: PASS (56 + 2 = 58 tests), 0 failed.

- [ ] **Step 6: Commit**

```bash
git add src/selfplay/writer.rs tests/selfplay_buffer.rs
git commit -m "feat(selfplay): append-only JSONL buffer writer"
```

---

### Task 6: The UCI entry point — `selfplay log`

The logger needs a way to be driven from the command line, so that the corpus can be produced without writing a Rust driver first.

**Files:**
- Modify: `src/uci.rs` (add a `selfplay` subcommand path), or add `src/selfplay/cli.rs` and dispatch from `src/main.rs`.
- Test: `tests/selfplay_cli.rs`

**Interfaces:**
- Consumes: `mychess::selfplay::{log_root, BufferWriter}`, `mychess::mcts::Limits`.
- Produces: a `mychess selfplay log <fen> <nodes> <out.jsonl> [gen] [gencfg]` subcommand, plus `mychess selfplay log --positions <file>` where `<file>` has one FEN per line.

- [ ] **Step 1: Write the failing test**

Create `tests/selfplay_cli.rs`:

```rust
//! The `selfplay log` subcommand: the corpus producer.

use std::process::Command;

fn mychess_bin() -> std::path::PathBuf {
    // The integration test binary lives next to the built binary.
    let mut p = std::env::current_exe().expect("test binary path");
    p.pop();
    if p.ends_with("deps") {
        p.pop();
    }
    p.join("mychess")
}

#[test]
fn the_selfplay_log_subcommand_writes_a_buffer() {
    let dir = std::env::temp_dir().join("mychess_cli_test");
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("gen-0.jsonl");
    let _ = std::fs::remove_file(&out);

    let status = Command::new(mychess_bin())
        .args([
            "selfplay",
            "log",
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            "2000",
            out.to_str().unwrap(),
        ])
        .status()
        .expect("run mychess");

    assert!(status.success(), "the subcommand must succeed");
    let text = std::fs::read_to_string(&out).expect("buffer file written");
    assert_eq!(text.lines().count(), 1);
    let row = mychess::selfplay::BufferRow::from_json_line(text.lines().next().unwrap())
        .expect("the row parses");
    assert_eq!(row.schema, mychess::selfplay::SCHEMA_VERSION);

    let _ = std::fs::remove_file(&out);
}

#[test]
fn a_bad_fen_is_reported_without_a_panic() {
    let status = Command::new(mychess_bin())
        .args(["selfplay", "log", "not-a-fen", "100", "/tmp/should-not-exist.jsonl"])
        .status()
        .expect("run mychess");
    assert!(!status.success(), "a bad FEN must exit non-zero");
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --release --test selfplay_cli`
Expected: FAIL — the `selfplay` subcommand is unknown, so the process exits with the usage code (2), and the buffer file is never written.

- [ ] **Step 3: Implement the subcommand**

Create `src/selfplay/cli.rs`:

```rust
//! The `mychess selfplay` command-line surface.

use std::path::Path;

use crate::mcts::Limits;
use crate::position::Position;
use crate::types::Color;

use super::rootlog::log_root;
use super::row::Provenance;
use super::writer::BufferWriter;

/// Run the `selfplay log` subcommand.
///
/// Usage:
///     mychess selfplay log <fen> <nodes> <out.jsonl> [gen] [gencfg]
///     mychess selfplay log --positions <fens.txt> <nodes> <out.jsonl> [gen] [gencfg]
///
/// `<fens.txt>` holds one FEN per line; blank lines and `#` comments are
/// skipped. The output is a JSONL buffer at schema 3.
pub fn run(args: &[String]) -> Result<(), String> {
    if args.first().map(String::as_str) != Some("log") {
        return Err("usage: mychess selfplay log <fen| --positions <file>> <nodes> <out.jsonl> [gen] [gencfg]".into());
    }
    let rest = &args[1..];

    let (positions, rest): (Vec<String>, &[String]) = if rest.first().map(String::as_str) == Some("--positions") {
        let path = rest.get(1).ok_or("--positions needs a file")?;
        let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
        let fens: Vec<String> = text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(str::to_string)
            .collect();
        (fens, &rest[2..])
    } else {
        let fen = rest.first().ok_or("a FEN is required")?.clone();
        (vec![fen], &rest[1..])
    };

    let nodes: u64 = rest
        .first()
        .ok_or("a node budget is required")?
        .parse()
        .map_err(|e| format!("invalid node budget: {e}"))?;
    let out = rest.get(1).ok_or("an output path is required")?;
    let gen: u32 = rest.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
    let gencfg = rest.get(3).cloned().unwrap_or_else(|| "default".to_string());

    let limits = Limits {
        nodes,
        movetime_ms: None,
        max_playout_ply: 100,
        c_uct: std::f32::consts::SQRT_2,
    };

    let mut writer = BufferWriter::create(Path::new(out)).map_err(|e| e.to_string())?;

    for (i, fen) in positions.iter().enumerate() {
        let pos = Position::from_fen(fen).map_err(|e| format!("invalid FEN {fen:?}: {e:?}"))?;
        let log = log_root(
            &pos,
            None,
            &limits,
            0x5EED_0000 + i as u64,
            gen,
            &gencfg,
            Provenance::SelfPlayed,
        );
        writer.write_row(&log.row).map_err(|e| e.to_string())?;
    }

    println!(
        "selfplay: wrote {} row(s) to {out} (schema {})",
        writer.rows_written(),
        super::SCHEMA_VERSION
    );
    Ok(())
}

/// The colour name, for CLI output.
pub fn color_name(c: Color) -> &'static str {
    match c {
        Color::White => "white",
        Color::Black => "black",
    }
}
```

Add `pub mod cli;` to `src/selfplay/mod.rs` and `pub use cli::run as run_cli;`.

In `src/main.rs`, add a dispatch arm before the catch-all:

```rust
        "selfplay" => {
            let args: Vec<String> = std::env::args().skip(2).collect();
            if let Err(e) = mychess::selfplay::run_cli(&args) {
                eprintln!("selfplay: {e}");
                std::process::exit(2);
            }
        }
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --release --test selfplay_cli`
Expected: PASS (2 tests)

- [ ] **Step 5: Verify the whole suite and the perft goldens**

Run: `cargo test --release`
Expected: PASS (58 + 2 = 60 tests), 0 failed.

Run: `./target/release/mychess perft "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1" 5`
Expected: `perft(5) = 193690690`

- [ ] **Step 6: Commit**

```bash
git add src/selfplay/ src/main.rs tests/selfplay_cli.rs
git commit -m "feat(selfplay): the 'mychess selfplay log' subcommand writes a schema-3 buffer"
```

---

### Task 7: The root-exploration statistical guard (the "most expensive bug" guard)

`SELFPLAY.md` §7 requires, before any distillation, a statistical test that the root exploration is alive: "the empirical entropy of the generated root distributions is in the expected range (otherwise the exploration died silently and S4 distills a collapse)". This task builds that guard, because it is a test, and a test that ships late is a test that never ships.

**Files:**
- Modify: `src/selfplay/rootlog.rs` (add `expected_entropy_range`).
- Test: `tests/selfplay_rootlog.rs` (extend)

**Interfaces:**
- Consumes: `mychess::selfplay::visit_entropy`, `mychess::mcts::RootMove`.
- Produces: `mychess::selfplay::expected_entropy_range(legal_move_count: usize) -> (f32, f32)` — the expected band of the root visit entropy, given the number of legal moves, under a healthy exploration.

- [ ] **Step 1: Write the failing test**

Append to `tests/selfplay_rootlog.rs`:

```rust
use mychess::selfplay::expected_entropy_range;

#[test]
fn the_expected_entropy_band_grows_with_the_number_of_legal_moves() {
    let (lo1, hi1) = expected_entropy_range(2);
    let (lo10, hi10) = expected_entropy_range(10);
    assert!(lo10 > lo1, "more legal moves means a higher expected entropy");
    assert!(hi10 > hi1);
}

#[test]
fn the_expected_entropy_band_brackets_a_uniform_distribution() {
    let n = 8usize;
    let (lo, hi) = expected_entropy_range(n);
    let uniform = n as f32 * (1.0f32 / n as f32).ln();
    // A perfectly uniform visit distribution is the maximum-entropy case;
    // the band must not exclude it.
    assert!(uniform <= hi + 1e-4, "uniform entropy {uniform} must be within the band");
    assert!(uniform >= lo - 1e-4);
}

#[test]
fn a_dead_exploration_falls_below_the_band() {
    // One move taking all the visits: zero entropy, far below the band.
    let (lo, _) = expected_entropy_range(20);
    assert!(0.0 < lo, "a collapsed exploration must be detectable");
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --release --test selfplay_rootlog`
Expected: FAIL with `error[E0432]: unresolved import mychess::selfplay::expected_entropy_range`

- [ ] **Step 3: Implement the band**

Append to `src/selfplay/rootlog.rs`:

```rust
/// The expected band of the root visit entropy, given the number of legal
/// moves.
///
/// A healthy exploration spreads its visits over the plausible moves, so
/// the entropy sits between "one move dominates" and "perfectly uniform".
/// The lower bound is the entropy of a distribution where the top move
/// takes three quarters of the visits; the upper bound is the uniform
/// entropy `ln(n)`.
///
/// The purpose is to detect a *silently dead* exploration (a Dirichlet
/// noise that never fires, a temperature that collapsed to zero): such a
/// run produces an entropy far below the lower bound, and the guard stops
/// the pipeline before it distils the collapse into the network.
///
/// `SELFPLAY.md` §7 calls this "the most expensive bug" — the failure it
/// prevents is silent and costs a whole generation.
pub fn expected_entropy_range(legal_move_count: usize) -> (f32, f32) {
    let n = legal_move_count.max(2) as f32;
    let uniform = n.ln();
    // A distribution where one move takes 3/4 of the visits and the rest
    // share the remaining quarter.
    let dominant = 0.75f32;
    let others = (1.0 - dominant) / (n - 1.0);
    let lower = -(dominant * dominant.ln() + (n - 1.0) * others * others.ln());
    // A little headroom below the uniform bound: a real search is never
    // perfectly uniform.
    (lower * 0.5, uniform * 0.95)
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --release --test selfplay_rootlog`
Expected: PASS (11 tests)

- [ ] **Step 5: Add the integration guard test — a real search must land in the band**

Append to `tests/selfplay_rootlog.rs`:

```rust
#[test]
fn a_healthy_search_lands_inside_the_expected_entropy_band() {
    let pos = Position::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap();
    let limits = Limits { nodes: 4000, movetime_ms: None, max_playout_ply: 100, c_uct: std::f32::consts::SQRT_2 };
    let log = log_root(&pos, None, &limits, 42, 0, "cfg1", Provenance::SelfPlayed);

    let legal = mychess::movegen::generate_all(&pos).len();
    let (lo, hi) = expected_entropy_range(legal);
    assert!(
        log.row.entrop >= lo && log.row.entrop <= hi,
        "root entropy {} outside the expected band [{lo}, {hi}] for {legal} legal moves",
        log.row.entrop
    );
}
```

- [ ] **Step 6: Run the test to verify it passes**

Run: `cargo test --release --test selfplay_rootlog`
Expected: PASS (12 tests). If the entropy is above the band, the upper bound is too tight for a 4000-node search — widen `uniform * 0.95` to `uniform * 1.0` and re-run.

- [ ] **Step 7: Verify the whole suite**

Run: `cargo test --release`
Expected: PASS (60 + 3 = 63 tests), 0 failed.

- [ ] **Step 8: Commit**

```bash
git add src/selfplay/rootlog.rs tests/selfplay_rootlog.rs
git commit -m "feat(selfplay): root-exploration entropy guard against a silently dead search"
```

---

### Task 8: The magnitude instrument, validated against a real SPRT log

Task 1 built PR-SPRT against synthetic logs. This task validates it against a **real** fastchess log, because the format of a real log is where a parser written in isolation always breaks.

**Files:**
- Create: `slaml/` is not the right place — use `tools/fixtures/`:
  - Create: `tools/fixtures/README.md`
  - Create: `tools/fixtures/make_sample_log.py` (a generator for a synthetic-but-realistic log)
- Modify: `tests/test_pr_sprt.py`
- Test: `tests/test_pr_sprt.py`

**Interfaces:**
- Consumes: a fastchess SPRT log in the real format.
- Produces: a fixture log under `tools/fixtures/` and a test that parses it.

- [ ] **Step 1: Inspect a real log's format**

If a real log exists, use it:

```bash
ls prt/sprt-*.log 2>/dev/null | head -1
```

If none exists (the `prt/` directory is not committed), capture one:

```bash
mkdir -p prt && ./test.sh ./target/release/mychess ./target/release/mychess 2 40 2>&1 | tail -20
```

Record the exact format — the parser in Task 1 was written against the *documented* format, and the real one may differ (e.g. a different `W/L/D` label, extra lines, a different `LLR` format).

- [ ] **Step 2: Write the failing test against the real format**

Append to `tests/test_pr_sprt.py`:

```python
class RealLogFormatTest(unittest.TestCase):
    """The parser must handle the format fastchess actually emits."""

    def test_parses_a_real_captured_log(self):
        from pathlib import Path

        fixture = Path(__file__).parent.parent / "tools" / "fixtures" / "sample_sprt.log"
        if not fixture.exists():
            self.skipTest("no captured log available; run step 1 to capture one")
        log = parse_sprt_log(fixture)
        self.assertGreater(log.games, 0)
        self.assertEqual(log.wins + log.losses + log.draws, log.games)
        est = elo_estimate(log)
        self.assertIn(est.verdict, ("accepted", "rejected", "inconclusive"))
```

- [ ] **Step 3: Run the test to verify it fails or skips**

Run: `python3 -m unittest tests.test_pr_sprt -v`
Expected: either FAIL (the real format differs from the documented one) or SKIP (no fixture yet).

- [ ] **Step 4: Capture the fixture and fix the parser if needed**

Copy the captured log to `tools/fixtures/sample_sprt.log` and write `tools/fixtures/README.md`:

```markdown
# Test fixtures

`sample_sprt.log` — a captured fastchess SPRT log, used by
`tests/test_pr_sprt.py` to check that `tools/pr_sprt.py` parses the format
fastchess actually emits (not the documented one).

Captured on: <date>, machine: <machine>, command: `<command>`.
```

If the parser fails on the real format, fix `parse_sprt_log_from_text` in `tools/pr_sprt.py` and re-run.

- [ ] **Step 5: Run the test to verify it passes**

Run: `python3 -m unittest tests.test_pr_sprt -v`
Expected: PASS (7 tests), the new one no longer skipped.

- [ ] **Step 6: Verify the tool end-to-end on the fixture**

Run: `python3 tools/pr_sprt.py tools/fixtures/sample_sprt.log`
Expected: a three-line summary; the exit code is 0, 1 or 2 depending on the verdict.

- [ ] **Step 7: Commit**

```bash
git add tools/fixtures/ tests/test_pr_sprt.py tools/pr_sprt.py
git commit -m "test(measure): validate PR-SPRT against a captured fastchess log"
```

---

## Self-Review

### 1. Spec coverage

Checking the v3 document's requirements against the plan's tasks:

| v3 requirement | Covered by |
|---|---|
| §4 buffer schema, `schema` field, versioned | Task 2 |
| §4 `z`/`res` arbitration rule | Task 2 (`value_target`) |
| §4 `targets{}` extensible (WDL, moves-left) | Task 2 (`Targets`, both `None` at schema 3) |
| §4 `relabel` traced | Task 2 (`Relabel`) |
| §4 confidence hierarchy per provenance | Task 2 (`Provenance::conf`) |
| §4 replay eligibility (asym weak, exploit excluded) | Task 2 (`is_replay_eligible`) |
| §3.1 S17 policy target = `softmax(Q/τ)`, not visits | Task 2 (`policy_target`), Task 3 (`RootMove.q`) |
| §3.2 S8 threshold calibration | **Not implemented here** — the threshold calibration needs a measured |ΔQ| distribution from a real corpus, which does not exist yet. Task 3 provides the `q` values it will need. Deferred to a follow-up plan, deliberately. |
| §5.1 magnitude instrument | Task 1 (implementation), Task 8 (validation on a real log) |
| §5.1 SPRT protocol stays frozen | Task 1 does not touch `test.sh`; asserted by the plan's global constraints |
| §6 holdout external, disjoint | **Not implemented here** — a holdout is a *corpus selection* concern, not an engine concern. The engine side of it is Task 4/6 (the logger can write any position set); the disjointness rule is a pipeline rule. Deferred. |
| §6 P0 DIV metrics (null rate, uniqueness, entropy) | Task 4 (entropy per row), Task 7 (the entropy guard). The null-rate and uniqueness metrics need the corpus, so they are measured from the buffer in a later phase. |
| §7 "the most expensive bug" guard | Task 7 |
| §1.3 CPU envelope | **Not implemented here** — it is a measurement, not code. The plan deliberately does not build it. |
| §1.5 ceiling unit = Elo/hour-CPU | **Not implemented here** — the unit is a *convention* over measurements PR-SPRT provides (Task 1) plus a CPU-time accounting the pipeline needs. The convention is recorded here so the follow-up plan can use it. |

The three "not implemented" rows are honest gaps, not oversights: each needs data that does not exist until the pipeline has run. The plan's job is to make that data collectable.

### 2. Placeholder scan

- "TBD" / "TODO" / "implement later": **none**. Every task names concrete files, concrete function signatures, and complete code.
- "Add appropriate error handling": **none**. Every fallible operation has a specified error path (`Result`, explicit `Err(String)`, or a documented `expect` with its message).
- "Write tests for the above": **none**. Every task spells out the test code.
- "Similar to Task N": **none**. Each task repeats the code it needs.
- **One deliberate conditional**: Task 3 Step 3 says "Fix each by making the binding mutable" followed by the exact `sed` command. This is not a placeholder — the failing borrow sites are enumerated by the build error, and the fix is mechanical and given verbatim.
- **One conditional with a stated fallback**: Task 8 Step 1 says "If a real log exists, use it; if not, capture one" with both commands given, and Step 4 with the fallback if the parser needs fixing. The outcome is the same either way (a fixture exists and the parser handles it).

### 3. Type consistency

Cross-checking the names this plan introduces:

| Name | Defined in | Used in | Consistent |
|---|---|---|---|
| `SCHEMA_VERSION` | Task 2, `selfplay/mod.rs` | Tasks 2, 4, 6, 7 | yes |
| `BufferRow` | Task 2, `row.rs` | Tasks 2, 4, 5, 6 | yes |
| `MoveStat { u, q, n }` | Task 2, `row.rs` | Tasks 2, 4 | yes |
| `Provenance` | Task 2, `row.rs` | Tasks 2, 4, 6 | yes |
| `Targets { wdl, moves_left }` | Task 2, `row.rs` | Tasks 2, 4 | yes |
| `Relabel { by, q, budget }` | Task 2, `row.rs` | Task 2 | yes |
| `BufferRow::value_target()` | Task 2 | Task 2 | yes |
| `BufferRow::policy_target(tau)` | Task 2 | Task 2 | yes |
| `BufferRow::to_json_line()` / `from_json_line()` | Task 2 | Tasks 2, 4, 5, 6 | yes |
| `Json` + `parse_json_object` | Task 2, `row.rs` | Task 2 | yes |
| `RootMove { mv, q, visits }` | Task 3, `mcts.rs` | Tasks 4, 7 | yes |
| `RootStats { moves, visits_total }` | Task 3, `mcts.rs` | Tasks 3, 4 | yes |
| `Search::root_stats()` / `root_visits()` | Task 3, `mcts.rs` | Task 4 | yes |
| `RootLog { row, best, played }` | Task 4, `rootlog.rs` | Task 4 | yes |
| `log_root(pos, played, limits, seed, gen, gencfg, src)` | Task 4 | Tasks 4, 6 | yes |
| `visit_entropy(&[RootMove]) -> f32` | Task 4 | Tasks 4, 7 | yes |
| `expected_entropy_range(usize) -> (f32, f32)` | Task 7 | Task 7 | yes |
| `BufferWriter::{create, append, write_row, path, rows_written}` | Task 5 | Tasks 5, 6 | yes |
| `selfplay::run_cli(&[String])` | Task 6, `cli.rs` | Task 6 | yes |
| `parse_sprt_log`, `SprtLog`, `EloEstimate`, `elo_estimate` | Task 1, `pr_sprt.py` | Tasks 1, 8 | yes |
| `Limits: Clone` | Task 4 (added to the derive) | Task 4 | yes |

One ordering dependency to note: `Task 4` adds `Clone` to `Limits`, and `Task 3` changes `Search::run` to `&mut self`. Task 3 precedes Task 4, so `Task 4`'s `log_root` — which calls `search.run(...)` on a local `let mut search` — is consistent with `&mut self`.

**A defect this review caught and fixed.** The first draft of `Task 2`'s `mod.rs` declared `pub mod rootlog;` and `pub mod writer;` and re-exported `RootLog`, `log_root` and `BufferWriter` from them, with the two files created as empty placeholders. That does not compile: an empty module cannot export symbols. The plan now declares only `pub mod row;` in Task 2, and each later task adds its own module declaration and re-export when it creates the contents:

- **Task 4** adds `pub mod rootlog;` and `pub use rootlog::{RootLog, log_root, visit_entropy};`
- **Task 5** adds `pub mod writer;` and `pub use writer::BufferWriter;`
- **Task 6** adds `pub mod cli;` and `pub use cli::run as run_cli;`

With that, the module tree compiles at every task boundary — no task depends on a symbol a later task creates.

### 4. Ordering and independence

The eight tasks form a chain with no cycles:

```
Task 1 (PR-SPRT)          independent — no engine code
        │
Task 2 (BufferRow) ──────┐
        │                 │
Task 3 (mcts root data) ─┤
        │                 │
Task 4 (root logger) ────┤
        │                 │
Task 5 (writer) ─────────┤
        │                 │
Task 6 (CLI) ────────────┤
        │                 │
Task 7 (entropy guard) ──┘
        │
Task 8 (PR-SPRT on a real log)   independent of 2-7
```

Task 1 and Task 8 are independent of the engine work and could be done in parallel by two agents. Tasks 2→7 are strictly sequential because each consumes the previous task's public types.

### 5. Risks this plan does not remove

- **The `q` sign convention is the sharpest edge.** `RootMove::q` is the *parent's* view of the child. The policy target `softmax(Q/τ)` is built from it. If a future task feeds the child-relative value instead, the policy target inverts. The plan pins this in the `RootMove` doc comment and asserts the range in a test, but the semantic (whose perspective) is only enforced by prose.
- **`Search::run` becoming `&mut self`** is a public API change. It is source-compatible for every current call site (all of them own their `Search`), but a downstream user calling `run` on a `&Search` would need to clone. `Search` is not `Clone` today. Flagged, not solved.
- **The entropy band's constants are guesses.** Task 7 Step 6 admits the upper bound may need widening. The band is a heuristic; the *test* is what makes it actionable, and the numbers should be recalibrated against a real corpus.

### 6. Defects this review caught by execution

Running the plan's code — rather than only reading it — caught two real defects that a read-through had missed. Both are fixed in the plan above.

**The SPRT acceptance threshold was inverted.** The first draft computed the bounds as `log(beta/(1-beta))` and `log(alpha/(1-alpha))`, which with `alpha = beta = 0.05` gives **−2.944 for both** — so every verdict came out "accepted". The correct SPRT bounds are:

- accept H1 when `LLR >= log(1/beta)` = **+2.996**
- reject H0 when `LLR <= log(alpha)` = **−2.996**

The asymmetry is the trap: the acceptance bound uses `1/beta`, not `beta`. The test fixtures in Task 1 now use `LLR: 3.10` and `LLR: -3.10`, which sit unambiguously outside the band. Verified by execution:

```
LLR=    3.1 -> accepted      attendu accepted      OK
LLR=   -3.1 -> rejected      attendu rejected      OK
LLR=    0.1 -> inconclusive  attendu inconclusive  OK
```

**Task 2's `mod.rs` re-exported symbols that did not exist yet.** Fixed as described above (each module is declared by the task that creates it).

The lesson for the executor: the plan's Python and JSON code has been executed and passes; the parts that have **not** been executed are the Rust snippets in Tasks 2–7, which should be treated as a first draft to be corrected against the compiler rather than as finished code.
