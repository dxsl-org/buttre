/* buttre — C ABI for native input-method hosts (FFI v2).
 *
 * Hand-maintained mirror of crates/buttre-platform/src/platforms/macos/ffi.rs
 * — keep the two in sync when the surface changes.
 *
 * Threading: all functions are safe to call from any thread; calls on the
 * same engine are internally serialized.
 *
 * String lifetime: pointers in ButtreKeyResult are UTF-8, owned by the
 * engine, and valid until the NEXT call on the SAME engine id.
 *
 * IMKit mapping:
 *   result.commit  != NULL  -> [client insertText:commit]   (do this FIRST)
 *   result.preedit          -> [client setMarkedText:preedit]; "" -> unmark
 *   result.handled == false -> return NO from handle:client: so the system
 *                              delivers the original key event (separators:
 *                              the committed word lands first, then the key)
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
    /* Full current composition (marked text). "" clears the marked range.
     * NULL only when the engine id is invalid. */
    const char *preedit;
} ButtreKeyResult;

/* Create an engine (telex). Returns a non-zero handle, or 0 on failure. */
uint64_t buttre_engine_new(void);

/* Free an engine. 0 / unknown ids are safe no-ops. */
void buttre_engine_free(uint64_t engine_id);

/* Feed one key press.
 *   keycode: macOS virtual keycode (US ANSI). Space and Return ARE mapped —
 *            the engine classifies separators itself.
 *   shift/capslock: letter case = capslock XOR shift.
 * Unmapped keycodes (arrows, Tab, Escape, ...) come back handled=false with
 * no state change — call buttre_engine_flush first for keys that must end
 * the composition. */
ButtreKeyResult buttre_engine_process_key(uint64_t engine_id, uint16_t keycode,
                                          bool shift, bool capslock);

/* Backspace. handled=false when nothing is composing. */
ButtreKeyResult buttre_engine_process_backspace(uint64_t engine_id);

/* Commit the pending word out-of-band with word-boundary repair — call on
 * focus loss (deactivateServer), navigation keys, or shortcuts. */
ButtreKeyResult buttre_engine_flush(uint64_t engine_id);

/* Discard the composition WITHOUT committing (Escape semantics). */
void buttre_engine_reset(uint64_t engine_id);

/* Switch method: 0 = telex, 1 = vni, 2 = nom. Discards any live
 * composition. Returns true on success. */
bool buttre_engine_set_method(uint64_t engine_id, uint8_t method);

/* Enable/disable. Disabling discards the composition — flush first if the
 * pending word should be committed. Disabled engines pass everything. */
void buttre_engine_set_enabled(uint64_t engine_id, bool enabled);

#ifdef __cplusplus
}
#endif

#endif /* BUTTRE_PLATFORM_H */
