/* buttre_ffi.h — C ABI around buttre's EngineBridge (Linux hosts).
 *
 * First consumer: the fcitx5 addon (addons/fcitx5-buttre, Phase 3).
 * Hand-maintained mirror of crates/buttre-ffi/src/lib.rs — keep in sync
 * (same convention as buttre_platform.h for the macOS IMKit host).
 *
 * Conventions:
 *  - Engines are opaque uint64_t handles; 0 is never a live handle.
 *  - Returned strings are UTF-8, owned by the engine, valid until the NEXT
 *    call on the SAME engine. Copy them before calling anything else.
 *  - Every function is safe on 0/unknown handles (no-op / pass result).
 *  - Key input is X11 keysyms (fcitx5: KeyEvent::rawKey().sym()).
 */

#ifndef BUTTRE_FFI_H
#define BUTTRE_FFI_H

#include <stdbool.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Result of one key event. */
typedef struct {
    /* false → let the original key event through (after inserting commit,
     * if any — the committed word lands first). */
    bool handled;
    /* Text to insert into the client, or NULL when nothing commits. */
    const char *commit;
    /* Full current composition; empty string = clear the preedit region.
     * NULL only on dead handles / ignored keys. */
    const char *preedit;
} BtKeyResult;

/* Create an engine for a method id ("telex"/"vni"/"nom"/"english"/custom
 * keyboard id; NULL → "telex"). Returns non-zero handle, or 0 on failure
 * (unknown method id, invalid UTF-8). */
uint64_t bt_engine_new(const char *method);

/* Free an engine. 0 / unknown ids are safe no-ops. */
void bt_engine_free(uint64_t engine_id);

/* Feed one key press. Routing matches the IBus engine: modifiers pass,
 * BackSpace edits, break keys (Tab/arrows/Escape/…) commit-then-pass,
 * printable ASCII composes. */
BtKeyResult bt_engine_process_keysym(uint64_t engine_id, uint32_t keysym);

/* Commit the pending word out-of-band (focus loss, shortcuts). */
BtKeyResult bt_engine_flush(uint64_t engine_id);

/* Discard the composition WITHOUT committing (Escape semantics). */
void bt_engine_reset(uint64_t engine_id);

/* Switch method by id. Discards any live composition. Returns true on
 * success; false leaves the previous method active. */
bool bt_engine_set_method(uint64_t engine_id, const char *method);

/* Disabled engines pass everything. Disabling discards the composition —
 * flush first if the pending word should be committed. */
void bt_engine_set_enabled(uint64_t engine_id, bool enabled);

/* Nôm candidate list of the current composition. Indexes are stable until
 * the next call on this engine. display: "𡗶 (trời)", value: "𡗶". */
uint32_t bt_engine_candidate_count(uint64_t engine_id);
const char *bt_engine_candidate_display(uint64_t engine_id, uint32_t index);
const char *bt_engine_candidate_value(uint64_t engine_id, uint32_t index);
BtKeyResult bt_engine_select_candidate(uint64_t engine_id, uint32_t index);

/* Candidate navigation — the bridge owns the cursor; the host renders the
 * highlight from bt_engine_candidate_cursor and routes keys here while
 * bt_engine_candidate_count() > 0 (mirror of the IBus engine's routing:
 * Return/Space = select_current, Up/Left = prev, Down/Right = next,
 * PgUp/PgDn = page, digits 1..9 = select_at_page(digit-1, page)). */
uint32_t bt_engine_candidate_cursor(uint64_t engine_id);
BtKeyResult bt_engine_cursor_next(uint64_t engine_id);
BtKeyResult bt_engine_cursor_prev(uint64_t engine_id);
BtKeyResult bt_engine_cursor_page_down(uint64_t engine_id, uint32_t page);
BtKeyResult bt_engine_cursor_page_up(uint64_t engine_id, uint32_t page);
BtKeyResult bt_engine_select_current(uint64_t engine_id);
BtKeyResult bt_engine_select_at_page(uint64_t engine_id, uint32_t index, uint32_t page);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* BUTTRE_FFI_H */
