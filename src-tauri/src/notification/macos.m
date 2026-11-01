#import <Foundation/Foundation.h>
#import <UserNotifications/UserNotifications.h>

API_AVAILABLE(macos(10.14))
@interface CarambolaNotificationDelegate : NSObject <UNUserNotificationCenterDelegate>
@end

@implementation CarambolaNotificationDelegate
- (void)userNotificationCenter:(UNUserNotificationCenter *)center
       willPresentNotification:(UNNotification *)notification
         withCompletionHandler:(void (^)(UNNotificationPresentationOptions))completionHandler {
    if (@available(macOS 11.0, *)) {
        completionHandler(UNNotificationPresentationOptionBanner | UNNotificationPresentationOptionList);
    } else {
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
        completionHandler(UNNotificationPresentationOptionAlert);
#pragma clang diagnostic pop
    }
}
@end

// The notification center keeps a weak delegate reference. Retain ours for the
// process lifetime and install it before Tauri starts the application loop.
void carambola_install_notification_delegate(void) {
    @autoreleasepool {
        if (![NSBundle mainBundle].bundleIdentifier) return;
        if (@available(macOS 10.14, *)) {
            @try {
                static CarambolaNotificationDelegate *delegate;
                delegate = [CarambolaNotificationDelegate new];
                [UNUserNotificationCenter currentNotificationCenter].delegate = delegate;
                NSLog(@"[Carambola notification] foreground presentation delegate installed");
            } @catch (NSException *exception) {
                NSLog(@"[Carambola notification] delegate unavailable: %@", exception.name);
            }
        }
    }
}

static void carambola_deliver_screenshot_notification(UNUserNotificationCenter *center)
    API_AVAILABLE(macos(10.14));

static void carambola_deliver_screenshot_notification(UNUserNotificationCenter *center) {
    UNMutableNotificationContent *content = [UNMutableNotificationContent new];
    content.title = @"Carambola";
    content.body = @"Screenshot captured successfully.";
    UNNotificationRequest *request = [UNNotificationRequest
        requestWithIdentifier:[[NSUUID UUID] UUIDString]
        content:content
        trigger:nil];
    [center addNotificationRequest:request withCompletionHandler:^(NSError *error) {
        if (error) {
            NSLog(@"[Carambola notification] delivery failed: %@ (%ld)", error.domain, (long)error.code);
        } else {
            NSLog(@"[Carambola notification] request accepted by macOS");
        }
    }];
}

// The callback chain checks the actual macOS authorization state; the Tauri
// desktop plugin reports granted without consulting UserNotifications.
int carambola_notify_screenshot(void) {
    @autoreleasepool {
        if (![NSBundle mainBundle].bundleIdentifier) {
            NSLog(@"[Carambola notification] no app bundle identifier; run a packaged app");
            return 0;
        }
        if (@available(macOS 10.14, *)) {
            @try {
                UNUserNotificationCenter *center = [UNUserNotificationCenter currentNotificationCenter];
                NSLog(@"[Carambola notification] delegate=%@", NSStringFromClass([center.delegate class]));
                [center getNotificationSettingsWithCompletionHandler:^(UNNotificationSettings *settings) {
                    UNAuthorizationStatus status = settings.authorizationStatus;
                    NSLog(@"[Carambola notification] authorization status=%ld alert_setting=%ld alert_style=%ld", (long)status, (long)settings.alertSetting, (long)settings.alertStyle);
                    if (status == UNAuthorizationStatusNotDetermined) {
                        [center requestAuthorizationWithOptions:UNAuthorizationOptionAlert
                                             completionHandler:^(BOOL granted, NSError *error) {
                            if (error) {
                                NSLog(@"[Carambola notification] authorization failed: %@ (%ld)", error.domain, (long)error.code);
                            } else if (granted) {
                                carambola_deliver_screenshot_notification(center);
                            } else {
                                NSLog(@"[Carambola notification] authorization declined");
                            }
                        }];
                    } else if (status == UNAuthorizationStatusAuthorized ||
                               status == UNAuthorizationStatusProvisional) {
                        carambola_deliver_screenshot_notification(center);
                    } else {
                        NSLog(@"[Carambola notification] notifications disabled in macOS settings");
                    }
                }];
                return 1;
            } @catch (NSException *exception) {
                NSLog(@"[Carambola notification] unavailable: %@", exception.name);
                return 0;
            }
        }
        NSLog(@"[Carambola notification] requires macOS 10.14 or later");
        return 0;
    }
}
