// buttre macOS IMKit host — input controller.
//
// Thin glue over the Rust FFI v3 (`buttre_platform.h`): the engine does all
// composition; this controller only translates NSEvents into engine calls
// and the resulting ButtreKeyResult into IMKit marked-text/commit calls and
// the Nôm candidate panel, and reports the selected input mode (each method
// — Telex, VNI, Chữ Nôm, every bundled keyboard — is its own input source).
//
// Why IMKit (not a CGEventTap): the OS routes keys here only while buttre is
// the selected input source, so there is no global key tap and NO
// Accessibility permission — the app is not flagged as a keylogger.
//
// Mapping (see buttre_platform.h):
//   result.commit  != NULL  -> [client insertText:]          (word first)
//   result.preedit != NULL  -> [client setMarkedText:]; "" -> unmark
//                              (NULL: untouched — an empty setMarkedText with
//                              no marked text would delete the selection)
//   result.candidates_changed -> refresh / hide the candidate panel
//   result.handled == false -> return NO so the OS delivers the original key
//                              (separators: the committed word is inserted,
//                              then the space/punct reaches the client)

#import "ButtreInputController.h"
#import "buttre_platform.h"

// Layout-independent virtual keycodes the controller routes itself; every
// other key goes to the engine as the character the user's layout produced.
enum {
    kVKReturn = 36,
    kVKSpace = 49,
    kVKDelete = 51,  // Backspace
    kVKEscape = 53,
    kVKKeypadEnter = 76,
    kVKPageUp = 116,
    kVKPageDown = 121,
    kVKLeftArrow = 123,
    kVKRightArrow = 124,
    kVKDownArrow = 125,
    kVKUpArrow = 126,
};

// Rows the single-column panel shows at once; number keys 1-9 pick a row on
// the page holding the highlight (same rule as the IBus lookup table).
static const NSUInteger kCandidatePageSize = 9;

// One panel per process, shared by every controller (IMKit draws a single
// candidate window and asks the active controller for its `candidates:`).
static IMKCandidates *shared_candidates(IMKServer *server) {
    static IMKCandidates *panel = nil;
    if (panel == nil) {
        panel = [[IMKCandidates alloc]
            initWithServer:server
                 panelType:kIMKSingleColumnScrollingCandidatePanel];
        // Keys reach handleEvent: first; the controller decides which ones
        // the panel gets, so Space/digits/Return follow the engine's rules.
        [panel setAttributes:@{
            (NSString *)IMKCandidatesSendServerKeyEventFirst : @YES
        }];
    }
    return panel;
}

// The single Unicode scalar an event's characters carry, or 0 when it is not
// exactly one (dead key, multi-character event) — the engine treats 0 as a
// non-text key that ends the word.
static uint32_t single_codepoint(NSString *chars) {
    if (chars.length == 1) {
        return [chars characterAtIndex:0];
    }
    if (chars.length == 2) {
        unichar hi = [chars characterAtIndex:0];
        unichar lo = [chars characterAtIndex:1];
        if (CFStringIsSurrogateHighCharacter(hi) &&
            CFStringIsSurrogateLowCharacter(lo)) {
            return (uint32_t)CFStringGetLongCharacterForSurrogatePair(hi, lo);
        }
    }
    return 0;
}

static NSString *string_or_empty(const char *utf8) {
    NSString *s = utf8 != NULL ? [NSString stringWithUTF8String:utf8] : nil;
    return s != nil ? s : @"";
}

@implementation ButtreInputController {
    uint64_t _engine;
    NSArray<NSString *> *_candidates;
}

- (instancetype)initWithServer:(IMKServer *)server
                      delegate:(id)delegate
                        client:(id)inputClient {
    self = [super initWithServer:server delegate:delegate client:inputClient];
    if (self) {
        _engine = buttre_engine_new();  // 0 on failure — handled per call
        _candidates = @[];
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
        NSString *preedit = string_or_empty(r.preedit);
        [client setMarkedText:preedit
               selectionRange:NSMakeRange(preedit.length, 0)
             replacementRange:NSMakeRange(NSNotFound, NSNotFound)];
    }
    if (r.candidates_changed) {
        [self refreshCandidates];
    }
}

// Pull the engine's candidate list into the shared panel, or hide it.
- (void)refreshCandidates {
    uint32_t count = buttre_engine_candidate_count(_engine);
    NSMutableArray<NSString *> *items = [NSMutableArray arrayWithCapacity:count];
    for (uint32_t i = 0; i < count; i++) {
        [items addObject:string_or_empty(buttre_engine_candidate_at(_engine, i))];
    }
    _candidates = items;
    IMKCandidates *panel = shared_candidates(self.server);
    if (count == 0) {
        [panel hide];
        return;
    }
    [panel updateCandidates];
    [panel show:kIMKLocateCandidatesBelowHint];
}

- (NSArray *)candidates:(id)sender {
    return _candidates;
}

// Index of the panel's highlighted row in `_candidates` (top row when the
// panel reports none).
- (NSUInteger)highlightedCandidate {
    NSAttributedString *selected = [shared_candidates(self.server) selectedCandidateString];
    NSUInteger index = selected != nil ? [_candidates indexOfObject:selected.string]
                                       : NSNotFound;
    return index == NSNotFound ? 0 : index;
}

- (void)selectCandidate:(NSUInteger)index client:(id)client {
    if (index < _candidates.count) {
        [self apply:buttre_engine_select_candidate(_engine, (uint32_t)index)
           toClient:client];
    }
}

// Mouse click on a panel row.
- (void)candidateSelected:(NSAttributedString *)candidateString {
    NSUInteger index = [_candidates indexOfObject:candidateString.string];
    if (index != NSNotFound) {
        [self selectCandidate:index client:self.client];
    }
}

// Keys that operate the visible candidate list. Returns NO for keys that
// should keep composing (letters refine the lookup, Backspace shrinks it).
- (BOOL)handleCandidateKey:(NSEvent *)event client:(id)sender {
    switch (event.keyCode) {
        case kVKEscape:
            [self apply:buttre_engine_reset(_engine) toClient:sender];
            return YES;
        case kVKSpace:
        case kVKReturn:
        case kVKKeypadEnter:
            [self selectCandidate:[self highlightedCandidate] client:sender];
            return YES;
        case kVKUpArrow:
        case kVKDownArrow:
        case kVKLeftArrow:
        case kVKRightArrow:
        case kVKPageUp:
        case kVKPageDown:
            [shared_candidates(self.server) interpretKeyEvents:@[ event ]];
            return YES;
        default:
            break;
    }
    uint32_t ch = single_codepoint(event.characters);
    if (ch >= '1' && ch <= '9') {
        NSUInteger page = [self highlightedCandidate] / kCandidatePageSize;
        [self selectCandidate:page * kCandidatePageSize + (ch - '1') client:sender];
        return YES;  // out of range is a no-op, never a stray digit
    }
    return NO;
}

- (BOOL)handleEvent:(NSEvent *)event client:(id)sender {
    if (event.type != NSEventTypeKeyDown) {
        return NO;
    }

    // Let the OS keep system shortcuts (Cmd/Ctrl/Option combos): flush the
    // pending word first so it isn't lost, then pass the combo through.
    if (event.modifierFlags & (NSEventModifierFlagCommand | NSEventModifierFlagControl |
                               NSEventModifierFlagOption)) {
        [self apply:buttre_engine_flush(_engine) toClient:sender];
        return NO;
    }

    if (_candidates.count > 0 && [self handleCandidateKey:event client:sender]) {
        return YES;
    }

    // Everything else goes to the engine as the character the user's layout
    // produced; navigation/function keys come through as non-text and end
    // the word there.
    ButtreKeyResult result = event.keyCode == kVKDelete
        ? buttre_engine_process_backspace(_engine)
        : buttre_engine_process_char(_engine, single_codepoint(event.characters));
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

- (void)hidePalettes {
    [shared_candidates(self.server) hide];
    [super hidePalettes];
}

// Input mode (= method) chosen in the OS input menu. Mode ids are
// "<bundle id>.<method>" (Info.plist ComponentInputModeDict, generated per
// bundled keyboard by build_app.sh); anything else is not ours to apply.
- (void)selectMode:(NSString *)modeID client:(id)client {
    NSString *prefix = [NSBundle.mainBundle.bundleIdentifier stringByAppendingString:@"."];
    if (![modeID hasPrefix:prefix]) {
        return;
    }
    NSString *method = [modeID substringFromIndex:prefix.length];
    [self apply:buttre_engine_select_method(_engine, method.UTF8String) toClient:client];
}

- (void)setValue:(id)value forTag:(long)tag client:(id)sender {
    if (tag == kTextServiceInputModePropertyTag && [value isKindOfClass:[NSString class]]) {
        [self selectMode:value client:sender];
    }
    [super setValue:value forTag:tag client:sender];
}

// Re-assert the selected mode on activation: the method file is shared with
// the config window and other sessions, so it may name a different method
// than the input source the menu bar shows.
- (void)activateServer:(id)sender {
    [super activateServer:sender];
    TISInputSourceRef source = TISCopyCurrentKeyboardInputSource();
    if (source == NULL) {
        return;
    }
    NSString *modeID =
        (__bridge NSString *)TISGetInputSourceProperty(source, kTISPropertyInputModeID);
    if (modeID != nil) {
        [self selectMode:modeID client:sender];
    }
    CFRelease(source);
}

// Input-menu items (the menu bar's input-source menu, under the selected
// buttre mode). macOS has no tray (ADR-0003: the OS owns method choice), so
// this is the door into the settings window — the macOS counterpart of the
// IBus property menu's "Cấu hình Buttre..." item. IMKit routes the item's
// action to the active controller.
- (NSMenu *)menu {
    NSMenu *menu = [[NSMenu alloc] initWithTitle:@"buttre"];
    [menu addItemWithTitle:@"Cấu hình buttre…" action:@selector(openSettings:) keyEquivalent:@""];
    return menu;
}

// Spawn the bundled settings helper (`Contents/Helpers/buttre-config`, the
// Rust `buttre` binary — see build_app.sh) with `--config`. A separate
// process: the Slint window needs its own event loop and a foreground
// activation policy this background-only IMKit server can't give it. A second
// click is harmless — the config window holds its own single-instance lock.
- (void)openSettings:(id)sender {
    NSURL *helper = [NSBundle.mainBundle.bundleURL
        URLByAppendingPathComponent:@"Contents/Helpers/buttre-config"];
    NSError *error = nil;
    NSTask *task = [NSTask launchedTaskWithExecutableURL:helper
                                               arguments:@[ @"--config" ]
                                                   error:&error
                                      terminationHandler:nil];
    if (task == nil) {
        NSLog(@"buttre: cannot open settings (%@): %@", helper.path, error);
    }
}

@end
