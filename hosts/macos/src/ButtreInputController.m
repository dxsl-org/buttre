// buttre macOS IMKit host — input controller.
//
// Thin glue over the Rust FFI v2 (`buttre_platform.h`): the engine does all
// composition; this controller only translates NSEvents into engine calls
// and the resulting ButtreKeyResult into IMKit marked-text/commit calls.
//
// Why IMKit (not a CGEventTap): the OS routes keys here only while buttre is
// the selected input source, so there is no global key tap and NO
// Accessibility permission — the app is not flagged as a keylogger.
//
// Mapping (see buttre_platform.h):
//   result.commit  != NULL  -> [client insertText:]          (word first)
//   result.preedit          -> [client setMarkedText:]; "" -> unmark
//   result.handled == false -> return NO so the OS delivers the original key
//                              (separators: the committed word is inserted,
//                              then the space/punct reaches the client)

#import "ButtreInputController.h"
#import "buttre_platform.h"

// macOS virtual keycodes we special-case here; letters/digits/punct are
// mapped inside the Rust FFI from the raw keycode.
enum {
    kVKDelete = 51,   // Backspace
    kVKEscape = 53,
};

@implementation ButtreInputController {
    uint64_t _engine;
}

- (instancetype)initWithServer:(IMKServer *)server
                      delegate:(id)delegate
                        client:(id)inputClient {
    self = [super initWithServer:server delegate:delegate client:inputClient];
    if (self) {
        _engine = buttre_engine_new();  // 0 on failure — handled per call
    }
    return self;
}

- (void)dealloc {
    buttre_engine_free(_engine);
}

// Apply one ButtreKeyResult to the client. Commit (if any) is inserted
// BEFORE the preedit is updated, so a separator's word lands ahead of the
// forwarded key and the marked region never momentarily doubles the text.
- (void)apply:(ButtreKeyResult)r toClient:(id)client {
    if (r.commit != NULL) {
        NSString *commit = [NSString stringWithUTF8String:r.commit];
        if (commit != nil) {
            [client insertText:commit
              replacementRange:NSMakeRange(NSNotFound, NSNotFound)];
        }
    }
    if (r.preedit != NULL) {
        NSString *preedit = [NSString stringWithUTF8String:r.preedit];
        if (preedit == nil) {
            preedit = @"";
        }
        // Empty marked text clears the composition region (there is no
        // separate unmark call needed — zero-length marked text does it).
        [client setMarkedText:preedit
               selectionRange:NSMakeRange(preedit.length, 0)
             replacementRange:NSMakeRange(NSNotFound, NSNotFound)];
    }
}

- (BOOL)handleEvent:(NSEvent *)event client:(id)sender {
    if (event.type != NSEventTypeKeyDown) {
        return NO;
    }

    NSEventModifierFlags flags = event.modifierFlags;
    // Let the OS keep system shortcuts (Cmd/Ctrl/Option combos): flush the
    // pending word first so it isn't lost, then pass the combo through.
    if (flags & (NSEventModifierFlagCommand | NSEventModifierFlagControl |
                 NSEventModifierFlagOption)) {
        [self apply:buttre_engine_flush(_engine) toClient:sender];
        return NO;
    }

    unsigned short keycode = event.keyCode;

    // Escape / navigation etc.: flush and pass through (the engine ends the
    // word; the app still receives the key).
    if (keycode == kVKEscape) {
        [self apply:buttre_engine_flush(_engine) toClient:sender];
        return NO;
    }

    ButtreKeyResult result;
    if (keycode == kVKDelete) {
        result = buttre_engine_process_backspace(_engine);
    } else {
        BOOL shift = (flags & NSEventModifierFlagShift) != 0;
        BOOL caps = (flags & NSEventModifierFlagCapsLock) != 0;
        result = buttre_engine_process_key(_engine, keycode, shift, caps);
    }

    [self apply:result toClient:sender];
    return result.handled ? YES : NO;
}

// The client is committing (focus change / app request): flush the pending
// word so switching apps or fields never eats what the user typed.
- (void)commitComposition:(id)sender {
    [self apply:buttre_engine_flush(_engine) toClient:sender];
}

- (void)deactivateServer:(id)sender {
    [self apply:buttre_engine_flush(_engine) toClient:sender];
    [super deactivateServer:sender];
}

@end
