// buttre macOS IMKit host — entry point.
//
// Bootstraps the IMKit server: IMKit reads InputMethodConnectionName and
// InputMethodServerControllerClass from Info.plist, then instantiates one
// ButtreInputController per input session and routes key events to it.
//
// The server object must outlive the run loop, so it is a global — releasing
// it would tear the input method off the mach port the moment it was built.

#import <Cocoa/Cocoa.h>
#import <InputMethodKit/InputMethodKit.h>

static IMKServer *gServer = nil;

int main(int argc, const char *argv[]) {
    @autoreleasepool {
        NSBundle *bundle = [NSBundle mainBundle];
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
