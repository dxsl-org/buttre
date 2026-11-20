// buttre macOS IMKit host — entry point.
//
// Default: bootstraps the IMKit server. IMKit reads InputMethodConnectionName
// and InputMethodServerControllerClass from Info.plist, then instantiates one
// ButtreInputController per input session and routes key events to it.
//
// `buttre --install` (run as the logged-in user, after the bundle is copied
// into an Input Methods folder): registers the bundle with Text Input Sources
// and enables the primary modes — Telex, VNI, Chữ Nôm — so they show up in
// the input menu without a trip to System Settings. The other modes (bundled
// ethnic-minority keyboards) stay available for the user to add there.
//
// The server object must outlive the run loop, so it is a global — releasing
// it would tear the input method off the mach port the moment it was built.

#import <Carbon/Carbon.h>
#import <Cocoa/Cocoa.h>
#import <InputMethodKit/InputMethodKit.h>

static IMKServer *gServer = nil;

// Methods enabled by `--install`: "<bundle id>.<method>" input-mode ids.
static NSArray<NSString *> *primary_methods(void) {
    return @[ @"telex", @"vni", @"nom" ];
}

// Return the ids in `wanted` that TIS reports as DISABLED (or does not list);
// empty = all enabled. With `request`, also ask TIS to enable each of them.
static NSSet<NSString *> *disabled_sources(NSString *bundleID, NSSet<NSString *> *wanted,
                                           BOOL request) {
    NSMutableSet<NSString *> *disabled = [NSMutableSet set];
    NSDictionary *filter = @{(__bridge NSString *)kTISPropertyBundleID : bundleID};
    CFArrayRef sources = TISCreateInputSourceList((__bridge CFDictionaryRef)filter, true);
    if (sources == NULL) {
        return [wanted copy];
    }
    NSMutableSet<NSString *> *seen = [NSMutableSet set];
    for (CFIndex i = 0; i < CFArrayGetCount(sources); i++) {
        TISInputSourceRef source = (TISInputSourceRef)CFArrayGetValueAtIndex(sources, i);
        NSString *sourceID =
            (__bridge NSString *)TISGetInputSourceProperty(source, kTISPropertyInputSourceID);
        if (sourceID == nil || ![wanted containsObject:sourceID]) {
            continue;
        }
        [seen addObject:sourceID];
        CFBooleanRef isEnabled =
            TISGetInputSourceProperty(source, kTISPropertyInputSourceIsEnabled);
        if (isEnabled != NULL && CFBooleanGetValue(isEnabled)) {
            continue;
        }
        [disabled addObject:sourceID];
        if (!request) {
            continue;
        }
        OSStatus err = TISEnableInputSource(source);
        if (err != noErr) {
            fprintf(stderr, "buttre: TISEnableInputSource(%s) failed: %d\n",
                    sourceID.UTF8String, (int)err);
        }
    }
    CFRelease(sources);
    NSMutableSet<NSString *> *unknown = [wanted mutableCopy];
    [unknown minusSet:seen];
    [disabled unionSet:unknown];
    return disabled;
}

// Register this bundle with Text Input Sources and enable the primary modes
// (plus the input method itself, which owns them). Idempotent: enabled
// sources are left alone. Returns 0 once TIS reports every one enabled.
//
// TIS applies enable requests asynchronously: observed on macOS 13,
// `TISEnableInputSource` returned noErr and the mode appeared seconds later.
// So this requests once, then polls (servicing the run loop, which delivers
// TIS's change notifications), re-requesting only every few seconds —
// back-to-back requests were seen to be dropped.
static int install_input_sources(NSBundle *bundle) {
    OSStatus status = TISRegisterInputSource((__bridge CFURLRef)bundle.bundleURL);
    if (status != noErr) {
        fprintf(stderr, "buttre: TISRegisterInputSource(%s) failed: %d\n",
                bundle.bundlePath.fileSystemRepresentation, (int)status);
        return 1;
    }

    NSString *bundleID = bundle.bundleIdentifier;
    NSMutableSet<NSString *> *wanted = [NSMutableSet setWithObject:bundleID];
    for (NSString *method in primary_methods()) {
        [wanted addObject:[NSString stringWithFormat:@"%@.%@", bundleID, method]];
    }

    NSSet<NSString *> *disabled = disabled_sources(bundleID, wanted, YES);
    for (int second = 1; second <= 15 && disabled.count > 0; second++) {
        [[NSRunLoop currentRunLoop] runUntilDate:[NSDate dateWithTimeIntervalSinceNow:1.0]];
        disabled = disabled_sources(bundleID, wanted, second % 5 == 0);
    }
    if (disabled.count > 0) {
        for (NSString *sourceID in disabled) {
            fprintf(stderr, "buttre: %s is still not enabled — add it under System Settings → "
                            "Keyboard → Text Input → Edit → (+) → Vietnamese\n",
                    sourceID.UTF8String);
        }
        return 1;
    }
    for (NSString *method in primary_methods()) {
        printf("enabled %s.%s\n", bundleID.UTF8String, method.UTF8String);
    }
    return 0;
}

int main(int argc, const char *argv[]) {
    @autoreleasepool {
        NSBundle *bundle = [NSBundle mainBundle];
        if (argc > 1 && strcmp(argv[1], "--install") == 0) {
            return install_input_sources(bundle);
        }

        NSString *connectionName =
            [bundle.infoDictionary objectForKey:@"InputMethodConnectionName"];
        if (connectionName.length == 0) {
            NSLog(@"buttre: InputMethodConnectionName missing from Info.plist");
            return 1;
        }

        gServer = [[IMKServer alloc] initWithName:connectionName
                                 bundleIdentifier:bundle.bundleIdentifier];
        if (gServer == nil) {
            NSLog(@"buttre: failed to create IMKServer");
            return 1;
        }

        [[NSApplication sharedApplication] run];
    }
    return 0;
}
