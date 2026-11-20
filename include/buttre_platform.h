/* buttre — C ABI for native input-method hosts (FFI v4).
 *
 * Hand-maintained mirror of crates/buttre-platform/src/platforms/macos/ffi.rs
 * — keep the two in sync when the surface changes.
 *
 * Threading: all functions are safe to call from any thread; calls on the
 * same engine are internally serialized.
 *
 * String lifetime: every returned pointer is UTF-8, owned by the engine, and
 * valid until the NEXT call on the SAME engine id.
 *
 * IMKit mapping:
 *   result.replaced != NULL    -> commit-as-you-go: the client must hold
 *                                 exactly `replaced` right before an empty
 *                                 selection (attributedSubstringFromRange:);
 *                                 then [client insertText:(commit ?: @"")
 *                                 replacementRange:<that range>]. If it does
 *                                 not (caret moved, client can't read back),
 *                                 replace NOTHING: buttre_engine_reset, then
 *                                 feed the same key again (a fresh run never
 *                                 replaces).
 *   result.commit  != NULL     -> [client insertText:commit]   (do this FIRST)
 *   result.preedit != NULL     -> [client setMarkedText:preedit]; "" clears.
 *                                 NULL -> leave the client alone: with no
 *                                 marked text, setMarkedText:@"" deletes the
 *                                 client's selection.
 *   result.candidates_changed  -> re-read candidate_count/candidate_at and
 *                                 show or hide the candidate panel
 *   result.handled == false    -> return NO from handleEvent:client: so the
 *                                 system delivers the original key event
 *                                 (separators: the committed word lands
 *                                 first, then the key)
 */
#ifndef BUTTRE_PLATFORM_H
#define BUTTRE_PLATFORM_H

#include <stdbool.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct ButtreKeyResult {
    /* false -> the host must let the original key event through. */
    bool handled;
    /* Text to insert into the client, or NULL when nothing commits. */
    const char *commit;
    /* Full current composition when it changed ("" clears the marked
     * range); NULL when the marked text is unchanged. */
    const char *preedit;
    /* The candidate list was shown, replaced or hidden. */
    bool candidates_changed;
    /* Commit-as-you-go only: text immediately before the caret that
     * `commit` replaces (NULL commit = delete it); NULL = plain insert. */
    const char *replaced;
} ButtreKeyResult;

/* Create an engine in the saved method. Returns a non-zero handle, or 0 on
 * failure. The first call also loads the bundle's resources and starts the
 * settings/method watchers. */
uint64_t buttre_engine_new(void);

/* Free an engine. 0 / unknown ids are safe no-ops. */
void buttre_engine_free(uint64_t engine_id);

/* Feed one key press as the layout-translated character AppKit reports
 * (event.characters), one Unicode scalar; pass 0 when the event does not
 * carry exactly one. Keys that compose nothing (arrows, F-keys, Escape, Tab,
 * forward delete, 0) commit the pending word and come back handled=false. */
ButtreKeyResult buttre_engine_process_char(uint64_t engine_id, uint32_t codepoint);

/* Backspace. handled=false when nothing is composing. */
ButtreKeyResult buttre_engine_process_backspace(uint64_t engine_id);

/* Commit the pending word out-of-band with word-boundary repair — call on
 * focus loss (deactivateServer), shortcuts, or commitComposition:. */
ButtreKeyResult buttre_engine_flush(uint64_t engine_id);

/* Discard the composition WITHOUT committing (Escape on the candidate list). */
ButtreKeyResult buttre_engine_reset(uint64_t engine_id);

/* Negotiate the typing model — call before EVERY key. `can_replace`: the
 * client reports a selectedRange (location != NSNotFound). With the
 * "Gõ thẳng không gạch chân" setting on and can_replace, Telex/VNI commit as
 * you go (results carry `replaced`); otherwise — and always for Nôm — the
 * preedit model stays. A model change commits the pending word. */
ButtreKeyResult buttre_engine_sync_client(uint64_t engine_id, bool can_replace);

/* Nôm candidates: count (0 = hide the panel), display string of one entry
 * (NULL when out of range), and commit of one entry (global 0-based index). */
uint32_t buttre_engine_candidate_count(uint64_t engine_id);
const char *buttre_engine_candidate_at(uint64_t engine_id, uint32_t index);
ButtreKeyResult buttre_engine_select_candidate(uint64_t engine_id, uint32_t index);

/* The OS selected input mode `method`: "telex", "vni", "nom" or a bundled
 * keyboard stem (the mode id minus "<bundle id>."). Call on every mode
 * switch and on activation. A change commits the pending word
 * (result.commit), switches this engine and persists the choice so other
 * engines follow; the current method is a no-op. Unknown ids / NULL ->
 * handled=false, nothing changes. `method`: NUL-terminated UTF-8, valid for
 * the call. */
ButtreKeyResult buttre_engine_select_method(uint64_t engine_id, const char *method);

#ifdef __cplusplus
}
#endif

#endif /* BUTTRE_PLATFORM_H */
