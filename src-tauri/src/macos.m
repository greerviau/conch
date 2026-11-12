#include <stdbool.h>
#import <AppKit/AppKit.h>
#import <AVFoundation/AVFoundation.h>
#import <ApplicationServices/ApplicationServices.h>
#import <objc/message.h>

int conch_microphone_status(void) {
    return (int)[AVCaptureDevice authorizationStatusForMediaType:AVMediaTypeAudio];
}
int conch_request_microphone(void) {
    if (conch_microphone_status() != AVAuthorizationStatusNotDetermined) return conch_microphone_status();
    dispatch_semaphore_t completed = dispatch_semaphore_create(0);
    [AVCaptureDevice requestAccessForMediaType:AVMediaTypeAudio completionHandler:^(BOOL granted) { (void)granted; dispatch_semaphore_signal(completed); }];
    dispatch_semaphore_wait(completed, dispatch_time(DISPATCH_TIME_NOW, 45 * NSEC_PER_SEC));
    return conch_microphone_status();
}
void conch_request_accessibility(void) {
    AXIsProcessTrustedWithOptions((__bridge CFDictionaryRef)@{(__bridge NSString *)kAXTrustedCheckOptionPrompt: @YES});
    [[NSWorkspace sharedWorkspace] openURL:[NSURL URLWithString:@"x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"]];
}
void conch_open_microphone_settings(void) {
    [[NSWorkspace sharedWorkspace] openURL:[NSURL URLWithString:@"x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone"]];
}
int conch_frontmost_pid(void) {
    NSRunningApplication *application = NSWorkspace.sharedWorkspace.frontmostApplication;
    return application ? application.processIdentifier : -1;
}
void conch_frontmost_name(char *buffer, int size) {
    NSString *name = NSWorkspace.sharedWorkspace.frontmostApplication.localizedName;
    if (size > 0) buffer[0] = 0;
    if (name) [name getCString:buffer maxLength:size encoding:NSUTF8StringEncoding];
}
void conch_overlay_visibility(void *window, bool visible) {
    NSWindow *panel = (__bridge NSWindow *)window;
    panel.ignoresMouseEvents = YES;
    panel.level = NSFloatingWindowLevel;
    panel.collectionBehavior = NSWindowCollectionBehaviorCanJoinAllSpaces | NSWindowCollectionBehaviorFullScreenAuxiliary;
    // The indicator is a pill: clear the window and round its content view.
    panel.opaque = NO;
    panel.backgroundColor = NSColor.clearColor;
    panel.hasShadow = NO;
    NSView *content = panel.contentView;
    content.wantsLayer = YES;
    content.layer.cornerRadius = NSHeight(content.bounds) / 2.0;
    content.layer.masksToBounds = YES;
    if (visible) [panel orderFrontRegardless]; else [panel orderOut:nil];
}

// Lets the indicator take clicks while it lists profiles. Clicks must not
// activate Conch, or the app being dictated into loses its focused field.
void conch_overlay_interactive(void *window, bool interactive) {
    NSWindow *panel = (__bridge NSWindow *)window;
    panel.ignoresMouseEvents = !interactive;
    SEL prevents = NSSelectorFromString(@"_setPreventsActivation:");
    if ([panel respondsToSelector:prevents]) {
        ((void (*)(id, SEL, BOOL))objc_msgSend)(panel, prevents, interactive);
    }
}
// Activates the application with `pid` again when Conch became active.
void conch_return_focus(int pid) {
    if (pid <= 0 || !NSApp.isActive) return;
    [[NSRunningApplication runningApplicationWithProcessIdentifier:pid] activateWithOptions:0];
}

int conch_overlay_focus_test(void) {
    @autoreleasepool {
        NSApplication *application = [NSApplication sharedApplication];
        [application setActivationPolicy:NSApplicationActivationPolicyAccessory];
        pid_t before = NSWorkspace.sharedWorkspace.frontmostApplication.processIdentifier;
        NSWindow *panel = [[NSWindow alloc] initWithContentRect:NSMakeRect(50, 50, 320, 68) styleMask:NSWindowStyleMaskBorderless backing:NSBackingStoreBuffered defer:NO];
        panel.releasedWhenClosed = NO;
        conch_overlay_visibility((__bridge void *)panel, true);
        [[NSRunLoop currentRunLoop] runUntilDate:[NSDate dateWithTimeIntervalSinceNow:0.15]];
        pid_t after = NSWorkspace.sharedWorkspace.frontmostApplication.processIdentifier;
        BOOL passed = before == after && !panel.isKeyWindow && panel.ignoresMouseEvents && panel.isVisible;
        conch_overlay_visibility((__bridge void *)panel, false);
        [panel close];
        return passed ? 0 : 1;
    }
}

int conch_screen_geometry(double *values) {
    NSScreen *screen = NSScreen.screens.firstObject;
    if (!screen) return 1;
    NSRect frame = screen.frame, visible = screen.visibleFrame;
    double geometry[] = {frame.origin.x, frame.origin.y, frame.size.width, frame.size.height, visible.origin.x, visible.origin.y, visible.size.width, visible.size.height, screen.backingScaleFactor};
    memcpy(values, geometry, sizeof(geometry));
    return 0;
}
int conch_overlay_bounds_test(double x, double y, double width, double height) {
    @autoreleasepool {
        NSScreen *screen = NSScreen.screens.firstObject;
        if (!screen) return 1;
        double scale = screen.backingScaleFactor;
        NSRect frame = NSMakeRect(screen.frame.origin.x + x / scale, NSMaxY(screen.frame) - (y + height) / scale, width / scale, height / scale);
        pid_t before = NSWorkspace.sharedWorkspace.frontmostApplication.processIdentifier;
        NSWindow *panel = [[NSWindow alloc] initWithContentRect:frame styleMask:NSWindowStyleMaskBorderless backing:NSBackingStoreBuffered defer:NO];
        panel.releasedWhenClosed = NO;
        conch_overlay_visibility((__bridge void *)panel, true);
        [[NSRunLoop currentRunLoop] runUntilDate:[NSDate dateWithTimeIntervalSinceNow:0.15]];
        BOOL passed = NSContainsRect(screen.visibleFrame, panel.frame) && NSMinY(panel.frame) >= NSMinY(screen.visibleFrame) + 23.5 && !panel.isKeyWindow && before == NSWorkspace.sharedWorkspace.frontmostApplication.processIdentifier;
        conch_overlay_visibility((__bridge void *)panel, false);
        [panel close];
        return passed ? 0 : 1;
    }
}
