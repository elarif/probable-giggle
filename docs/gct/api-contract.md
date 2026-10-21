# API Contract — "The Pipeline and Perft" 7-part C# move-generator series
Source: https://grandchesstree.com/move-generator/{01-introduction,02-board,03-move-encoding,04-pseudo-legal-moves,05-make-move,06-magic-bitboards,07-legal-moves}.html
Author: Tim Jones. All names/signatures below are verbatim from the fetched pages.

**Note on Zobrist:** the series contains **no** Zobrist hashing at all (0 occurrences of "Zobrist" across all 7 pages). `Position` has no hash field, and make/unmake performs no incremental hash update. Do not add one to the contract unless asked.

---

## Part 1 — The Pipeline and Perft

### Types
- `public static class FenBoard` — `public static string Render(string fen)`
- `public static class Perft` — `public static ulong Run(Position position, int depth)`
- `public sealed class Position { }` (throwaway stub, replaced in Part 2)
- `public readonly struct Move { }` (throwaway stub, replaced in Part 3)
- `public static class MoveGenerator` — `public static Move[] GenerateMoves(Position position) => System.Array.Empty<Move>();` (stub)
- `public static class PositionExtensions` — `public static void MakeMove(this Position p, Move m) { }`, `public static void UnmakeMove(this Position p, Move m) { }` (stubs)
- `internal class Program` — `const string InitialFen`, `const string KiwipeteFen`, `static void Main()`

### Perft loop shape (Part 1 version — no bulk counting)
```csharp
public static ulong Run(Position position, int depth)
{
    if (depth == 0) return 1UL;
    ulong nodes = 0UL;
    Move[] moves = MoveGenerator.GenerateMoves(position);
    for (int i = 0; i < moves.Length; i++)
    {
        position.MakeMove(moves[i]);
        nodes += Run(position, depth - 1);
        position.UnmakeMove(moves[i]);
    }
    return nodes;
}
```
Nodes counted only at leaves; interior nodes not counted. "The shape of perft never changes throughout the series; only the things it calls do."

### Perft references asserted
Initial position: depth 0=1, 1=20, 2=400, 3=8,902, 4=197,281, 5=4,865,609, 6=119,060,324.
Kiwipete FEN `r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq -`: depth 1=48, 2=2,039, 3=97,862, 4=4,085,603, 5=193,690,690.

### Deliberately NOT done
- `FenBoard.Render` processes **only the first FEN field** (piece placement). "Full FEN parsing comes in Part 2."
- No move generator yet: `perft(1)` returns 0 because the stub generator returns no moves.
- `Position`, `Move`, `MoveGenerator` are stubs "we'll replace in Part 2".

---

## Part 2 — Representing the Board

### Types
- `public enum Piece : byte` — `WhitePawn, WhiteKnight, WhiteBishop, WhiteRook, WhiteQueen, WhiteKing, BlackPawn, BlackKnight, BlackBishop, BlackRook, BlackQueen, BlackKing, None` (implicit values 0..12; doubles as bitboard array index for 0..11).
- `[Flags] public enum CastlingRights : byte` — `None = 0, WhiteKingside = 1, WhiteQueenside = 2, BlackKingside = 4, BlackQueenside = 8`.
- `public sealed class Position` (Part 5 later makes it `public sealed partial class Position`):
  - `public ulong[] Pieces = new ulong[12];` — 12 bitboards, indexed by `(int)Piece` (0..11)
  - `public Piece[] Squares = new Piece[64];` — 8×8 mailbox, `Piece.None` on empty
  - `public ulong WhiteOccupied; public ulong BlackOccupied; public ulong AllOccupied;`
  - `public bool WhiteToMove; public CastlingRights Castling; public int EpSquare;` (-1 if none) `public int HalfmoveClock; public int FullmoveNumber;`
- `public static class Fen`:
  - `public const string Initial = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";`
  - `public const string Kiwipete = "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq -";`
  - `public static Position Parse(string fen)`
  - `static Piece PieceFromChar(char c)`
  - `static int SquareFromName(string s)` → `(s[1]-'1')*8 + (s[0]-'a')`
  - second listing: `public static string Write(Position pos)`, `static char CharOf(Piece p)` → `"PNBRQKpnbrqk."[(int)p]`, `static string SquareName(int sq)`, `static string CastlingString(CastlingRights c)`
- `public static class PositionPrinter` — `public static string Render(Position pos)`, `static string SquareName(int sq)`
- Free helpers shown as snippets: `int Square(int file, int rank) => rank * 8 + file;`, `int FileOf(int sq) => sq & 7;`, `int RankOf(int sq) => sq >> 3;`

### Board representation scheme
- **Square indexing: LERF (Little-Endian Rank-File)**, Stockfish convention. Bit `n` = square `n`; `a1 = 0`, `h1 = 7`, `a8 = 56`, `h8 = 63`; `sq = rank*8 + file`, `file = sq & 7`, `rank = sq >> 3`.
- Bitboard layout: 64 bits, bit `n` ↔ square `n` (see LERF table in page).
- Direction shifts under LERF: N `<<8`, S `>>8`, E `<<1`, W `>>1`, NE `<<9`, NW `<<7`, SE `>>7`, SW `>>9`.
- Bitboards are the **source of truth**; the mailbox `Squares[64]` and the three occupancy aggregates are **caches** that must move in lockstep.
- 12 bitboards = 96 bytes.

### Deliberately NOT done
- No `Validate()` yet (Part 5 adds it).
- Alternative representations named but rejected: 10×12, 0x88, piece-lists — "the rest of this series uses bitboards-plus-mailbox throughout."

---

## Part 3 — Encoding a Move

### Types
- `public enum MoveFlag : byte` — `Quiet = 0, DoublePawnPush = 1, KingsideCastle = 2, QueensideCastle = 3, Capture = 4, EnPassant = 5, PromoteKnight = 8, PromoteBishop = 9, PromoteRook = 10, PromoteQueen = 11, PromoCaptureKnight = 12, PromoCaptureBishop = 13, PromoCaptureRook = 14, PromoCaptureQueen = 15` (6 and 7 unused).
- `public readonly struct Move : IEquatable<Move>`
  - private field `private readonly ushort _value;`
  - `public Move(int from, int to, MoveFlag flag)`
  - `public int To => _value & 0x3F;`
  - `public int From => (_value >> 6) & 0x3F;`
  - `public MoveFlag Flag => (MoveFlag)((_value >> 12) & 0x0F);`
  - `public bool IsCapture => ((int)Flag & 0b0100) != 0;`
  - `public bool IsPromotion => ((int)Flag & 0b1000) != 0;`
  - `public bool IsEnPassant => Flag == MoveFlag.EnPassant;`
  - `public bool IsCastle => Flag == MoveFlag.KingsideCastle || Flag == MoveFlag.QueensideCastle;`
  - `public int PromotionPieceIndex => (int)Flag & 0b0011;` (0=N,1=B,2=R,3=Q; only valid when IsPromotion)
  - `public bool Equals(Move other)`, `public override bool Equals(object? o)`, `public override int GetHashCode() => _value;`, `public static bool operator ==(Move a, Move b)`, `public static bool operator !=(Move a, Move b)`
- `public static class MoveIo` — `public static string ToUci(this Move m)`, `static void AppendSquare(StringBuilder sb, int sq)`

### Bit layout of the move value
`_value = (ushort)(((int)flag << 12) | ((from & 0x3F) << 6) | (to & 0x3F));`
- bits 0–5: **to** square
- bits 6–11: **from** square
- bits 12–15: **flag** nibble
- Total 16 bits, fits a `ushort`.

Flag nibble structure: **bit 3 = is promotion**, **bit 2 = is capture**, **bits 1–0 = sub-type / promotion piece**. Promotion sub-codes: `00=N, 01=B, 10=R, 11=Q`.

Flag table (Code | Promo | Cap | S1 S0 | Meaning):
0 Quiet | 1 DoublePawnPush | 2 KingsideCastle | 3 QueensideCastle | 4 Capture | 5 EnPassant | 8 PromoteKnight | 9 PromoteBishop | 10 PromoteRook | 11 PromoteQueen | 12 PromoCaptureKnight | 13 PromoCaptureBishop | 14 PromoCaptureRook | 15 PromoCaptureQueen.

- `IsCapture` is true for straight captures, promo-captures AND en-passant (all set bit 2).
- `IsPromotion` is true for promotions and promo-captures (both set bit 3).
- UCI = from + to + optional promotion letter (`"nbrq"[PromotionPieceIndex]`); no `ep` marker in UCI.
- Masking in the constructor ("guard against sign-extended int") is described as not strictly necessary.

### Deliberately NOT done
- No UCI → `Move` parser: "To disambiguate we'd need the current `Position`. We'll defer that until Part 4 lands a generator… generate all moves and find the one whose `ToUci()` matches."
- No move ordering yet (queen promotion "we'll want to score it highest in move ordering later").

---

## Part 4 — Generating Pseudo-Legal Moves

### Types
- `public static class Bitboards` — constants `FileA = 0x0101010101010101UL`, `FileH = 0x8080808080808080UL`, `NotA = ~FileA`, `NotH = ~FileH`, `Rank1 = 0x00000000000000FFUL`, `Rank2 = 0x000000000000FF00UL`, `Rank4 = 0x00000000FF000000UL`, `Rank5 = 0x000000FF00000000UL`, `Rank7 = 0x00FF000000000000UL`, `Rank8 = 0xFF00000000000000UL`; methods `public static ulong ShiftN(ulong b)`, `ShiftS`, `ShiftE`, `ShiftW`, `ShiftNE`, `ShiftNW`, `ShiftSE`, `ShiftSW` (all `(ulong b) => ulong`).
  - **Missing from the listing but required by the code: `Rank3 = 0x0000000000FF0000UL` and `Rank6 = 0x0000FF0000000000UL`** — the text says "Add `Rank3 = 0x0000000000FF0000UL` and `Rank6 = 0x0000FF0000000000UL` to `Bitboards` if you haven't."
- `public static class Attacks`
  - `public static readonly ulong[] Knight = new ulong[64];`
  - `public static readonly ulong[] King = new ulong[64];`
  - `public static readonly ulong[,] Pawn = new ulong[2, 64];` — [colour, square]; `Pawn[0,sq]` = white-pawn attack set (NE|NW), `Pawn[1,sq]` = black-pawn attack set (SE|SW)
  - `public static readonly ulong[,] Ray = new ulong[8, 64];` — [direction, square], excludes sq itself, runs to board edge
  - `public const int N = 0, NE = 1, E = 2, SE = 3, S = 4, SW = 5, W = 6, NW = 7;`
  - static ctor calls `InitKnight(); InitKing(); InitPawn(); InitRays();`
  - `public static ulong RookAttacks(int sq, ulong occ)`, `public static ulong BishopAttacks(int sq, ulong occ)`, `public static ulong QueenAttacks(int sq, ulong occ)`
  - `public static bool IsAttackedBy(Position pos, int sq, int byColor)` — `byColor`: 0 = white, 1 = black. Pawn lookup uses the **opposite** colour's table: `Pawn[1 - byColor, sq] & pawns`. Cheap pieces first (pawn, knight, king), sliders last.
  - private helpers: `static ulong KnightAttacksOf(ulong knights)`, `static void InitKnight()`, `static ulong KingAttacksOf(ulong k)`, `static void InitKing()`, `static void InitPawn()`, `static void InitRays()`, `static ulong PositiveRay(int sq, int dir, ulong occ)`, `static ulong NegativeRay(int sq, int dir, ulong occ)`
- `public static class MoveGenerator` — `public static int GeneratePseudoLegal(Position pos, Span<Move> dest)`
- Generation helpers (all `static`, in the MoveGenerator file):
  - `static int GenKnights(Position pos, Span<Move> dest, int n, Piece us)`
  - `static int GenKing(Position pos, Span<Move> dest, int n, Piece us)`
  - `static int GenSliders(Position pos, Span<Move> dest, int n, Piece us, bool isBishop)`
  - `static int GenWhitePawns(Position pos, Span<Move> dest, int n)`
  - `static int GenBlackPawns(Position pos, Span<Move> dest, int n)`
  - `static int GenWhiteCastling(Position pos, Span<Move> dest, int n)`
  - `static int GenBlackCastling(Position pos, Span<Move> dest, int n)`
  - `static int EmitTargets(Span<Move> dest, int n, int from, ulong targets, ulong enemy)`
  - `static int EmitPawnTargets(Span<Move> dest, int n, ulong targets, int deltaFrom, MoveFlag flag)`
  - `static int EmitPromotions(Span<Move> dest, int n, ulong targets, int deltaFrom, bool capture)`

### Generation strategy per piece type
- **Buffer contract:** caller owns a `Span<Move>`; each generator appends and returns the new index `n`. "The maximum legal position has 218 moves, so a 256-element buffer is always enough." (`stackalloc Move[256]`).
- **Dispatch order (white):** `GenWhitePawns`, `GenKnights(WhiteKnight)`, `GenSliders(WhiteBishop, isBishop:true)`, `GenSliders(WhiteRook, isBishop:false)`, `GenSliders(WhiteQueen, isBishop:true)`, `GenSliders(WhiteQueen, isBishop:false)`, `GenKing(WhiteKing)`, `GenWhiteCastling`. Black mirrors with Black* pieces.
- **Queens are generated twice** (once as bishop, once as rook) — "We could special-case them but it's not worth the duplication."
- **Captures vs quiets separation:** attacks are masked with `& ~ours`; then `EmitTargets` picks the flag per target: `MoveFlag flag = ((1UL << to) & enemy) != 0 ? MoveFlag.Capture : MoveFlag.Quiet;`.
- **Knights/King:** `Attacks.Knight[from] & ~ours` / `Attacks.King[from] & ~ours`, then `EmitTargets`.
- **Sliders:** `Attacks.BishopAttacks(from, occ)` or `Attacks.RookAttacks(from, occ)` (classical ray-scan), then `& ~ours`, then `EmitTargets`.
  - `PositiveRay` used for N, NE, E, NW (first blocker = LSB, `TrailingZeroCount`); `NegativeRay` for S, SE, W, SW (first blocker = MSB, `63 - LeadingZeroCount`). XOR trick removes the ray beyond the blocker.
- **Pawns (white)** — all four kinds via shifts:
  - singles: `ShiftN(pawns) & empty`; split into `promoSingles = singles & Rank8` and `quietSingles = singles & ~Rank8`.
  - doubles: `ShiftN(singles & Rank3) & empty` (the double shift through rank 3 guarantees the transit square was empty).
  - captures: `capE = ShiftNE(pawns) & enemy`, `capW = ShiftNW(pawns) & enemy`; `& ~Rank8` → quiet capture, `& Rank8` → promo-capture.
  - `deltaFrom` values: push `-8`, double `-16`, east capture `-9`, west capture `-7`; `from = to + deltaFrom`.
- **Pawns (black)** — mirror: `ShiftS`, `Rank1`, `Rank6`; `deltaFrom`: push `+8`, double `+16`, SE capture `+7`, SW capture `+9`.
- **Promotions:** `EmitPromotions` loops `for (int i = 0; i < 4; i++)` (N, B, R, Q) adding `i` to a base flag: `baseFlag = capture ? (int)MoveFlag.PromoCaptureKnight : (int)MoveFlag.PromoteKnight;` → emits flags 8..11 or 12..15.
- **En passant (pseudo-legal level):** if `pos.EpSquare >= 0`, build `epBB = 1UL << pos.EpSquare`; `epE = ShiftNE(pawns) & epBB` (or `ShiftSE` for black), `epW = ShiftNW(pawns) & epBB` (or `ShiftSW`), emitted with `MoveFlag.EnPassant` and the corresponding `deltaFrom`. **No legality check here** — the captured pawn is not on the destination square.
- **Castling (pseudo-legal):** rights intact AND the squares between king and rook empty. Masks: white kingside `0x0000000000000060UL` (f1,g1) → `Move(4, 6, KingsideCastle)`; white queenside `0x000000000000000EUL` (b1,c1,d1) → `Move(4, 2, QueensideCastle)`; black kingside `0x6000000000000000UL` (f8,g8) → `Move(60, 62, KingsideCastle)`; black queenside `0x0E00000000000000UL` (b8,c8,d8) → `Move(60, 58, QueensideCastle)`.
- Knight attack table built by shifting: `l1=(knights>>1)&0x7F7F…`, `l2=(knights>>2)&0x3F3F…`, `r1=(knights<<1)&0xFEFE…`, `r2=(knights<<2)&0xFCFC…`; `h1=l1|r1`, `h2=l2|r2`; result `(h1<<16)|(h1>>16)|(h2<<8)|(h2>>8)`.
- King table: `horiz = ShiftE(k)|ShiftW(k); k |= horiz; return horiz | ShiftN(k) | ShiftS(k);`

### Deliberately NOT done
- **No legality filtering** at all: "Legality comes in Part 7; today we just enumerate." The king being in check is not considered.
- **Castling transit/king-attacked checks are omitted** — "The 'king isn't in check / transit square not attacked' checks come in Part 7."
- Sliders use the **classical ray-scan**, not magic bitboards (Part 6 replaces it).
- No bulk counting, no transposition table, no move ordering.

### Counts asserted
Initial position = exactly `20` pseudo-legal moves; Kiwipete = exactly `48`. "at depth 1 from the initial position pseudo-legal *is* legal (no checks possible)… at depth 1 every white move happens to be legal." ⇒ "counts at depth 1 are a poor differentiator."

---

## Part 5 — Making a Move

### Types
- `public struct UndoInfo` — `public CastlingRights Castling; public int EpSquare; public int HalfmoveClock; public Piece Captured;` (Piece.None for non-captures).
- `public sealed partial class Position` (split into `Position.cs` + `PositionMake.cs`):
  - `internal readonly UndoInfo[] _undoStack = new UndoInfo[1024];`
  - `internal int _undoTop;`
  - `static readonly CastlingRights[] CastlingRightsMask = BuildCastlingMask();`
  - `static CastlingRights[] BuildCastlingMask()`
  - `public void MakeMove(Move move)`
  - `public void UnmakeMove(Move move)`
  - `void RebuildOccupancy()`
  - `void ApplySpecialMoves(Move move, int to, Piece captured, bool whiteMoving)`
  - `void MoveRookForCastle(int rookFrom, int rookTo)`
  - `public void Validate()`
- `public static class Perft` — `public static ulong Run(Position pos, int depth)` (rewritten version below)
- `public static void Divide(Position pos, int depth)` (free function)
- `static bool IsCastleLegal(Position pos, Move m)`

### Make/unmake contract
**Reversible state** (piece bitboards, mailbox, occupancy) — XOR on and off. **Irreversible state** (castling rights, ep square, halfmove clock, captured piece, side to move) — must be saved.

`MakeMove` order (exact steps as listed):
1. Capture `from`, `to`, `moving = Squares[from]`, `whiteMoving = WhiteToMove` (the side doing the move).
2. Determine `captured`: `move.IsEnPassant ? (whiteMoving ? Piece.BlackPawn : Piece.WhitePawn) : Squares[to]`.
3. **Save irreversible state**: push `UndoInfo { Castling, EpSquare, HalfmoveClock, Captured }` onto `_undoStack[_undoTop++]`.
4. **Castling rights**: `Castling &= CastlingRightsMask[from] & CastlingRightsMask[to];` — ANDed for BOTH from and to ("the to-square check is the people miss — if our rook captures the enemy's rook on its home corner, the enemy lost that castling right too").
   - `CastlingRightsMask` is a 64-entry table of rights to **preserve**; default `(CastlingRights)0xF` (no change); `m[0] &= ~WhiteQueenside` (a1), `m[7] &= ~WhiteKingside` (h1), `m[4] &= ~(WhiteKingside|WhiteQueenside)` (e1), `m[56] &= ~BlackQueenside` (a8), `m[63] &= ~BlackKingside` (h8), `m[60] &= ~(BlackKingside|BlackQueenside)` (e8).
5. **Halfmove clock**: reset to 0 if `moving` is a pawn **or** `move.IsCapture`, else `HalfmoveClock++`.
6. **Move the piece**: `ulong fromTo = (1UL << from) | (1UL << to); Pieces[(int)moving] ^= fromTo; Squares[from] = Piece.None; Squares[to] = moving;`
7. **Capture** (default case): `if (move.IsCapture && !move.IsEnPassant) { Pieces[(int)captured] ^= 1UL << to; }` — the mailbox at `to` is already overwritten by the mover.
8. `ApplySpecialMoves(move, to, captured, whiteMoving);` — runs **before** the side flip.
9. **En-passant square**: set only on `MoveFlag.DoublePawnPush` → `whiteMoving ? to - 8 : to + 8`; otherwise `-1`.
10. **Side/fullmove**: `if (!whiteMoving) FullmoveNumber++;` then `WhiteToMove = !whiteMoving;` — **the flip is the last thing touched**. All piece-bitboard mutation happens *before* the flip.
11. `RebuildOccupancy();`

**Special moves (`ApplySpecialMoves`)**, switch on `move.Flag`:
- `MoveFlag.EnPassant`: captured pawn is one rank behind the to-square: `int capSq = whiteMoving ? to - 8 : to + 8;` → `Pieces[(int)captured] ^= 1UL << capSq; Squares[capSq] = Piece.None;`
- `MoveFlag.KingsideCastle`: `MoveRookForCastle(7, 5)` (white h1→f1) / `MoveRookForCastle(63, 61)` (black h8→f8).
- `MoveFlag.QueensideCastle`: `MoveRookForCastle(0, 3)` (white a1→d1) / `MoveRookForCastle(56, 59)` (black a8→d8).
- `default:` if `move.IsPromotion`: swap the pawn on `to` for the promo piece — `Piece promo = (Piece)(colorOffset + 1 + move.PromotionPieceIndex);` with `colorOffset = whiteMoving ? 0 : 6` (piece codes: WhitePawn=0 … WhiteKing=5, BlackPawn=6 … BlackKing=11). `Pieces[pawn] ^= 1UL << to; Pieces[promo] ^= 1UL << to; Squares[to] = promo;`
- `MoveRookForCastle(int rookFrom, int rookTo)`: `Piece rook = Squares[rookFrom]; Pieces[(int)rook] ^= (1UL<<rookFrom)|(1UL<<rookTo); Squares[rookFrom] = Piece.None; Squares[rookTo] = rook;`

**`UnmakeMove` order (mirror):**
1. `WhiteToMove = !WhiteToMove;` **first** (symmetric to make's "flip last"); `if (!WhiteToMove) FullmoveNumber--;` then `bool whiteMoving = WhiteToMove;` (again the side that originally made the move).
2. Pop `UndoInfo u = _undoStack[--_undoTop];`; restore `Castling`, `EpSquare`, `HalfmoveClock` from `u`.
3. If `move.IsPromotion`: swap the promoted piece on `to` back to a pawn (`nowOnTo = pawn`).
4. Move the piece back: `Pieces[(int)nowOnTo] ^= fromTo; Squares[to] = Piece.None; Squares[from] = nowOnTo;`
5. Restore captured piece if `u.Captured != Piece.None`: `capSq = move.IsEnPassant ? (whiteMoving ? to - 8 : to + 8) : to;` → `Pieces[(int)u.Captured] ^= 1UL << capSq; Squares[capSq] = u.Captured;`
6. Undo the castling rook move: kingside `MoveRookForCastle(5, 7)` / `(61, 63)`; queenside `MoveRookForCastle(3, 0)` / `(59, 56)`.
7. `RebuildOccupancy();`

**`RebuildOccupancy()`**: recomputes `WhiteOccupied` as OR of `Pieces[WhitePawn..WhiteKing]`, `BlackOccupied` as OR of `Pieces[BlackPawn..BlackKing]`, `AllOccupied = w | b`. Called at the end of both Make and Unmake. (Bitboards are the source of truth; occupancy is derived, not incrementally updated.)

**Invariant table (page's own):** `_undoStack` push/pop; `Castling` AND-mask / restore; `EpSquare` set/clear / restore; `HalfmoveClock` reset-or-++ / restore; `FullmoveNumber` ++ if black moved / -- if black moved; `WhiteToMove` flip / flip back first; `Pieces[moving]` XOR / XOR; `Squares[from]` None / mover; `Squares[to]` mover / None; capture XOR off / XOR on; castling rook move / move back; ep captured pawn remove / restore; promotion pawn↔promo swap / reverse; occupancy rebuilt / rebuilt.

**`Validate()`**: asserts (a) no two piece bitboards overlap, (b) every set bit in `Pieces[p]` matches `Squares[sq] == p`, (c) mailbox→bitboard agreement both ways, (d) `WhiteOccupied`/`BlackOccupied`/`AllOccupied` equal recomputed unions. Throws `InvalidOperationException`. Intended to be called before and after every Make/Unmake pair in test mode.

### Perft in Part 5 (make-then-test filter, no bulk counting)
```csharp
public static ulong Run(Position pos, int depth)
{
    if (depth == 0) return 1UL;
    Span<Move> buf = stackalloc Move[256];
    int n = MoveGenerator.GeneratePseudoLegal(pos, buf);
    ulong nodes = 0;
    for (int i = 0; i < n; i++)
    {
        Move m = buf[i];
        pos.MakeMove(m);
        int ourKingSq = BitOperations.TrailingZeroCount(
            pos.WhiteToMove ? pos.Pieces[(int)Piece.BlackKing] : pos.Pieces[(int)Piece.WhiteKing]);
        int opponent = pos.WhiteToMove ? 0 : 1;
        bool stillLegal = !Attacks.IsAttackedBy(pos, ourKingSq, opponent);
        if (stillLegal) nodes += Run(pos, depth - 1);
        pos.UnmakeMove(m);
    }
    return nodes;
}
```
Plus an explicit castling guard inside the loop: `if (m.IsCastle && !IsCastleLegal(pos, m)) continue;` where
`IsCastleLegal(Position pos, Move m)` tests `kingFrom = m.From`, `kingTo = m.To`, `transit = (kingFrom + kingTo) / 2`, `opponent = pos.WhiteToMove ? 1 : 0` and returns `!IsAttackedBy(pos, kingFrom, opponent) && !IsAttackedBy(pos, transit, opponent) && !IsAttackedBy(pos, kingTo, opponent)`.

### Deliberately NOT done
- **No Zobrist hash / no hash field / no incremental hash update anywhere.**
- No bulk counting (depth-1 shortcut) — that arrives in Part 7.
- No transposition table, no move ordering, no search/eval/UCI.
- Castling legality is a belt-and-braces filter **outside** the generator ("Part 7 will fold this back into the move generator itself").

---

## Part 6 — Magic Bitboards

### Types / members
- `public static class Magic` (presented as three successive listings of the same class)
  - `static readonly ulong[] RookMasks = new ulong[64];` / `BishopMasks`
  - `static readonly ulong[] RookMagics` / `BishopMagics`
  - `static readonly int[] RookShifts` / `BishopShifts`
  - `static readonly ulong[][] RookTable = new ulong[64][];` / `BishopTable`
  - `static bool _initialised;`
  - `static Magic() => Init();` (static ctor runs on first access)
  - `public static void Init()` — idempotent (`if (_initialised) return;`), calls `InitMasks()`, then per square finds magics, sets shifts, builds tables, sets `_initialised = true`.
  - `static void InitMasks()`
  - `static ulong BuildRookMask(int sq)`
  - `static ulong BuildBishopMask(int sq)`
  - `static ulong FindMagic(int sq, ulong mask, int bits, bool isBishop)`
  - `static ulong[] BuildTable(int sq, ulong mask, ulong magic, int bits, bool isBishop)`
  - `public static ulong RookAttacks(int sq, ulong occ)`
  - `public static ulong BishopAttacks(int sq, ulong occ)`
  - `public static ulong QueenAttacks(int sq, ulong occ)` → `RookAttacks | BishopAttacks`
- `static ulong SparseRandom(System.Random rng)` — `R() => ((ulong)(uint)rng.Next() << 32) | (uint)rng.Next(); return R() & R() & R();`
- `static ulong OccupancyAtIndex(int index, ulong mask)` — enumerates the `2^popcount(mask)` occupancies by treating the index as a bitmask over the mask's set bits.
- Optional precomputed constants: `static readonly ulong[] PrecomputedRookMagics = new ulong[] { 0xa8002c000108020UL, /* a1 */ 0x6c00049b0002001UL, /* b1 */ ... };`

### Search algorithm
- **Index formula:** `index = ((occupancy & mask[sq]) * magic[sq]) >> (64 - bits[sq])`; `attack = attackTable[sq][index]`.
- **Mask construction:** along the piece's rays, excluding the square itself and excluding the outer edges (`rr <= 6`, `ff >= 1` for rook; `rr <= 6 && ff <= 6` etc. for bishop). Edge squares "can never *be* blockers in a meaningful way".
- **Mask popcounts (stated):** corner rooks see 12; central rooks see 10; corner bishops see 6; central bishops see 9. Bishops are easier because their masks are smaller.
- **Magic search:** sparse random candidates (`R() & R() & R()`), seeded `new System.Random(sq + (isBishop ? 1000 : 0))`, up to `100_000_000` attempts. Accept a magic if no two occupancies with *different* reference attack sets collide on the same index (`used[idx] == 0` → store reference; else must match). Throws `System.Exception($"No magic found for sq={sq} bishop={isBishop}")` if none found.
- **Quick-reject heuristic (stated as optional):** `BitOperations.PopCount((mask * magic) & 0xFF00000000000000UL) < 6` → skip. "the high byte of `mask * magic` needs at least 6 set bits, or the index distribution will be terrible. It's optional but skipping it makes the search much slower."
- **Reference attack sets** come from the Part-4 classical ray-scan (`Attacks.RookAttacks` / `Attacks.BishopAttacks`).
- **Table layout:** `RookTable[sq]` and `BishopTable[sq]` are `ulong[]` of length `1 << bits`; `RookShifts[sq] = 64 - rbits`, `BishopShifts[sq] = 64 - bbits`.
- **Caveats stated:** the "spread the top bits" pre-filter is a heuristic; `Magic.Init()` costs roughly 128 magic searches and "takes a fraction of a second" on a recent machine; the classical ray-scan in `Attacks` is deliberately kept ("the magic-finder uses it to compute reference attack sets, and it's a useful fallback if we ever want to double-check a magic implementation"); magics are found at every program start unless pasted in as constants.

### Drop-in changes
- In `MoveGenerator.GenSliders`: `Magic.BishopAttacks(from, occ)` / `Magic.RookAttacks(from, occ)` instead of `Attacks.*`.
- In `Attacks.IsAttackedBy`: `Magic.BishopAttacks(sq, occ) & (bishops|queens)`, `Magic.RookAttacks(sq, occ) & (rooks|queens)`.

### Deliberately NOT done / performance
- "**No new perft numbers** — the move generator still produces the same moves."
- Memory: bishops up to 512 entries/square, rooks up to 4096; **~840 KiB total**, "comfortably within L2 on any modern CPU".
- Speed: "typically **3–5× speedup**, more on hardware with fast 64-bit multiply"; the classical ray-scan cost is "four bitscans and four conditional XORs per slider per square", replaced by "a single `mul + shift + lookup`".
- Startup: one-time cost to find magic constants.

---

## Part 7 — Legal Move Generation

### Types / members
- `public static class Legality`
  - `public static ulong ComputeKingDanger(Position pos, int kingSq, int enemyColor)`
  - `public static ulong ComputeCheckers(Position pos, int kingSq, int enemyColor)`
  - `public static void ComputePins(Position pos, int kingSq, int enemyColor, out ulong pinned, Span<ulong> pinLines)`
  - `static ulong ScanPinners(int kingSq, ulong us, ulong occ, ulong enemySliders, bool isBishop, Span<ulong> pinLines)`
  - `public static readonly ulong[,] SquaresBetween = BuildSquaresBetween();`
  - `static ulong[,] BuildSquaresBetween()` → `ulong[64,64]`; squares strictly between `a` and `b` when on same rank/file/diagonal, else 0.
- `public static class LegalMoveGenerator` — `public static int Generate(Position pos, System.Span<Move> dest)`
- `delegate ulong AttackFn(int sq, ulong occ);`
- `public static class Perft` — `public static ulong Run(Position pos, int depth)` (two versions given: plain, and bulk-counting)
- Generation helpers (static, part of `LegalMoveGenerator` in the listing):
  - `static int GenKnightsLegal(Position pos, System.Span<Move> dest, int n, int us, ulong checkMask, ulong pinned)`
  - `static int GenSlidersLegal(Position pos, System.Span<Move> dest, int n, int us, ulong checkMask, ulong pinned, ulong[] pinLines)`
  - `static int EmitSlider(System.Span<Move> dest, int n, ulong pieces, AttackFn attackFn, ulong ourPieces, ulong enemy, ulong occ, ulong checkMask, ulong pinned, Span<ulong> pinLines)`
  - `static int GenPawnsLegal(Position pos, System.Span<Move> dest, int n, int us, ulong checkMask, ulong pinned, ulong[] pinLines)`
  - `static int EmitPawnLegal(System.Span<Move> dest, int n, ulong targets, int deltaFrom, MoveFlag flag, ulong pinned, ulong[] pinLines)`
  - `static int EmitPromos(System.Span<Move> dest, int n, ulong targets, int deltaFrom, bool capture, ulong pinned, ulong[] pinLines)`
  - `static int GenEnPassant(Position pos, System.Span<Move> dest, int n, int us, int kingSq, ulong checkers)`
  - `static int TryEpMove(Position pos, System.Span<Move> dest, int n, int from, int to, int us, int kingSq, ulong checkers)`
  - `static int GenKingMoves(Position pos, System.Span<Move> dest, int n, int kingSq, ulong kingDanger)`
  - `static int GenCastlingLegal(Position pos, System.Span<Move> dest, int n, int kingSq, ulong kingDanger)`
  - `EmitTargets` is **reused from Part 4** ("`EmitTargets` is the same helper from Part 4").

### The three bitboards
1. `checkers` — enemy pieces currently attacking our king.
2. `pinned` — our pieces that can't move freely; plus per-pinned-piece **pin line** (`pinLines[from]`), the squares it may still move to (including capturing the pinner). `pinLines` is initialised to `ulong.MaxValue` for all 64 entries, so masking by it is a no-op for unpinned pieces.
3. `kingDanger` — squares the enemy attacks **as if our king were not on the board** (occupancy with the king removed), so a slider behind the king is covered.

### Order of operations in `LegalMoveGenerator.Generate`
1. `us = pos.WhiteToMove ? 0 : 1; them = 1 - us;` (0 = white, 1 = black).
2. `kingSq = TrailingZeroCount(pos.Pieces[us == 0 ? Piece.WhiteKing : Piece.BlackKing])`.
3. `kingDanger = Legality.ComputeKingDanger(pos, kingSq, them);`
4. `checkers = Legality.ComputeCheckers(pos, kingSq, them);`
5. `Span<ulong> pinLines = stackalloc ulong[64]; Legality.ComputePins(pos, kingSq, them, out ulong pinned, pinLines);`
6. `n = 0; n = GenKingMoves(pos, dest, n, kingSq, kingDanger);` — king moves are always generated first.
7. `numCheckers = PopCount(checkers); if (numCheckers >= 2) return n;` — **double check: king moves only.**
8. `checkMask = ulong.MaxValue;` and if `numCheckers == 1`: `checkMask = checkers | Legality.SquaresBetween[kingSq, checkerSq];` (checker square for capture ∪ squares between for blocking).
9. `n = GenPawnsLegal(...); n = GenKnightsLegal(...); n = GenSlidersLegal(...);`
10. `if (numCheckers == 0) n = GenCastlingLegal(pos, dest, n, kingSq, kingDanger);` — castling is **not** generated when in check.
11. `return n;`

### Filtering rules per piece
- **King:** `targets = Attacks.King[kingSq] & ~ourPieces & ~kingDanger` → `EmitTargets`.
- **Knights:** pinned knights are removed entirely (`& ~pinned`); attacks = `Attacks.Knight[from] & ~ourPieces & checkMask`.
- **Sliders:** `attacks = attackFn(from, occ) & ~ourPieces & checkMask;` and if the piece is pinned, `attacks &= pinLines[from];` (pin line includes the pinner's square). Queens use `Magic.QueenAttacks`.
- **Pawns:** targets masked with `checkMask`; a move is emitted only if the piece is not pinned, or the target is on its pin line: `if (((1UL << from) & pinned) == 0 || ((1UL << to) & pinLines[from]) != 0)`. Promotions use the same condition.
- **En passant (special):** `GenEnPassant` computes capE/capW from `ShiftNE/ShiftSE` and `ShiftNW/ShiftSW` against `1UL << pos.EpSquare`; `from = ep + dCapE` (dCapE = −9 white / +7 black) or `ep + dCapW` (−7 white / +9 black). `TryEpMove`:
  - `capturedPawnSq = white ? to - 8 : to + 8;`
  - If `numCheckers == 1` and the lone checker is **not** the captured pawn → reject ("removing the pawn doesn't help").
  - `simOcc = pos.AllOccupied ^ (1UL << from) ^ (1UL << to) ^ (1UL << capturedPawnSq);` (removes both pawns and the ep square — the horizontal-pin simulation).
  - `kingAttacked = (Magic.RookAttacks(kingSq, simOcc) & enemyRQ) != 0 || (Magic.BishopAttacks(kingSq, simOcc) & enemyBQ) != 0;` → reject if attacked. Only rook/bishop checks are tested here (the horizontal case).
  - Emits `new Move(from, to, MoveFlag.EnPassant)`.
- **Castling (full legality):** rights present **and** the between-squares empty **and** the king's start/transit/destination squares unattacked:
  - white kingside: `(occ & 0x60UL) == 0` and `(kingDanger & 0x70UL) == 0` (e1,f1,g1) → `Move(4, 6, KingsideCastle)`
  - white queenside: `(occ & 0x0EUL) == 0` and `(kingDanger & 0x1CUL) == 0` (e1,d1,c1) → `Move(4, 2, QueensideCastle)`
  - black kingside: `(occ & 0x6000000000000000UL) == 0` and `(kingDanger & 0x7000000000000000UL) == 0` → `Move(60, 62, KingsideCastle)`
  - black queenside: `(occ & 0x0E00000000000000UL) == 0` and `(kingDanger & 0x1C00000000000000UL) == 0` → `Move(60, 58, QueensideCastle)`
  - Note: occupancy test uses the squares *between* king and rook (excluding the king square); the attack test uses three squares (start, transit, destination).

### Perft in Part 7 (bulk counting)
Plain version:
```csharp
public static ulong Run(Position pos, int depth)
{
    if (depth == 0) return 1UL;
    Span<Move> buf = stackalloc Move[256];
    int n = LegalMoveGenerator.Generate(pos, buf);
    ulong nodes = 0;
    for (int i = 0; i < n; i++)
    {
        pos.MakeMove(buf[i]);
        nodes += Run(pos, depth - 1);
        pos.UnmakeMove(buf[i]);
    }
    return nodes;
}
```
Bulk-counting version (the one the page calls "the bulk-counting trick"):
```csharp
public static ulong Run(Position pos, int depth)
{
    if (depth == 0) return 1UL;
    Span<Move> buf = stackalloc Move[256];
    int n = LegalMoveGenerator.Generate(pos, buf);
    if (depth == 1) return (ulong)n;
    ulong nodes = 0;
    for (int i = 0; i < n; i++)
    {
        pos.MakeMove(buf[i]);
        nodes += Run(pos, depth - 1);
        pos.UnmakeMove(buf[i]);
    }
    return nodes;
}
```
The Part-5 make-then-test filter is **gone**. Bulk counting "roughly doubles speed".

### Deliberately NOT done
- No alpha-beta search, no quiescence, no evaluation function, no UCI interface ("From here you could turn it into a real engine by adding alpha-beta search, quiescence, an evaluation function, and a UCI interface. None of that needs the move generator to change.").
- No transposition tables, no move ordering, no hash.
- No perft `Divide` routine in Part 7 (that was Part 5).
- No Zobrist hashing.

### Compile errors visible in the Part-7 listing
1. **`Span<ulong>` passed where `ulong[]` is expected.** `LegalMoveGenerator.Generate` calls `GenPawnsLegal(pos, dest, n, us, checkMask, pinned, pinLines)` and `GenSlidersLegal(pos, dest, n, us, checkMask, pinned, pinLines)` where `pinLines` is `Span<ulong>` (from `stackalloc ulong[64]`), but both methods declare the last parameter as `ulong[] pinLines`. Same for `EmitPawnLegal` and `EmitPromos` (declared `ulong[] pinLines`) — they are internally consistent with `GenPawnsLegal` but the whole chain is inconsistent with the `Span<ulong>` produced by `ComputePins`/`ScanPinners`.
2. **`GenEnPassant` called with 4 args where 6 are declared.** In `GenPawnsLegal`: `n = GenEnPassant(pos, dest, n, us);` but the declaration is `static int GenEnPassant(Position pos, System.Span<Move> dest, int n, int us, int kingSq, ulong checkers)`. `kingSq` and `checkers` are therefore not supplied.
3. **Duplicate class declarations** (CS0101) in the listings: `public static class Legality` appears twice in Part 7 (the `ComputeKingDanger`/`ComputeCheckers`/`ComputePins` listing and the `SquaresBetween` listing) unless declared `partial`; `public static class Fen` appears twice in Part 2 (Parse / Write); `public static class Magic` appears three times in Part 6; `public static class Attacks` / `MoveGenerator` in Part 4 are shown as a class wrapper plus separate free-function listings that must be placed inside the class.
4. **Missing constants:** `Bitboards.Rank3` and `Bitboards.Rank6` are used by `GenWhitePawns`/`GenBlackPawns` (Part 4) and by `GenPawnsLegal` (Part 7) but are absent from the Part-4 `Bitboards` listing (the text tells the reader to add them). (`Bitboards.Rank1` *is* declared and is used by Part 4's black pawns and Part 7's `GenPawnsLegal`.)
5. **Cross-class helper visibility:** `EmitTargets` is defined with the Part-4 `MoveGenerator` helpers but called from `LegalMoveGenerator` (Part 7); as printed it is not a member of any shown class, so it must live in a shared static class (or be made accessible).
6. **Member-resolution risks:** `Attacks.IsAttackedBy` uses unqualified `Pawn`, `Knight`, `King`, `BishopAttacks`, `RookAttacks` and `Attacks`'s static ctor calls `InitKnight()`/`InitKing()`/`InitPawn()`/`InitRays()` — all of these must be members of `Attacks`. `Legality.ScanPinners` references `SquaresBetween` unqualified while `SquaresBetween` is declared in the second `Legality` listing — again requires one merged/partial class.
7. **Unused parameter:** `GenCastlingLegal(..., int kingSq, ulong kingDanger)` never uses `kingSq` (it hard-codes 4/6/2/60/62/58) — a warning, not an error.
8. **Part-1/Part-4 API drift:** Part 1's `Perft.Run` calls `MoveGenerator.GenerateMoves(position)` returning `Move[]`, and Part 1's `PositionExtensions.MakeMove/UnmakeMove` are extension methods; Part 4/5 replace these with `MoveGenerator.GeneratePseudoLegal(pos, Span<Move>)` and instance methods `Position.MakeMove/UnmakeMove`. The Part-1 stubs must be deleted, not kept alongside.

### EXACT final Part-7 implementation listing (verbatim)

```csharp
public static class Legality
{
    public static ulong ComputeKingDanger(Position pos, int kingSq, int enemyColor)
    {
        ulong occWithoutKing = pos.AllOccupied ^ (1UL << kingSq);

        ulong danger = 0;

        // Pawns
        ulong epawns = pos.Pieces[(int)(enemyColor == 0 ? Piece.WhitePawn : Piece.BlackPawn)];
        while (epawns != 0)
        {
            int sq = BitOperations.TrailingZeroCount(epawns);
            danger |= Attacks.Pawn[enemyColor, sq];
            epawns &= epawns - 1;
        }

        // Knights
        ulong knights = pos.Pieces[(int)(enemyColor == 0 ? Piece.WhiteKnight : Piece.BlackKnight)];
        while (knights != 0)
        {
            int sq = BitOperations.TrailingZeroCount(knights);
            danger |= Attacks.Knight[sq];
            knights &= knights - 1;
        }

        // Bishops + queens (diagonal)
        ulong bq = pos.Pieces[(int)(enemyColor == 0 ? Piece.WhiteBishop : Piece.BlackBishop)]
                 | pos.Pieces[(int)(enemyColor == 0 ? Piece.WhiteQueen  : Piece.BlackQueen)];
        while (bq != 0)
        {
            int sq = BitOperations.TrailingZeroCount(bq);
            danger |= Magic.BishopAttacks(sq, occWithoutKing);
            bq &= bq - 1;
        }

        // Rooks + queens (orthogonal)
        ulong rq = pos.Pieces[(int)(enemyColor == 0 ? Piece.WhiteRook  : Piece.BlackRook)]
                 | pos.Pieces[(int)(enemyColor == 0 ? Piece.WhiteQueen : Piece.BlackQueen)];
        while (rq != 0)
        {
            int sq = BitOperations.TrailingZeroCount(rq);
            danger |= Magic.RookAttacks(sq, occWithoutKing);
            rq &= rq - 1;
        }

        // Enemy king
        int eksq = BitOperations.TrailingZeroCount(
            pos.Pieces[(int)(enemyColor == 0 ? Piece.WhiteKing : Piece.BlackKing)]);
        danger |= Attacks.King[eksq];

        return danger;
    }
}
```
```csharp
public static ulong ComputeCheckers(Position pos, int kingSq, int enemyColor)
{
    ulong occ = pos.AllOccupied;

    ulong attackerPawns   = pos.Pieces[(int)(enemyColor == 0 ? Piece.WhitePawn   : Piece.BlackPawn)];
    ulong attackerKnights = pos.Pieces[(int)(enemyColor == 0 ? Piece.WhiteKnight : Piece.BlackKnight)];
    ulong attackerBQ      = pos.Pieces[(int)(enemyColor == 0 ? Piece.WhiteBishop : Piece.BlackBishop)]
                          | pos.Pieces[(int)(enemyColor == 0 ? Piece.WhiteQueen  : Piece.BlackQueen)];
    ulong attackerRQ      = pos.Pieces[(int)(enemyColor == 0 ? Piece.WhiteRook   : Piece.BlackRook)]
                          | pos.Pieces[(int)(enemyColor == 0 ? Piece.WhiteQueen  : Piece.BlackQueen)];

    return  (Attacks.Pawn[1 - enemyColor, kingSq] & attackerPawns)
          | (Attacks.Knight[kingSq]               & attackerKnights)
          | (Magic.BishopAttacks(kingSq, occ)     & attackerBQ)
          | (Magic.RookAttacks  (kingSq, occ)     & attackerRQ);
}
```
```csharp
public static void ComputePins(Position pos, int kingSq, int enemyColor,
                               out ulong pinned, Span<ulong> pinLines)
{
    for (int i = 0; i < 64; i++) pinLines[i] = ulong.MaxValue;
    pinned = 0;

    ulong us = pos.WhiteToMove ? pos.WhiteOccupied : pos.BlackOccupied;
    ulong occ = pos.AllOccupied;

    ulong enemyRQ = pos.Pieces[(int)(enemyColor == 0 ? Piece.WhiteRook  : Piece.BlackRook)]
                  | pos.Pieces[(int)(enemyColor == 0 ? Piece.WhiteQueen : Piece.BlackQueen)];
    ulong enemyBQ = pos.Pieces[(int)(enemyColor == 0 ? Piece.WhiteBishop : Piece.BlackBishop)]
                  | pos.Pieces[(int)(enemyColor == 0 ? Piece.WhiteQueen : Piece.BlackQueen)];

    pinned |= ScanPinners(kingSq, us, occ, enemyRQ, isBishop: false, pinLines);
    pinned |= ScanPinners(kingSq, us, occ, enemyBQ, isBishop: true,  pinLines);
}

static ulong ScanPinners(int kingSq, ulong us, ulong occ, ulong enemySliders,
                         bool isBishop, Span<ulong> pinLines)
{
    ulong directAttacks = isBishop
        ? Magic.BishopAttacks(kingSq, occ)
        : Magic.RookAttacks  (kingSq, occ);

    ulong blockers = directAttacks & us;         // our blockers on the slider's rays
    ulong throughOccupancy = occ ^ blockers;     // remove them

    ulong xrayAttacks = isBishop
        ? Magic.BishopAttacks(kingSq, throughOccupancy)
        : Magic.RookAttacks  (kingSq, throughOccupancy);

    ulong xrayPinners = xrayAttacks & enemySliders & ~directAttacks;

    ulong pinned = 0;
    while (xrayPinners != 0)
    {
        int pinnerSq = BitOperations.TrailingZeroCount(xrayPinners);
        ulong between = SquaresBetween[kingSq, pinnerSq];
        ulong ourBlocker = between & us;
        // X-rays were constructed so exactly one of our pieces sits between king & pinner.
        if (ourBlocker != 0 && BitOperations.PopCount(ourBlocker) == 1)
        {
            int pinnedSq = BitOperations.TrailingZeroCount(ourBlocker);
            pinned |= ourBlocker;
            pinLines[pinnedSq] = between | (1UL << pinnerSq);
        }
        xrayPinners &= xrayPinners - 1;
    }
    return pinned;
}
```
```csharp
public static class Legality
{
    public static readonly ulong[,] SquaresBetween = BuildSquaresBetween();

    static ulong[,] BuildSquaresBetween()
    {
        var tbl = new ulong[64, 64];
        for (int a = 0; a < 64; a++)
            for (int b = 0; b < 64; b++)
            {
                if (a == b) continue;
                int ra = a >> 3, fa = a & 7;
                int rb = b >> 3, fb = b & 7;
                int dr = System.Math.Sign(rb - ra);
                int df = System.Math.Sign(fb - fa);
                bool aligned = ra == rb || fa == fb
                            || System.Math.Abs(ra - rb) == System.Math.Abs(fa - fb);
                if (!aligned) continue;

                ulong bb = 0;
                int r = ra + dr, f = fa + df;
                while (r != rb || f != fb)
                {
                    bb |= 1UL << (r * 8 + f);
                    r += dr; f += df;
                }
                tbl[a, b] = bb;
            }
        return tbl;
    }
}
```
```csharp
public static class LegalMoveGenerator
{
    public static int Generate(Position pos, System.Span<Move> dest)
    {
        int us       = pos.WhiteToMove ? 0 : 1;
        int them     = 1 - us;
        int kingSq   = BitOperations.TrailingZeroCount(
            pos.Pieces[(int)(us == 0 ? Piece.WhiteKing : Piece.BlackKing)]);

        ulong kingDanger = Legality.ComputeKingDanger(pos, kingSq, them);
        ulong checkers   = Legality.ComputeCheckers  (pos, kingSq, them);

        Span<ulong> pinLines = stackalloc ulong[64];
        Legality.ComputePins(pos, kingSq, them, out ulong pinned, pinLines);

        int n = 0;

        // King moves: always allowed, just avoid danger and own pieces.
        n = GenKingMoves(pos, dest, n, kingSq, kingDanger);

        int numCheckers = BitOperations.PopCount(checkers);
        if (numCheckers >= 2)
            return n;   // double check — king moves only

        // checkMask: squares a non-king piece is allowed to *land* on.
        //   not in check       → everywhere
        //   single check       → checker's square (for capture) ∪ squares between king and checker (for block)
        ulong checkMask = ulong.MaxValue;
        if (numCheckers == 1)
        {
            int checkerSq = BitOperations.TrailingZeroCount(checkers);
            checkMask = checkers | Legality.SquaresBetween[kingSq, checkerSq];
        }

        n = GenPawnsLegal  (pos, dest, n, us, checkMask, pinned, pinLines);
        n = GenKnightsLegal(pos, dest, n, us, checkMask, pinned);
        n = GenSlidersLegal(pos, dest, n, us, checkMask, pinned, pinLines);

        if (numCheckers == 0)
            n = GenCastlingLegal(pos, dest, n, kingSq, kingDanger);

        return n;
    }
}
```
```csharp
static int GenKnightsLegal(Position pos, System.Span<Move> dest, int n,
                           int us, ulong checkMask, ulong pinned)
{
    ulong ourPieces = us == 0 ? pos.WhiteOccupied : pos.BlackOccupied;
    ulong enemy     = us == 0 ? pos.BlackOccupied : pos.WhiteOccupied;

    ulong knights = pos.Pieces[(int)(us == 0 ? Piece.WhiteKnight : Piece.BlackKnight)] & ~pinned;
    while (knights != 0)
    {
        int from = BitOperations.TrailingZeroCount(knights);
        ulong attacks = Attacks.Knight[from] & ~ourPieces & checkMask;
        n = EmitTargets(dest, n, from, attacks, enemy);
        knights &= knights - 1;
    }
    return n;
}
```
```csharp
static int GenSlidersLegal(Position pos, System.Span<Move> dest, int n,
                           int us, ulong checkMask, ulong pinned, ulong[] pinLines)
{
    ulong ourPieces = us == 0 ? pos.WhiteOccupied : pos.BlackOccupied;
    ulong enemy     = us == 0 ? pos.BlackOccupied : pos.WhiteOccupied;
    ulong occ = pos.AllOccupied;

    Piece bishop = us == 0 ? Piece.WhiteBishop : Piece.BlackBishop;
    Piece rook   = us == 0 ? Piece.WhiteRook   : Piece.BlackRook;
    Piece queen  = us == 0 ? Piece.WhiteQueen  : Piece.BlackQueen;

    n = EmitSlider(dest, n, pos.Pieces[(int)bishop], Magic.BishopAttacks,
                   ourPieces, enemy, occ, checkMask, pinned, pinLines);
    n = EmitSlider(dest, n, pos.Pieces[(int)rook],   Magic.RookAttacks,
                   ourPieces, enemy, occ, checkMask, pinned, pinLines);
    n = EmitSlider(dest, n, pos.Pieces[(int)queen],  Magic.QueenAttacks,
                   ourPieces, enemy, occ, checkMask, pinned, pinLines);
    return n;
}

delegate ulong AttackFn(int sq, ulong occ);

static int EmitSlider(System.Span<Move> dest, int n, ulong pieces, AttackFn attackFn,
                      ulong ourPieces, ulong enemy, ulong occ,
                      ulong checkMask, ulong pinned, Span<ulong> pinLines)
{
    while (pieces != 0)
    {
        int from = BitOperations.TrailingZeroCount(pieces);
        ulong attacks = attackFn(from, occ) & ~ourPieces & checkMask;
        if (((1UL << from) & pinned) != 0)
            attacks &= pinLines[from];      // pinned: restrict to pin line
        n = EmitTargets(dest, n, from, attacks, enemy);
        pieces &= pieces - 1;
    }
    return n;
}
```
```csharp
static int GenPawnsLegal(Position pos, System.Span<Move> dest, int n,
                         int us, ulong checkMask, ulong pinned, ulong[] pinLines)
{
    bool white = us == 0;
    ulong pawns = pos.Pieces[(int)(white ? Piece.WhitePawn : Piece.BlackPawn)];
    ulong empty = ~pos.AllOccupied;
    ulong enemy = white ? pos.BlackOccupied : pos.WhiteOccupied;

    int dPush = white ? -8 : +8;
    int dCapE = white ? -9 : +7;
    int dCapW = white ? -7 : +9;
    ulong promoRank = white ? Bitboards.Rank8 : Bitboards.Rank1;
    ulong startRank3 = white ? Bitboards.Rank3 : Bitboards.Rank6;

    ulong singles = (white ? Bitboards.ShiftN(pawns) : Bitboards.ShiftS(pawns)) & empty;
    ulong doubles = (white ? Bitboards.ShiftN(singles & startRank3) : Bitboards.ShiftS(singles & startRank3)) & empty;
    ulong capE    = (white ? Bitboards.ShiftNE(pawns) : Bitboards.ShiftSE(pawns)) & enemy;
    ulong capW    = (white ? Bitboards.ShiftNW(pawns) : Bitboards.ShiftSW(pawns)) & enemy;

    // Apply check mask: if in check, pushes must land on a blocking square and
    // captures must take the checker (which the same mask expresses).
    singles &= checkMask;
    doubles &= checkMask;
    capE    &= checkMask;
    capW    &= checkMask;

    n = EmitPawnLegal(dest, n, singles & ~promoRank, dPush, MoveFlag.Quiet,           pinned, pinLines);
    n = EmitPromos   (dest, n, singles &  promoRank, dPush, capture: false,           pinned, pinLines);
    n = EmitPawnLegal(dest, n, doubles,              2 * dPush, MoveFlag.DoublePawnPush, pinned, pinLines);
    n = EmitPawnLegal(dest, n, capE & ~promoRank,    dCapE, MoveFlag.Capture,         pinned, pinLines);
    n = EmitPawnLegal(dest, n, capW & ~promoRank,    dCapW, MoveFlag.Capture,         pinned, pinLines);
    n = EmitPromos   (dest, n, capE &  promoRank,    dCapE, capture: true,            pinned, pinLines);
    n = EmitPromos   (dest, n, capW &  promoRank,    dCapW, capture: true,            pinned, pinLines);

    // En passant — special legality check
    if (pos.EpSquare >= 0)
    {
        n = GenEnPassant(pos, dest, n, us);
    }

    return n;
}

static int EmitPawnLegal(System.Span<Move> dest, int n, ulong targets, int deltaFrom,
                         MoveFlag flag, ulong pinned, ulong[] pinLines)
{
    while (targets != 0)
    {
        int to = BitOperations.TrailingZeroCount(targets);
        int from = to + deltaFrom;
        if (((1UL << from) & pinned) == 0 || ((1UL << to) & pinLines[from]) != 0)
            dest[n++] = new Move(from, to, flag);
        targets &= targets - 1;
    }
    return n;
}

static int EmitPromos(System.Span<Move> dest, int n, ulong targets, int deltaFrom,
                      bool capture, ulong pinned, ulong[] pinLines)
{
    int baseFlag = capture ? (int)MoveFlag.PromoCaptureKnight : (int)MoveFlag.PromoteKnight;
    while (targets != 0)
    {
        int to = BitOperations.TrailingZeroCount(targets);
        int from = to + deltaFrom;
        if (((1UL << from) & pinned) == 0 || ((1UL << to) & pinLines[from]) != 0)
            for (int i = 0; i < 4; i++)
                dest[n++] = new Move(from, to, (MoveFlag)(baseFlag + i));
        targets &= targets - 1;
    }
    return n;
}
```
```csharp
static int GenEnPassant(Position pos, System.Span<Move> dest, int n,
                        int us, int kingSq, ulong checkers)
{
    bool white = us == 0;
    int ep = pos.EpSquare;
    ulong epBB = 1UL << ep;

    ulong pawns = pos.Pieces[(int)(white ? Piece.WhitePawn : Piece.BlackPawn)];
    ulong capE = (white ? Bitboards.ShiftNE(pawns) : Bitboards.ShiftSE(pawns)) & epBB;
    ulong capW = (white ? Bitboards.ShiftNW(pawns) : Bitboards.ShiftSW(pawns)) & epBB;

    int dCapE = white ? -9 : +7;
    int dCapW = white ? -7 : +9;

    if (capE != 0) n = TryEpMove(pos, dest, n, ep + dCapE, ep, us, kingSq, checkers);
    if (capW != 0) n = TryEpMove(pos, dest, n, ep + dCapW, ep, us, kingSq, checkers);
    return n;
}

static int TryEpMove(Position pos, System.Span<Move> dest, int n,
                     int from, int to, int us, int kingSq, ulong checkers)
{
    bool white = us == 0;
    int capturedPawnSq = white ? to - 8 : to + 8;

    // If we're in single check, the ep capture is only legal when it removes
    // the lone checker.  Double check was filtered out before we got here.
    int numCheckers = BitOperations.PopCount(checkers);
    if (numCheckers == 1 && (checkers & (1UL << capturedPawnSq)) == 0)
        return n;

    ulong simOcc = pos.AllOccupied
                 ^ (1UL << from)
                 ^ (1UL << to)
                 ^ (1UL << capturedPawnSq);

    int them = 1 - us;
    ulong enemyRQ = pos.Pieces[(int)(them == 0 ? Piece.WhiteRook  : Piece.BlackRook)]
                  | pos.Pieces[(int)(them == 0 ? Piece.WhiteQueen : Piece.BlackQueen)];
    ulong enemyBQ = pos.Pieces[(int)(them == 0 ? Piece.WhiteBishop : Piece.BlackBishop)]
                  | pos.Pieces[(int)(them == 0 ? Piece.WhiteQueen : Piece.BlackQueen)];

    bool kingAttacked =
           (Magic.RookAttacks  (kingSq, simOcc) & enemyRQ) != 0
        || (Magic.BishopAttacks(kingSq, simOcc) & enemyBQ) != 0;

    if (!kingAttacked)
        dest[n++] = new Move(from, to, MoveFlag.EnPassant);
    return n;
}
```
```csharp
static int GenKingMoves(Position pos, System.Span<Move> dest, int n,
                        int kingSq, ulong kingDanger)
{
    bool white = pos.WhiteToMove;
    ulong ourPieces = white ? pos.WhiteOccupied : pos.BlackOccupied;
    ulong enemy     = white ? pos.BlackOccupied : pos.WhiteOccupied;
    ulong targets = Attacks.King[kingSq] & ~ourPieces & ~kingDanger;
    return EmitTargets(dest, n, kingSq, targets, enemy);
}

static int GenCastlingLegal(Position pos, System.Span<Move> dest, int n,
                            int kingSq, ulong kingDanger)
{
    bool white = pos.WhiteToMove;
    ulong occ = pos.AllOccupied;

    if (white)
    {
        if ((pos.Castling & CastlingRights.WhiteKingside) != 0
            && (occ & 0x60UL) == 0
            && (kingDanger & 0x70UL) == 0)       // e1, f1, g1 unattacked
            dest[n++] = new Move(4, 6, MoveFlag.KingsideCastle);
        if ((pos.Castling & CastlingRights.WhiteQueenside) != 0
            && (occ & 0x0EUL) == 0
            && (kingDanger & 0x1CUL) == 0)       // e1, d1, c1 unattacked
            dest[n++] = new Move(4, 2, MoveFlag.QueensideCastle);
    }
    else
    {
        if ((pos.Castling & CastlingRights.BlackKingside) != 0
            && (occ & 0x6000000000000000UL) == 0
            && (kingDanger & 0x7000000000000000UL) == 0)
            dest[n++] = new Move(60, 62, MoveFlag.KingsideCastle);
        if ((pos.Castling & CastlingRights.BlackQueenside) != 0
            && (occ & 0x0E00000000000000UL) == 0
            && (kingDanger & 0x1C00000000000000UL) == 0)
            dest[n++] = new Move(60, 58, MoveFlag.QueensideCastle);
    }
    return n;
}
```
```csharp
public static class Perft
{
    public static ulong Run(Position pos, int depth)
    {
        if (depth == 0) return 1UL;
        Span<Move> buf = stackalloc Move[256];
        int n = LegalMoveGenerator.Generate(pos, buf);
        ulong nodes = 0;
        for (int i = 0; i < n; i++)
        {
            pos.MakeMove(buf[i]);
            nodes += Run(pos, depth - 1);
            pos.UnmakeMove(buf[i]);
        }
        return nodes;
    }
}
```
```csharp
public static ulong Run(Position pos, int depth)
{
    if (depth == 0) return 1UL;
    Span<Move> buf = stackalloc Move[256];
    int n = LegalMoveGenerator.Generate(pos, buf);
    if (depth == 1) return (ulong)n;
    ulong nodes = 0;
    for (int i = 0; i < n; i++)
    {
        pos.MakeMove(buf[i]);
        nodes += Run(pos, depth - 1);
        pos.UnmakeMove(buf[i]);
    }
    return nodes;
}
```
```csharp
internal class Program
{
    static void Main()
    {
        Magic.Init();

        var pos = Fen.Parse(Fen.Initial);
        for (int d = 1; d <= 6; d++)
        {
            var sw = System.Diagnostics.Stopwatch.StartNew();
            ulong nodes = Perft.Run(pos, d);
            sw.Stop();
            System.Console.WriteLine($"perft({d}) = {nodes,12}   {sw.Elapsed.TotalSeconds,7:F2}s");
        }

        System.Console.WriteLine();

        var kp = Fen.Parse(Fen.Kiwipete);
        for (int d = 1; d <= 5; d++)
        {
            var sw = System.Diagnostics.Stopwatch.StartNew();
            ulong nodes = Perft.Run(kp, d);
            sw.Stop();
            System.Console.WriteLine($"kiwipete({d}) = {nodes,12}   {sw.Elapsed.TotalSeconds,7:F2}s");
        }
    }
}
```

---

## Perft numbers asserted by the series (summary)

| Position | Depth 1 | 2 | 3 | 4 | 5 | 6 |
|---|---|---|---|---|---|---|
| Initial (`rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1`) | 20 | 400 | 8,902 | 197,281 | 4,865,609 | 119,060,324 |
| Kiwipete (`r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq -`) | 48 | 2,039 | 97,862 | 4,085,603 | 193,690,690 | — |

Perft(0) = 1 (Part 1). Part 1 asserts depth 0 = 1 explicitly.

**Perft loop shapes by part:**
- Part 1: `Move[] moves = MoveGenerator.GenerateMoves(position)` — no bulk counting.
- Part 5: `Span<Move> buf = stackalloc Move[256]` + make-then-test-king-attacked filter + explicit `IsCastleLegal` guard — no bulk counting. `Divide(Position pos, int depth)` prints per-root-move subtotals and uses `depth == 1 ? 1 : Run(pos, depth - 1)`.
- Part 7: `LegalMoveGenerator.Generate(pos, buf)` + optional bulk counting `if (depth == 1) return (ulong)n;`.

**Performance claims with numbers:**
- Part 4: classical ray-scan = "up to four bitscans per slider"; "four bitscans and four conditional XORs per slider per square" (Part 5/6 wording).
- Part 5: "Depth 5 on the initial position will take a few seconds; Kiwipete depth 4 will take longer still."
- Part 6: magic lookup = "a single `mul + shift + lookup`"; "typically **3–5× speedup**, more on hardware with fast 64-bit multiply"; tables ≈ **840 KiB** total (bishops ≤512 entries/square, rooks ≤4096); "comfortably within L2 on any modern CPU"; `Magic.Init()` = "roughly 128 magic searches", "fraction of a second"; mask popcounts: corner rook 12, central rook 10, corner bishop 6, central bishop 9; a rook on a corner has 49 unique attack sets over 4096 occupancies, a bishop on d4 has 108 over 512.
- Part 7: bulk counting "roughly doubles speed".
- Part 4: buffer 256 elements covers the 218-move maximum legal position.

---

## Cross-part API summary

| Name | Kind | Signature | Part | Notes |
|---|---|---|---|---|
| `FenBoard` | static class | — | 1 | `Render(string fen)`; FEN placement only |
| `FenBoard.Render` | method | `public static string Render(string fen)` | 1 | ASCII board printer |
| `Perft` | static class | — | 1, 5, 7 | `Run` re-declared 3 times across parts |
| `Perft.Run` | method | `public static ulong Run(Position position, int depth)` | 1 | Part 1 signature (param name `position`) |
| `Perft.Run` | method | `public static ulong Run(Position pos, int depth)` | 5, 7 | Part 5 = pseudo-legal + make-then-test; Part 7 = legal generator (+ bulk count) |
| `Position` | sealed class | — | 1(stub), 2, 5 | Becomes `public sealed partial class Position` in Part 5 |
| `Position.Pieces` | field | `public ulong[] Pieces` (len 12) | 2 | Indexed by `(int)Piece` 0..11; source of truth |
| `Position.Squares` | field | `public Piece[] Squares` (len 64) | 2 | 8×8 mailbox cache |
| `Position.WhiteOccupied` | field | `public ulong` | 2 | Cached aggregate |
| `Position.BlackOccupied` | field | `public ulong` | 2 | Cached aggregate |
| `Position.AllOccupied` | field | `public ulong` | 2 | Cached aggregate |
| `Position.WhiteToMove` | field | `public bool` | 2 | |
| `Position.Castling` | field | `public CastlingRights` | 2 | |
| `Position.EpSquare` | field | `public int` | 2 | `-1` if none |
| `Position.HalfmoveClock` | field | `public int` | 2 | |
| `Position.FullmoveNumber` | field | `public int` | 2 | |
| `Position._undoStack` | field | `internal readonly UndoInfo[]` (1024) | 5 | Undo stack |
| `Position._undoTop` | field | `internal int` | 5 | Undo stack cursor |
| `Position.CastlingRightsMask` | static field | `static readonly CastlingRights[]` (64) | 5 | Rights to *preserve* per square |
| `Position.MakeMove` | method | `public void MakeMove(Move move)` | 5 | Instance method (Part 1 stub was an extension) |
| `Position.UnmakeMove` | method | `public void UnmakeMove(Move move)` | 5 | |
| `Position.Validate` | method | `public void Validate()` | 5 | Consistency assertions |
| `Position.RebuildOccupancy` | method | `void RebuildOccupancy()` | 5 | Recomputes the 3 aggregates |
| `Position.ApplySpecialMoves` | method | `void ApplySpecialMoves(Move move, int to, Piece captured, bool whiteMoving)` | 5 | ep / castle / promo |
| `Position.MoveRookForCastle` | method | `void MoveRookForCastle(int rookFrom, int rookTo)` | 5 | |
| `Position.BuildCastlingMask` | method | `static CastlingRights[] BuildCastlingMask()` | 5 | |
| `Move` | readonly struct | `: IEquatable<Move>` | 1(stub), 3 | 16-bit packed value |
| `Move._value` | field | `private readonly ushort _value` | 3 | `flag<<12 \| from<<6 \| to` |
| `Move..ctor` | ctor | `public Move(int from, int to, MoveFlag flag)` | 3 | |
| `Move.To` | property | `public int` | 3 | `_value & 0x3F` |
| `Move.From` | property | `public int` | 3 | `(_value>>6) & 0x3F` |
| `Move.Flag` | property | `public MoveFlag` | 3 | `(_value>>12) & 0x0F` |
| `Move.IsCapture` | property | `public bool` | 3 | flag bit 2 |
| `Move.IsPromotion` | property | `public bool` | 3 | flag bit 3 |
| `Move.IsEnPassant` | property | `public bool` | 3 | |
| `Move.IsCastle` | property | `public bool` | 3 | |
| `Move.PromotionPieceIndex` | property | `public int` | 3 | 0=N,1=B,2=R,3=Q |
| `Move.Equals` | method | `public bool Equals(Move other)` | 3 | |
| `Move.GetHashCode` | method | `public override int GetHashCode()` | 3 | returns `_value` |
| `Move.operator ==` | operator | `public static bool operator ==(Move a, Move b)` | 3 | |
| `Move.operator !=` | operator | `public static bool operator !=(Move a, Move b)` | 3 | |
| `MoveFlag` | enum | `: byte` | 3 | 14 members, values 0,1,2,3,4,5,8..15 |
| `Piece` | enum | `: byte` | 2 | 13 members, 0..12 |
| `CastlingRights` | enum | `: byte`, `[Flags]` | 2 | 0,1,2,4,8 |
| `Fen` | static class | — | 2 | Declared twice in the page (Parse / Write) |
| `Fen.Initial` | const | `public const string` | 2 | Initial FEN with `0 1` |
| `Fen.Kiwipete` | const | `public const string` | 2 | Kiwipete FEN, no trailing clocks |
| `Fen.Parse` | method | `public static Position Parse(string fen)` | 2 | |
| `Fen.Write` | method | `public static string Write(Position pos)` | 2 | |
| `Fen.PieceFromChar` | method | `static Piece PieceFromChar(char c)` | 2 | |
| `Fen.SquareFromName` | method | `static int SquareFromName(string s)` | 2 | |
| `Fen.CharOf` | method | `static char CharOf(Piece p)` | 2 | |
| `Fen.SquareName` | method | `static string SquareName(int sq)` | 2 | |
| `Fen.CastlingString` | method | `static string CastlingString(CastlingRights c)` | 2 | |
| `PositionPrinter` | static class | — | 2 | |
| `PositionPrinter.Render` | method | `public static string Render(Position pos)` | 2 | Full board + state |
| `PositionPrinter.SquareName` | method | `static string SquareName(int sq)` | 2 | |
| `Square`/`FileOf`/`RankOf` | functions | `int Square(int file, int rank)`, `int FileOf(int sq)`, `int RankOf(int sq)` | 2 | LERF helpers |
| `MoveIo` | static class | — | 3 | |
| `MoveIo.ToUci` | method | `public static string ToUci(this Move m)` | 3 | Extension method |
| `MoveIo.AppendSquare` | method | `static void AppendSquare(StringBuilder sb, int sq)` | 3 | |
| `Bitboards` | static class | — | 4, 7 | |
| `Bitboards.FileA/FileH/NotA/NotH` | const | `public const ulong` | 4 | `0x0101010101010101`, `0x8080808080808080` |
| `Bitboards.Rank1/Rank2/Rank4/Rank5/Rank7/Rank8` | const | `public const ulong` | 4 | `0xFF`, `0xFF00`, `0xFF000000`, `0xFF00000000`, `0xFF000000000000`, `0xFF00000000000000` |
| `Bitboards.Rank3` / `Rank6` | const | `public const ulong` | 4, 7 | **Not in the listing** — text says to add: `0x0000000000FF0000UL` / `0x0000FF0000000000UL` |
| `Bitboards.ShiftN/ShiftS/ShiftE/ShiftW/ShiftNE/ShiftNW/ShiftSE/ShiftSW` | method | `public static ulong ShiftX(ulong b)` | 4 | |
| `Attacks` | static class | — | 4, 6, 7 | |
| `Attacks.Knight` | static field | `public static readonly ulong[]` (64) | 4 | |
| `Attacks.King` | static field | `public static readonly ulong[]` (64) | 4 | |
| `Attacks.Pawn` | static field | `public static readonly ulong[,]` (2×64) | 4 | `[colour, square]` |
| `Attacks.Ray` | static field | `public static readonly ulong[,]` (8×64) | 4 | `[direction, square]` |
| `Attacks.N/NE/E/SE/S/SW/W/NW` | const | `public const int` | 4 | 0,1,2,3,4,5,6,7 |
| `Attacks.RookAttacks` | method | `public static ulong RookAttacks(int sq, ulong occ)` | 4 | classical (kept as reference) |
| `Attacks.BishopAttacks` | method | `public static ulong BishopAttacks(int sq, ulong occ)` | 4 | classical |
| `Attacks.QueenAttacks` | method | `public static ulong QueenAttacks(int sq, ulong occ)` | 4 | |
| `Attacks.IsAttackedBy` | method | `public static bool IsAttackedBy(Position pos, int sq, int byColor)` | 4 | 0=white,1=black |
| `Attacks.NightAttacksOf` | method | `static ulong KnightAttacksOf(ulong knights)` | 4 | |
| `Attacks.InitKnight` | method | `static void InitKnight()` | 4 | |
| `Attacks.KingAttacksOf` | method | `static ulong KingAttacksOf(ulong k)` | 4 | |
| `Attacks.InitKing` | method | `static void InitKing()` | 4 | |
| `Attacks.InitPawn` | method | `static void InitPawn()` | 4 | |
| `Attacks.InitRays` | method | `static void InitRays()` | 4 | |
| `Attacks.PositiveRay` | method | `static ulong PositiveRay(int sq, int dir, ulong occ)` | 4 | N,NE,E,NW |
| `Attacks.NegativeRay` | method | `static ulong NegativeRay(int sq, int dir, ulong occ)` | 4 | S,SE,W,SW |
| `MoveGenerator` | static class | — | 1(stub), 4 | |
| `MoveGenerator.GenerateMoves` | method | `public static Move[] GenerateMoves(Position position)` | 1 | Stub, removed in Part 2 |
| `MoveGenerator.GeneratePseudoLegal` | method | `public static int GeneratePseudoLegal(Position pos, Span<Move> dest)` | 4 | Returns new count `n` |
| `MoveGenerator.GenKnights` | method | `static int GenKnights(Position pos, Span<Move> dest, int n, Piece us)` | 4 | |
| `MoveGenerator.GenKing` | method | `static int GenKing(Position pos, Span<Move> dest, int n, Piece us)` | 4 | |
| `MoveGenerator.GenSliders` | method | `static int GenSliders(Position pos, Span<Move> dest, int n, Piece us, bool isBishop)` | 4 | |
| `MoveGenerator.GenWhitePawns` | method | `static int GenWhitePawns(Position pos, Span<Move> dest, int n)` | 4 | |
| `MoveGenerator.GenBlackPawns` | method | `static int GenBlackPawns(Position pos, Span<Move> dest, int n)` | 4 | |
| `MoveGenerator.GenWhiteCastling` | method | `static int GenWhiteCastling(Position pos, Span<Move> dest, int n)` | 4 | |
| `MoveGenerator.GenBlackCastling` | method | `static int GenBlackCastling(Position pos, Span<Move> dest, int n)` | 4 | |
| `EmitTargets` | method | `static int EmitTargets(Span<Move> dest, int n, int from, ulong targets, ulong enemy)` | 4, 7 | Shared helper; capture vs quiet decided here |
| `EmitPawnTargets` | method | `static int EmitPawnTargets(Span<Move> dest, int n, ulong targets, int deltaFrom, MoveFlag flag)` | 4 | |
| `EmitPromotions` | method | `static int EmitPromotions(Span<Move> dest, int n, ulong targets, int deltaFrom, bool capture)` | 4 | Part 4 name (Part 7 uses `EmitPromos`) |
| `UndoInfo` | struct | — | 5 | |
| `UndoInfo.Castling` | field | `public CastlingRights` | 5 | |
| `UndoInfo.EpSquare` | field | `public int` | 5 | |
| `UndoInfo.HalfmoveClock` | field | `public int` | 5 | |
| `UndoInfo.Captured` | field | `public Piece` | 5 | `Piece.None` for non-captures |
| `Divide` | method | `public static void Divide(Position pos, int depth)` | 5 | Per-root-move subtotals |
| `IsCastleLegal` | method | `static bool IsCastleLegal(Position pos, Move m)` | 5 | Start/transit/dest attack test |
| `Magic` | static class | — | 6, 7 | Declared 3× in the page |
| `Magic.Init` | method | `public static void Init()` | 6 | Idempotent; static ctor calls it |
| `Magic.RookAttacks` | method | `public static ulong RookAttacks(int sq, ulong occ)` | 6 | `((occ & mask)*magic) >> shift` |
| `Magic.BishopAttacks` | method | `public static ulong BishopAttacks(int sq, ulong occ)` | 6 | |
| `Magic.QueenAttacks` | method | `public static ulong QueenAttacks(int sq, ulong occ)` | 6 | |
| `Magic.RookMasks/BishopMasks/RookMagics/BishopMagics` | static field | `static readonly ulong[]` (64) | 6 | |
| `Magic.RookShifts/BishopShifts` | static field | `static readonly int[]` (64) | 6 | `64 - bits` |
| `Magic.RookTable/BishopTable` | static field | `static readonly ulong[][]` (64) | 6 | `ulong[1<<bits]` per square |
| `Magic.InitMasks` | method | `static void InitMasks()` | 6 | |
| `Magic.BuildRookMask` | method | `static ulong BuildRookMask(int sq)` | 6 | |
| `Magic.BuildBishopMask` | method | `static ulong BuildBishopMask(int sq)` | 6 | |
| `Magic.FindMagic` | method | `static ulong FindMagic(int sq, ulong mask, int bits, bool isBishop)` | 6 | Sparse-random search |
| `Magic.BuildTable` | method | `static ulong[] BuildTable(int sq, ulong mask, ulong magic, int bits, bool isBishop)` | 6 | |
| `SparseRandom` | method | `static ulong SparseRandom(System.Random rng)` | 6 | `R() & R() & R()` |
| `OccupancyAtIndex` | method | `static ulong OccupancyAtIndex(int index, ulong mask)` | 6 | |
| `PrecomputedRookMagics` | static field | `static readonly ulong[]` | 6 | Optional constant array |
| `Legality` | static class | — | 7 | Declared 2× in the page |
| `Legality.ComputeKingDanger` | method | `public static ulong ComputeKingDanger(Position pos, int kingSq, int enemyColor)` | 7 | Occupancy minus our king |
| `Legality.ComputeCheckers` | method | `public static ulong ComputeCheckers(Position pos, int kingSq, int enemyColor)` | 7 | |
| `Legality.ComputePins` | method | `public static void ComputePins(Position pos, int kingSq, int enemyColor, out ulong pinned, Span<ulong> pinLines)` | 7 | `pinLines` pre-filled with `ulong.MaxValue` |
| `Legality.ScanPinners` | method | `static ulong ScanPinners(int kingSq, ulong us, ulong occ, ulong enemySliders, bool isBishop, Span<ulong> pinLines)` | 7 | X-ray pin detection |
| `Legality.SquaresBetween` | static field | `public static readonly ulong[,]` (64×64) | 7 | |
| `Legality.BuildSquaresBetween` | method | `static ulong[,] BuildSquaresBetween()` | 7 | |
| `LegalMoveGenerator` | static class | — | 7 | |
| `LegalMoveGenerator.Generate` | method | `public static int Generate(Position pos, System.Span<Move> dest)` | 7 | Returns count of legal moves |
| `AttackFn` | delegate | `delegate ulong AttackFn(int sq, ulong occ)` | 7 | |
| `GenKnightsLegal` | method | `static int GenKnightsLegal(Position pos, System.Span<Move> dest, int n, int us, ulong checkMask, ulong pinned)` | 7 | |
| `GenSlidersLegal` | method | `static int GenSlidersLegal(Position pos, System.Span<Move> dest, int n, int us, ulong checkMask, ulong pinned, ulong[] pinLines)` | 7 | **param type mismatch vs caller** |
| `EmitSlider` | method | `static int EmitSlider(System.Span<Move> dest, int n, ulong pieces, AttackFn attackFn, ulong ourPieces, ulong enemy, ulong occ, ulong checkMask, ulong pinned, Span<ulong> pinLines)` | 7 | |
| `GenPawnsLegal` | method | `static int GenPawnsLegal(Position pos, System.Span<Move> dest, int n, int us, ulong checkMask, ulong pinned, ulong[] pinLines)` | 7 | **param type mismatch vs caller** |
| `EmitPawnLegal` | method | `static int EmitPawnLegal(System.Span<Move> dest, int n, ulong targets, int deltaFrom, MoveFlag flag, ulong pinned, ulong[] pinLines)` | 7 | |
| `EmitPromos` | method | `static int EmitPromos(System.Span<Move> dest, int n, ulong targets, int deltaFrom, bool capture, ulong pinned, ulong[] pinLines)` | 7 | Part 7 name (≠ Part 4 `EmitPromotions`) |
| `GenEnPassant` | method | `static int GenEnPassant(Position pos, System.Span<Move> dest, int n, int us, int kingSq, ulong checkers)` | 7 | **called with 4 args** |
| `TryEpMove` | method | `static int TryEpMove(Position pos, System.Span<Move> dest, int n, int from, int to, int us, int kingSq, ulong checkers)` | 7 | Simulates the horizontal pin |
| `GenKingMoves` | method | `static int GenKingMoves(Position pos, System.Span<Move> dest, int n, int kingSq, ulong kingDanger)` | 7 | |
| `GenCastlingLegal` | method | `static int GenCastlingLegal(Position pos, System.Span<Move> dest, int n, int kingSq, ulong kingDanger)` | 7 | `kingSq` unused |
| `PositionExtensions` | static class | — | 1 | Stub only |
| `PositionExtensions.MakeMove` | method | `public static void MakeMove(this Position p, Move m)` | 1 | Stub, replaced by instance method |
| `PositionExtensions.UnmakeMove` | method | `public static void UnmakeMove(this Position p, Move m)` | 1 | Stub, replaced by instance method |
| `Program.Main` | method | `static void Main()` | 1,2,4,5,6,7 | Re-declared per part |

### Conventions to carry into the Rust rewrite
- Colour encoding: `0` = white, `1` = black (`byColor`, `us`, `them`, `enemyColor`). Piece enum order: WhitePawn=0 … WhiteKing=5, BlackPawn=6 … BlackKing=11, None=12.
- Square encoding: LERF, `sq = rank*8 + file`.
- Move value: `flag << 12 | from << 6 | to`, 16 bits.
- Generator contract: append into a caller-owned buffer, return the new index.
- Perft contract: generate → make → recurse → unmake; count leaves only; depth-1 bulk count optional.
- No Zobrist hash, no transposition table, no move ordering anywhere in the series.
