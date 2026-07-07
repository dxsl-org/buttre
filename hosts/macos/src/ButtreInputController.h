// buttre macOS IMKit host — input controller.

#import <InputMethodKit/InputMethodKit.h>

// One instance per input session (IMKit creates/destroys these as focus
// moves). Owns one Rust engine handle for the session's composition state.
@interface ButtreInputController : IMKInputController
@end
