// Core Audio process taps (macOS 14.2+). No allocation, Objective-C messaging,
// locks or system calls in the audio callback; ownership stays with Rust's
// capture thread and AudioDeviceStop joins callbacks before their context dies.
#import <Foundation/Foundation.h>
#import <CoreAudio/CoreAudio.h>
#import <CoreAudio/AudioHardwareTapping.h>
#import <CoreAudio/CATapDescription.h>
#import <CoreGraphics/CoreGraphics.h>
#include <libproc.h>
#include <mach/mach_time.h>
#include <stdatomic.h>
#include <stdlib.h>

static AudioObjectPropertyAddress address(AudioObjectPropertySelector selector) {
    return (AudioObjectPropertyAddress){selector, kAudioObjectPropertyScopeGlobal,
                                       kAudioObjectPropertyElementMain};
}
static OSStatus read_property(AudioObjectID object, AudioObjectPropertySelector selector,
                              void *value, UInt32 size) {
    AudioObjectPropertyAddress a = address(selector);
    return AudioObjectGetPropertyData(object, &a, 0, NULL, &size, value);
}
static NSArray<NSNumber *> *audio_processes(OSStatus *error) {
    AudioObjectPropertyAddress a = address(kAudioHardwarePropertyProcessObjectList);
    UInt32 size = 0;
    *error = AudioObjectGetPropertyDataSize(kAudioObjectSystemObject, &a, 0, NULL, &size);
    if (*error) return nil;
    NSMutableData *data = [NSMutableData dataWithLength:size];
    *error = AudioObjectGetPropertyData(kAudioObjectSystemObject, &a, 0, NULL, &size,
                                      data.mutableBytes);
    if (*error) return nil;
    NSMutableArray *ids = [NSMutableArray array];
    AudioObjectID *objects = data.mutableBytes;
    for (UInt32 i = 0; i < size / sizeof(AudioObjectID); i++) [ids addObject:@(objects[i])];
    return ids;
}
static pid_t parent_pid(pid_t pid) {
    struct proc_bsdinfo info = {0};
    if (proc_pidinfo(pid, PROC_PIDTBSDINFO, 0, &info, sizeof(info)) != sizeof(info)) return 0;
    return (pid_t)info.pbi_ppid;
}
static BOOL in_tree(pid_t pid, pid_t root) {
    for (int depth = 0; pid > 1 && depth < 128; depth++) {
        if (pid == root) return YES;
        pid_t parent = parent_pid(pid);
        if (parent == pid) break;
        pid = parent;
    }
    return NO;
}
// Resolve helpers (Chrome Helper, WebKit, Electron) to their outer .app bundle.
// The process bundle alone identifies the helper, not the app users configured.
static NSString *app_id(pid_t pid) {
    char path[PROC_PIDPATHINFO_MAXSIZE] = {0};
    if (proc_pidpath(pid, path, sizeof(path)) <= 0) return nil;
    NSString *exe = [NSString stringWithUTF8String:path];
    NSRange app = [exe rangeOfString:@".app/"];
    if (app.location != NSNotFound) {
        NSString *bundle = [exe substringToIndex:app.location + 4];
        NSString *identifier = [NSBundle bundleWithPath:bundle].bundleIdentifier;
        if (identifier.length) return identifier.lowercaseString;
    }
    return exe.lastPathComponent.lowercaseString;
}

bool hark_mac_audio_supported(void) {
    if (@available(macOS 14.2, *)) return true;
    return false;
}

typedef void (*HarkSamples)(void *, const float *, uint32_t, double, uint64_t);
typedef struct {
    AudioObjectID tap;
    CFTypeRef description;
    AudioDeviceID device;
    AudioDeviceIOProcID io;
    HarkSamples callback;
    void *context;
    mach_timebase_info_data_t timebase;
    pid_t root;
    uint64_t root_start_sec;
    uint64_t root_start_usec;
    double rate;
    _Atomic bool failed;
} HarkTap;

static OSStatus audio_callback(AudioDeviceID device, const AudioTimeStamp *now,
                               const AudioBufferList *input, const AudioTimeStamp *inputTime,
                               AudioBufferList *output, const AudioTimeStamp *outputTime,
                               void *context) {
    (void)device; (void)now; (void)output; (void)outputTime;
    HarkTap *tap = context;
    if (!input || input->mNumberBuffers != 1) {
        atomic_store_explicit(&tap->failed, true, memory_order_relaxed);
        return noErr;
    }
    const AudioBuffer *buffer = &input->mBuffers[0];
    if (buffer->mNumberChannels != 1 || buffer->mDataByteSize % sizeof(float)) {
        atomic_store_explicit(&tap->failed, true, memory_order_relaxed);
        return noErr;
    }
    uint64_t ns = 0;
    if (inputTime->mFlags & kAudioTimeStampHostTimeValid)
        ns = (uint64_t)(((__uint128_t)inputTime->mHostTime * tap->timebase.numer) / tap->timebase.denom);
    double sample = (inputTime->mFlags & kAudioTimeStampSampleTimeValid) ? inputTime->mSampleTime : -1;
    tap->callback(tap->context, buffer->mData, buffer->mDataByteSize / sizeof(float), sample, ns);
    return noErr;
}

void hark_mac_tap_close(void *opaque) {
    if (!opaque) return;
    HarkTap *tap = opaque;
    if (tap->io) {
        AudioDeviceStop(tap->device, tap->io);
        AudioDeviceDestroyIOProcID(tap->device, tap->io);
    }
    if (tap->device) AudioHardwareDestroyAggregateDevice(tap->device);
    if (@available(macOS 14.2, *)) {
        if (tap->tap) AudioHardwareDestroyProcessTap(tap->tap);
    }
    if (tap->description) CFRelease(tap->description);
    free(tap);
}

static NSArray<NSNumber *> *tree_objects(pid_t root, OSStatus *error) {
    NSMutableArray *selected = [NSMutableArray array];
    NSArray *objects = audio_processes(error);
    if (*error) return nil;
    for (NSNumber *object in objects) {
        pid_t pid = 0;
        if (!read_property(object.unsignedIntValue, kAudioProcessPropertyPID, &pid, sizeof(pid))
            && in_tree(pid, root)) [selected addObject:object];
    }
    return selected;
}

// Split open/start so Rust can size the ring from the negotiated rate and put
// its callback context at a stable address before Core Audio can invoke it.
void *hark_mac_tap_open(uint32_t root, bool exclude, uint32_t *rate, int32_t *error) {
    @autoreleasepool {
        if (@available(macOS 14.2, *)) {
            *error = noErr;
            NSArray *selected = tree_objects((pid_t)root, error);
            if (*error) return NULL;
            if (!exclude && !selected.count) { *error = kAudioHardwareBadObjectError; return NULL; }
            CATapDescription *description = exclude
                ? [[CATapDescription alloc] initMonoGlobalTapButExcludeProcesses:selected]
                : [[CATapDescription alloc] initMonoMixdownOfProcesses:selected];
            description.name = @"Hark meeting audio";
            description.privateTap = YES;
            description.muteBehavior = CATapUnmuted;
            HarkTap *tap = calloc(1, sizeof(HarkTap));
            if (!tap) { *error = kAudioHardwareUnspecifiedError; return NULL; }
            tap->root = (pid_t)root;
            struct proc_bsdinfo root_info = {0};
            if (proc_pidinfo(tap->root, PROC_PIDTBSDINFO, 0, &root_info, sizeof(root_info)) != sizeof(root_info)) {
                *error = kAudioHardwareBadObjectError;
                hark_mac_tap_close(tap); return NULL;
            }
            tap->root_start_sec = root_info.pbi_start_tvsec;
            tap->root_start_usec = root_info.pbi_start_tvusec;
            mach_timebase_info(&tap->timebase);
            // Own the original description explicitly; property getters do
            // not document CATapDescription retain ownership uniformly.
            tap->description = CFBridgingRetain(description);
            *error = AudioHardwareCreateProcessTap(description, &tap->tap);
            if (*error) { hark_mac_tap_close(tap); return NULL; }
            AudioStreamBasicDescription format = {0};
            *error = read_property(tap->tap, kAudioTapPropertyFormat, &format, sizeof(format));
            if (*error || format.mFormatID != kAudioFormatLinearPCM ||
                !(format.mFormatFlags & kAudioFormatFlagIsFloat) ||
                format.mBitsPerChannel != 32 || format.mChannelsPerFrame != 1 ||
                format.mSampleRate < 8000 || format.mSampleRate > 192000) {
                if (!*error) *error = kAudioDeviceUnsupportedFormatError;
                hark_mac_tap_close(tap); return NULL;
            }
            tap->rate = format.mSampleRate;
            *rate = (uint32_t)format.mSampleRate;
            NSDictionary *aggregate = @{
                @kAudioAggregateDeviceNameKey: @"Hark private meeting capture",
                @kAudioAggregateDeviceUIDKey: NSUUID.UUID.UUIDString,
                @kAudioAggregateDeviceIsPrivateKey: @YES,
                @kAudioAggregateDeviceTapAutoStartKey: @YES,
                @kAudioAggregateDeviceTapListKey: @[@{
                    @kAudioSubTapUIDKey: description.UUID.UUIDString,
                    @kAudioSubTapDriftCompensationKey: @YES
                }]
            };
            *error = AudioHardwareCreateAggregateDevice((__bridge CFDictionaryRef)aggregate, &tap->device);
            if (*error) { hark_mac_tap_close(tap); return NULL; }
            return tap;
        }
        *error = kAudioHardwareUnsupportedOperationError;
        return NULL;
    }
}
int32_t hark_mac_tap_start(void *opaque, HarkSamples callback, void *context) {
    HarkTap *tap = opaque;
    tap->callback = callback;
    tap->context = context;
    OSStatus error = AudioDeviceCreateIOProcID(tap->device, audio_callback, tap, &tap->io);
    if (!error) error = AudioDeviceStart(tap->device, tap->io);
    return error;
}
// Called from the owner, never the IO thread. Refresh process membership so
// media children spawned or restarted during a call remain in the capture.
int32_t hark_mac_tap_refresh(void *opaque) {
    @autoreleasepool {
        HarkTap *tap = opaque;
        if (atomic_load_explicit(&tap->failed, memory_order_relaxed)) return kAudioDeviceUnsupportedFormatError;
        // A recycled PID must never switch the tap to an unrelated app.
        struct proc_bsdinfo root_info = {0};
        if (proc_pidinfo(tap->root, PROC_PIDTBSDINFO, 0, &root_info, sizeof(root_info)) != sizeof(root_info) ||
            root_info.pbi_start_tvsec != tap->root_start_sec ||
            root_info.pbi_start_tvusec != tap->root_start_usec) return kAudioHardwareBadObjectError;
        OSStatus error = 0;
        NSArray *selected = tree_objects(tap->root, &error);
        if (error) return error;
        CATapDescription *description = (__bridge CATapDescription *)tap->description;
        if (![description.processes isEqualToArray:selected]) {
            description.processes = selected;
            AudioObjectPropertyAddress a = address(kAudioTapPropertyDescription);
            error = AudioObjectSetPropertyData(tap->tap, &a, 0, NULL, sizeof(description), &description);
        }
        AudioStreamBasicDescription format = {0};
        if (!error) error = read_property(tap->tap, kAudioTapPropertyFormat, &format, sizeof(format));
        if (!error && format.mSampleRate != tap->rate) error = kAudioDeviceUnsupportedFormatError;
        return error;
    }
}

typedef void (*HarkProcess)(void *, uint32_t, uint32_t, const char *, bool);
typedef void (*HarkWindow)(void *, const char *, const char *);
int32_t hark_mac_audio_snapshot(void *context, HarkProcess process, HarkWindow window) {
    @autoreleasepool {
        OSStatus error = 0;
        NSArray *objects = audio_processes(&error);
        if (error) return error;
        for (NSNumber *object in objects) {
            pid_t pid = 0;
            UInt32 input = 0;
            if (read_property(object.unsignedIntValue, kAudioProcessPropertyPID, &pid, sizeof(pid)) ||
                in_tree(pid, getpid()) ||
                read_property(object.unsignedIntValue, kAudioProcessPropertyIsRunningInput, &input, sizeof(input))) continue;
            NSString *identifier = app_id(pid);
            if (identifier) {
                // Audio process lists often contain only a media helper. Use
                // the outer application root so include-tree capture covers
                // its sibling output process as well as its microphone owner.
                pid_t root = pid;
                for (int depth = 0; depth < 128; depth++) {
                    pid_t parent = parent_pid(root);
                    if (parent <= 1 || parent == root || ![app_id(parent) isEqualToString:identifier]) break;
                    root = parent;
                }
                process(context, (uint32_t)root, (uint32_t)parent_pid(root), identifier.UTF8String, input != 0);
            }
        }
        // Window titles are privacy-sensitive: examine only in memory and
        // never emit them to logs. Without OS permission titles are omitted.
        if (window) {
            CFArrayRef list = CGWindowListCopyWindowInfo(kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements, kCGNullWindowID);
            for (NSDictionary *item in (__bridge NSArray *)list) {
                NSString *title = item[(id)kCGWindowName];
                if (!title.length) continue;
                NSString *identifier = app_id([item[(id)kCGWindowOwnerPID] intValue]);
                if (identifier) window(context, identifier.UTF8String, title.UTF8String);
            }
            if (list) CFRelease(list);
        }
        return noErr;
    }
}
