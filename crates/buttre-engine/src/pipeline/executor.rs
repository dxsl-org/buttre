//! Pipeline Executor — orchestrates the 7-stage processing pipeline (Phase 4+).
//!
//! ## 7-Stage Architecture
//!
//! | # | Stage | Responsibility |
//! |---|-------|----------------|
//! | 1 | Normalization | Normalize input, populate char_buffer |
//! | 2 | Gatekeeper | Route non-Vietnamese / temp-English passthrough |
//! | 3 | Compose | Recompute-from-raw: segment → transform → tone → fallback |
//! | 4 | Orthography | Normalize Unicode form |
//! | 5 | Learning | Track patterns (no-op until Phase 5) |
//! | 6 | Lookup | Dictionary lookup (Nôm candidates) |
//! | 7 | Output | Diff last_output → syllable_buffer → emit actions |
//!
//! The former dual-engine (Transform + Tone + Permutation + Reconciliation + Retrofix)
//! has been retired.  All composition logic lives in `crates/buttre-engine/src/compose/`
//! and is invoked by `ComposeStage` as a single recompute-from-raw step.

use std::sync::{Arc, RwLock};

use crate::compose::{compose_closed, ComposeOpts, LearningSnapshot, Validator};
use crate::pipeline::context::{CharInfo, CharInfoBufferExt};
use crate::pipeline::stages::compose_stage::apply_case_mask;
use crate::pipeline::stages::*;
use crate::pipeline::{PipelineConfig, PipelineStage, StageResult, TypingContext};
use crate::types::Action;
use tracing::{debug, instrument, trace, warn};

/// Pipeline Executor
///
/// Orchestrates the 7-stage pipeline from Phase 4 onward.
///
/// ## Flow control
///
/// Each stage returns a [`StageResult`]:
/// - `Continue` — proceed to the next stage.
/// - `PassThrough` — confirm any in-progress composition, commit the raw char,
///   reset context.
/// - `Output(actions)` — short-circuit; return these actions directly.
pub struct PipelineExecutor {
    /// All stages in processing order.
    stages: Vec<Box<dyn PipelineStage>>,

    /// Shared mutable state threaded through every stage.
    context: TypingContext,

    /// Whether to emit TSF composition actions (vs. simple commit/replace).
    use_composition: bool,

    /// Word-boundary final repair opts (event-sourcing-completion Phase 3):
    /// `Some` only when the compose stage is present in this pipeline,
    /// `config.boundary_repair` is enabled, AND the compose validator is
    /// `Validator::Vietnamese` (there is no attested-syllable table to gate
    /// against otherwise). `None` makes [`Self::boundary_repair`] a no-op.
    boundary_repair_opts: Option<ComposeOpts>,

    /// Shared handle into the live compose stage's merged `ComposeOpts`
    /// (event-sourcing-completion Phase 5; perf: the learning fields are
    /// written in place here at word boundaries instead of being re-merged
    /// into a full opts clone on every keystroke) — `Some` only when a
    /// compose stage is present. Captured at construction time, before the
    /// stage is boxed into the type-erased `stages` vec (see [`Self::new`]),
    /// since `PipelineStage` has no reason to expose a generic "set learning
    /// data" method for the other 6 stages that never consult one. See
    /// [`Self::set_learning_snapshot`].
    compose_live_opts: Option<Arc<RwLock<ComposeOpts>>>,
}

impl PipelineExecutor {
    /// Construct a new `PipelineExecutor` from a [`PipelineConfig`].
    ///
    /// Stages 1 (Normalization) and 2 (Gatekeeper) are always first.
    /// Stage 7 (Output) is always last.
    /// The middle stages are controlled by `config.pipeline.enabled`:
    /// - Empty → default order: `["compose", "orthography", "learning", "lookup"]`
    /// - Non-empty → honour that list (allows non-default configs to opt in/out of steps).
    ///
    /// # Arguments
    ///
    /// * `config` — Pipeline configuration (transform tables, tone map, etc.)
    pub fn new(config: PipelineConfig) -> Self {
        let mut stages: Vec<Box<dyn PipelineStage>> = Vec::new();
        let use_composition = config.pipeline.use_composition;

        // Stage 1: Normalization — ALWAYS FIRST
        stages.push(Box::new(NormalizationStage::new()));

        // Stage 2: Gatekeeper — ALWAYS SECOND
        stages.push(Box::new(GatekeeperStage::from_config(&config)));

        // Middle stages: driven by config.pipeline.enabled or default order.
        let stage_order: Vec<String> = if config.pipeline.enabled.is_empty() {
            // Default 5-stage middle pipeline (total = 7 with fixed 1 + 7).
            vec![
                "compose".to_string(),
                "orthography".to_string(),
                "learning".to_string(),
                "lookup".to_string(),
            ]
        } else {
            config.pipeline.enabled.clone()
        };

        let mut compose_stage_present = false;
        let mut compose_live_opts: Option<Arc<RwLock<ComposeOpts>>> = None;
        for stage_name in &stage_order {
            match stage_name.as_str() {
                "compose" => {
                    let stage = ComposeStage::from_config(&config);
                    compose_live_opts = Some(stage.live_opts_handle());
                    stages.push(Box::new(stage));
                    compose_stage_present = true;
                }
                "orthography" => {
                    stages.push(Box::new(OrthographyStage::from_config(&config)));
                }
                "learning" => {
                    stages.push(Box::new(LearningStage::new()));
                }
                "lookup" => {
                    stages.push(Box::new(LookupStage::from_config(&config)));
                }
                // Tolerate old stage names in non-default configs so that
                // integration tests that pass custom stage lists don't hard-break.
                "validation" => {
                    stages.push(Box::new(ValidationStage::from_config(&config)));
                }
                other => {
                    warn!("Unknown or retired stage name: '{}' — skipped", other);
                }
            }
        }

        // Stage 7 (last): Output — ALWAYS LAST
        stages.push(Box::new(OutputStage::new(use_composition)));

        // Word-boundary repair opts (Phase 3): computed once here, not on the
        // hot per-keystroke path — `ComposeOpts::from_config` is the same
        // derivation `ComposeStage::from_config` already ran; a second copy
        // is the cheapest way to reach it without exposing `ComposeStage`'s
        // internals across the `PipelineStage` trait object boundary.
        let boundary_repair_opts = if compose_stage_present && config.boundary_repair {
            let opts = ComposeOpts::from_config(&config);
            (opts.validator == Validator::Vietnamese).then_some(opts)
        } else {
            None
        };

        Self {
            stages,
            context: TypingContext::new(),
            use_composition,
            boundary_repair_opts,
            compose_live_opts,
        }
    }

    /// Refresh the learning-store snapshot consulted by every `compose()`/
    /// `compose_closed()` call this pipeline makes (event-sourcing-
    /// completion Phase 5). Updates BOTH the live compose stage's opts
    /// (via the shared handle captured in [`Self::new`]) AND the cached
    /// `boundary_repair_opts` (P3's closed-gate projection) — the
    /// "single consult point" (`pipeline::validation::is_attested_overlay`)
    /// must see the SAME data from every caller, or the open-projection and
    /// closed-projection gates could silently diverge.
    ///
    /// Callers (`buttre_core::keyboard::Keyboard`) refresh this at word
    /// boundaries only, never mid-word — see the phase's Combined Contract.
    /// A no-op (aside from the `boundary_repair_opts` half) for pipelines
    /// without a compose stage (Nôm candidate-lookup configs, native
    /// scripts).
    pub fn set_learning_snapshot(&mut self, snapshot: LearningSnapshot) {
        if let Some(handle) = &self.compose_live_opts {
            let mut guard = match handle.write() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            guard.user_attested = snapshot.user_attested.clone();
            guard.raw_prefs = snapshot.raw_prefs.clone();
        }
        if let Some(opts) = self.boundary_repair_opts.as_mut() {
            opts.user_attested = snapshot.user_attested.clone();
            opts.raw_prefs = snapshot.raw_prefs.clone();
        }
    }

    /// Compose `word` forcing the COMPOSED interpretation, IGNORING any stored
    /// raw-preference (event-sourcing-completion Phase 4: a word toggle → composed
    /// must override a `Pref::Literal`, per the Combined Contract's
    /// `toggle > pref` precedence — otherwise a stored literal pref makes the
    /// toggle-to-composed direction unreachable and the invisible double-press
    /// silently corrupts the stored direction). The user-attested overlay still
    /// applies — only the literal/composed preference is suppressed for the
    /// duration of this one compose. Reuses the full pipeline (reset → process →
    /// closed boundary repair) so case-mask + orthography match the normal path.
    pub fn compose_word_forced_composed(&mut self, word: &[char], closed: bool) -> String {
        // Null-and-save `raw_prefs` on both the live opts (read per keystroke
        // by the compose stage) and the cached boundary-repair opts, so neither
        // the open nor the closed projection consults a pref here; restore after.
        let saved_snapshot_prefs = self
            .compose_live_opts
            .as_ref()
            .and_then(|h| h.write().ok().and_then(|mut g| g.raw_prefs.take()));
        let saved_boundary_prefs = self
            .boundary_repair_opts
            .as_mut()
            .and_then(|o| o.raw_prefs.take());

        self.reset();
        for &c in word {
            self.process(c);
        }
        let out = if closed {
            self.boundary_repair()
                .unwrap_or_else(|| self.get_buffer().to_string())
        } else {
            self.get_buffer().to_string()
        };

        if let (Some(handle), Some(prefs)) = (&self.compose_live_opts, saved_snapshot_prefs) {
            if let Ok(mut g) = handle.write() {
                g.raw_prefs = Some(prefs);
            }
        }
        if let (Some(opts), Some(prefs)) =
            (self.boundary_repair_opts.as_mut(), saved_boundary_prefs)
        {
            opts.raw_prefs = Some(prefs);
        }
        out
    }

    /// Recompute the CURRENT in-progress word's word-boundary "closed"
    /// projection (event-sourcing-completion Phase 3: [`compose_closed`])
    /// WITHOUT mutating any pipeline state.
    ///
    /// The repair diff is computed against the SAME case-masked display form
    /// already on screen (`apply_case_mask`) — comparing against the raw
    /// lowercase-anchored `compose` output would spuriously "repair" the
    /// case of every mixed-case word (red-team M2: `"Vieejt"` must not
    /// downcase to `"việt"`).
    ///
    /// Returns `None` when there is nothing to repair: `boundary_repair` is
    /// disabled/inapplicable for this config (see `boundary_repair_opts`),
    /// the buffer is empty, or the closed projection is byte-identical to
    /// what's already displayed. Callers should treat `None` as "commit the
    /// buffer unchanged".
    ///
    /// Reads the FULL `char_buffer` as the word's raw keys — correct for
    /// external callers (TSF's Enter / buffer-reset-key handlers), which
    /// query this BEFORE any commit key is ever pushed into the buffer.
    /// `PipelineExecutor`'s own `PassThrough` branch (a separator commit) is
    /// the one exception: Stage 1 (Normalization) has already pushed the
    /// triggering separator itself into `char_buffer` earlier in this SAME
    /// `process()` call, so it uses [`Self::boundary_repair_excluding_last`]
    /// instead to exclude that trailing key.
    pub fn boundary_repair(&self) -> Option<String> {
        self.boundary_repair_for(&self.context.char_buffer)
    }

    /// Same as [`Self::boundary_repair`], but recomputed on `char_buffer`
    /// WITHOUT its last entry — see that method's doc for why the
    /// `PassThrough` branch (the only caller) needs this instead.
    fn boundary_repair_excluding_last(&self) -> Option<String> {
        let len = self.context.char_buffer.len();
        self.boundary_repair_for(&self.context.char_buffer[..len.saturating_sub(1)])
    }

    /// Shared implementation: recompute `buf`'s closed projection and adopt
    /// it when it differs from the currently-displayed `syllable_buffer`.
    fn boundary_repair_for(&self, buf: &[CharInfo]) -> Option<String> {
        let opts = self.boundary_repair_opts.as_ref()?;
        if buf.is_empty() {
            return None;
        }
        // A word whose English latch was entered through a DELIBERATE
        // undo/toggle ("tesst" → undo → "test") must commit exactly as
        // displayed: the stateless closed projection cannot see the mid-
        // buffer undo event, so it would re-apply the very tone the user
        // removed ("tesst" → "tét") and destroy the escape hatch — making
        // English words like "test"/"text"/"reset" untypeable. A
        // NON-deliberate latch (the mid-word fallback misfire on
        // "chwowng") keeps the repair, which is what rescues it to
        // "chương" at commit. See `TypingContext::latch_from_undo`.
        if self.context.latch_from_undo {
            return None;
        }
        let raw = buf.to_char_vec();
        let closed = compose_closed(&raw, opts);
        let repaired = apply_case_mask(&closed.text, buf, opts);
        if repaired == self.context.syllable_buffer {
            None
        } else {
            Some(repaired)
        }
    }

    /// Process one input character through the pipeline.
    ///
    /// ## Returns
    ///
    /// A `Vec<Action>` describing what the IME should do (backspace, send text, etc.).
    #[instrument(skip(self), fields(input, syllable = %self.context.syllable_buffer))]
    pub fn process(&mut self, input: char) -> Vec<Action> {
        trace!("Processing input character: '{}'", input);

        for stage in &self.stages {
            let result = stage.process(&mut self.context, input);

            match result {
                StageResult::Continue => {}
                StageResult::PassThrough => {
                    debug!(
                        "Stage '{}' returned PassThrough — confirming and resetting",
                        stage.name()
                    );

                    let mut actions = Vec::new();

                    // If TSF composition is active, confirm any pending composition first.
                    if self.use_composition && !self.context.syllable_buffer.is_empty() {
                        // Word-boundary final repair (event-sourcing-completion
                        // Phase 3): `input` (the separator that just triggered
                        // this PassThrough) is the moment of complete evidence
                        // for the word about to be confirmed. The TSF stub's
                        // Replace handler ignores `backspace_count` (it only
                        // ever rewrites the live composition) — a separate
                        // Replace-then-Confirm pair is unexecutable there, so
                        // the repair MUST be folded directly into
                        // `ConfirmComposition`'s payload.
                        let confirmed = self
                            .boundary_repair_excluding_last()
                            .unwrap_or_else(|| self.context.syllable_buffer.clone());
                        debug!("Confirming composition: {}", confirmed);
                        actions.push(Action::ConfirmComposition(confirmed));
                    }

                    trace!("Committing pass-through character: '{}'", input);
                    actions.push(Action::Commit(input.to_string()));

                    self.reset();
                    return actions;
                }
                StageResult::Output(actions) => {
                    debug!(
                        "Stage '{}' returned Output with {} action(s)",
                        stage.name(),
                        actions.len()
                    );
                    return actions;
                }
            }
        }

        // All stages returned Continue — should not happen because OutputStage always emits.
        warn!("All stages returned Continue — OutputStage should have terminated processing");
        vec![Action::DoNothing]
    }

    /// Reset the pipeline to initial state (clears char_buffer, syllable_buffer, etc.).
    pub fn reset(&mut self) {
        self.context.clear();
        for stage in &mut self.stages {
            stage.reset();
        }
    }

    /// Current composed syllable (after the latest `process` call).
    pub fn syllable(&self) -> &str {
        &self.context.syllable_buffer
    }

    /// Current composed syllable (alias for backward compatibility).
    pub fn get_buffer(&self) -> &str {
        &self.context.syllable_buffer
    }

    /// Raw key buffer (lowercase normalized keystrokes).
    pub fn raw_buffer(&self) -> String {
        self.context.raw_buffer()
    }

    /// Whether the current buffer is in temporary-English passthrough mode.
    ///
    /// This flag is set by `ComposeStage` when the key sequence looks like an
    /// English word (validation-first fallback).  The Gatekeeper reads it on
    /// the *next* keystroke.
    pub fn is_temp_english_mode(&self) -> bool {
        self.context.temp_english_mode
    }

    /// Read-only access to the typing context (used in tests).
    pub fn context(&self) -> &TypingContext {
        &self.context
    }

    /// Total number of stages (used in tests to assert pipeline depth).
    pub fn stage_count(&self) -> usize {
        self.stages.len()
    }
}
