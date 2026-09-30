#import <AppKit/AppKit.h>
#import <AVFoundation/AVFoundation.h>
#import <ApplicationServices/ApplicationServices.h>
#import <UniformTypeIdentifiers/UniformTypeIdentifiers.h>

// Status checks do not trigger TCC prompts. Requests only follow a UI action.
int hark_microphone_status(void) {
    return (int)[AVCaptureDevice authorizationStatusForMediaType:AVMediaTypeAudio];
}

void hark_request_microphone(void) {
    [AVCaptureDevice requestAccessForMediaType:AVMediaTypeAudio
                           completionHandler:^(BOOL granted) { (void)granted; }];
}

bool hark_accessibility_status(void) { return AXIsProcessTrusted(); }
bool hark_keyboard_status(void) { return CGPreflightListenEventAccess(); }

void hark_request_accessibility(void) {
    NSDictionary *options = @{(__bridge NSString *)kAXTrustedCheckOptionPrompt: @YES};
    AXIsProcessTrustedWithOptions((__bridge CFDictionaryRef)options);
}

void hark_request_keyboard(void) { CGRequestListenEventAccess(); }

// All callers run in eframe's main-thread callbacks. Native coordinates avoid
// winit's mixed-DPI conversion and preserve screen origins, Dock and menu bar.
void hark_place_overlay(const char *title, bool primary) {
    NSCAssert([NSThread isMainThread], @"AppKit requires the main thread");
    NSString *name = [NSString stringWithUTF8String:title];
    NSWindow *window = nil;
    for (NSWindow *candidate in NSApp.windows) {
        if ([candidate.title isEqualToString:name]) { window = candidate; break; }
    }
    if (!window) return;
    NSScreen *screen = NSScreen.screens.firstObject;
    if (!primary) {
        NSPoint pointer = NSEvent.mouseLocation;
        for (NSScreen *candidate in NSScreen.screens) {
            if (NSPointInRect(pointer, candidate.frame)) { screen = candidate; break; }
        }
    }
    if (!screen) return;
    NSRect work = screen.visibleFrame;
    NSRect frame = window.frame;
    CGFloat x = NSMidX(work) - NSWidth(frame) / 2;
    CGFloat y = NSMinY(work) + 24;
    if (fabs(frame.origin.x - x) > 0.5 || fabs(frame.origin.y - y) > 0.5)
        [window setFrameOrigin:NSMakePoint(x, y)];
    window.level = NSFloatingWindowLevel;
    window.hidesOnDeactivate = NO;
    window.collectionBehavior |= NSWindowCollectionBehaviorCanJoinAllSpaces |
                                 NSWindowCollectionBehaviorFullScreenAuxiliary;
    if (!primary) window.ignoresMouseEvents = YES;
}

// winit's Visible(true) calls makeKeyAndOrderFront on macOS, even for a
// window created with active=false. Overlay visibility must bypass that path.
bool hark_overlay_visible(const char *title, bool visible) {
    NSCAssert([NSThread isMainThread], @"AppKit requires the main thread");
    NSString *name = [NSString stringWithUTF8String:title];
    for (NSWindow *window in NSApp.windows) {
        if (![window.title isEqualToString:name]) continue;
        if (visible && !window.visible) [window orderFrontRegardless];
        if (!visible && window.visible) [window orderOut:nil];
        return true;
    }
    return false;
}

// One retained picker per app, replaced when another share is requested.
// It must survive beyond the call while the user chooses a destination.
static NSSharingServicePicker *hark_picker;
bool hark_share_text(const char *text) {
    NSCAssert([NSThread isMainThread], @"AppKit requires the main thread");
    NSWindow *window = nil;
    for (NSWindow *candidate in NSApp.windows) {
        if ([candidate.title isEqualToString:@"Hark"]) { window = candidate; break; }
    }
    if (!window.contentView) return false;
    NSString *body = [NSString stringWithUTF8String:text];
    if (!body) return false;
    hark_picker = [[NSSharingServicePicker alloc] initWithItems:@[body]];
    NSView *view = window.contentView;
    NSRect anchor = NSMakeRect(NSMidX(view.bounds), NSMidY(view.bounds), 1, 1);
    [hark_picker showRelativeToRect:anchor ofView:view preferredEdge:NSMinYEdge];
    return true;
}

// Only the export worker waits. The main thread presents a sheet and returns
// to eframe so recording feedback remains responsive while a save is open.
char *hark_save_file(const char *name, const char *extension) {
    @autoreleasepool {
    NSCAssert(![NSThread isMainThread], @"Save must be called from the export worker");
    NSString *fileName = [NSString stringWithUTF8String:name];
    NSString *suffix = [NSString stringWithUTF8String:extension];
    dispatch_semaphore_t finished = dispatch_semaphore_create(0);
    __block char *result = NULL;
    dispatch_async(dispatch_get_main_queue(), ^{
        NSWindow *parent = nil;
        for (NSWindow *candidate in NSApp.windows) {
            if ([candidate.title isEqualToString:@"Hark"]) { parent = candidate; break; }
        }
        if (!parent) { dispatch_semaphore_signal(finished); return; }
        NSSavePanel *panel = [NSSavePanel savePanel];
        panel.nameFieldStringValue = fileName;
        UTType *type = [UTType typeWithFilenameExtension:suffix];
        if (type) panel.allowedContentTypes = @[type];
        panel.canCreateDirectories = YES;
        [panel beginSheetModalForWindow:parent completionHandler:^(NSModalResponse response) {
            if (response == NSModalResponseOK) result = strdup(panel.URL.path.fileSystemRepresentation);
            dispatch_semaphore_signal(finished);
        }];
    });
    dispatch_semaphore_wait(finished, DISPATCH_TIME_FOREVER);
    return result;
    }
}

void hark_free_path(char *path) { free(path); }
