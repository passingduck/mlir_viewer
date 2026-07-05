# M4a identity capture spike findings

**Toolchain:** LLVM/MLIR 21.1.0-rc1  
**Scope:** cooperative greedy rewrite drivers at `Fidelity::Full`

## Result

The spike met the lifecycle-event acceptance bar. `TraceRecorder` exposes a
`RewriterBase::Listener` that a pass supplies to `GreedyRewriteConfig`. A real
canonicalizer run over `arith.addi %arg, %c0` produced one `replaced`, one
`modified`, and two `erased` events. A small greedy-driver probe pattern over
`func.return` produced one `inserted` and one `modified` event. The integration
test checks all four event kinds in the SQLite trace.

The listener is deliberately cooperative rather than context-wide. MLIR 21.1
has no universal operation-mutation listener, so a pass that does not configure
the recorder's listener still gets Full text snapshots and op indexes but no
lifecycle events. M4b must fingerprint-match those gaps.

## Attribution

When the `MLIRContext` has no existing Actions handler, the recorder installs
one for the duration of capture. Events observed inside an action use
`source = action`; otherwise they use `source = listener`. The custom rewrite
pattern was attributed by its concrete debug name. Canonicalizer folding was
attributed only as `GreedyPatternRewriteIteration(1)`, because that fold is not
an `ApplyPatternAction` with a pattern debug name.

An existing context Actions handler is never replaced. In that case listener
capture remains active and pattern callbacks can still provide a debug name,
but broader action attribution is unavailable. Action and pattern state is
tracked per thread and as a stack for nested actions.

## Op index

The spike uses the documented ordinal fallback: `byte_start` is the operation's
pre-order ordinal and `byte_end = -1`. MLIR's public printer API does not expose
callbacks for operation byte boundaries. Printing each nested operation
separately does not reproduce its substring in the enclosing snapshot because
indentation, SSA naming, and surrounding scope affect the output, so accumulated
single-op lengths would be incorrect byte offsets.

Every Full before/after snapshot writes one index row per walked operation using
the same pointer token as lifecycle events. Timeline and Text modes write no
identity or op-index rows.

## Residual gaps for M4b

- Direct IR mutation and passes that do not accept a rewrite listener have no
  lifecycle events.
- Fold attribution may identify only the greedy iteration, not a named pattern.
- Replacement with block arguments or multiple values has no replacement
  operation token and stores `new_token = NULL`.
- Ordinals must be mapped to the tolerant parser's matching pre-order operation
  sequence; they are not source byte offsets.
