#include <stdbool.h>
#import <AppKit/AppKit.h>
#import <AVFoundation/AVFoundation.h>
#import <ApplicationServices/ApplicationServices.h>

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
void conch_overlay_visibility(void *window, bool visible) {
    NSWindow *panel = (__bridge NSWindow *)window;
    panel.ignoresMouseEvents = YES;
    panel.level = NSFloatingWindowLevel;
    panel.collectionBehavior = NSWindowCollectionBehaviorCanJoinAllSpaces | NSWindowCollectionBehaviorFullScreenAuxiliary;
    if (visible) [panel orderFrontRegardless]; else [panel orderOut:nil];
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
