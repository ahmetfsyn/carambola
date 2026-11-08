#import <CoreGraphics/CoreGraphics.h>
#import <Foundation/Foundation.h>
#import <Vision/Vision.h>
#include <stdint.h>
#include <math.h>
#include <string.h>

int carambola_vision_ocr(const uint8_t *rgba,
                         uint32_t width,
                         uint32_t height,
                         char *output,
                         size_t output_capacity) {
    if (rgba == NULL || width == 0 || height == 0 || output == NULL || output_capacity == 0) {
        return -1;
    }
    output[0] = '\0';

    size_t byte_count = (size_t)width * (size_t)height * 4;
    NSData *pixels = [NSData dataWithBytes:rgba length:byte_count];
    CGDataProviderRef provider = CGDataProviderCreateWithCFData((__bridge CFDataRef)pixels);
    CGColorSpaceRef color_space = CGColorSpaceCreateDeviceRGB();
    if (provider == NULL || color_space == NULL) {
        if (provider != NULL) CGDataProviderRelease(provider);
        if (color_space != NULL) CGColorSpaceRelease(color_space);
        return -1;
    }

    CGImageRef image = CGImageCreate(width,
                                     height,
                                     8,
                                     32,
                                     (size_t)width * 4,
                                     color_space,
                                     kCGBitmapByteOrder32Big | kCGImageAlphaLast,
                                     provider,
                                     NULL,
                                     false,
                                     kCGRenderingIntentDefault);
    CGColorSpaceRelease(color_space);
    CGDataProviderRelease(provider);
    if (image == NULL) return -1;

    VNRecognizeTextRequest *request = [[VNRecognizeTextRequest alloc] init];
    request.recognitionLevel = VNRecognizeTextRequestRecognitionLevelAccurate;
    request.usesLanguageCorrection = YES;
    VNImageRequestHandler *handler = [[VNImageRequestHandler alloc] initWithCGImage:image options:@{}];
    NSError *error = nil;
    BOOL succeeded = [handler performRequests:@[request] error:&error];
    CGImageRelease(image);
    if (!succeeded) return -2;

    NSArray<VNRecognizedTextObservation *> *observations =
        (NSArray<VNRecognizedTextObservation *> *)request.results;
    NSArray<VNRecognizedTextObservation *> *ordered = [observations sortedArrayUsingComparator:
        ^NSComparisonResult(VNRecognizedTextObservation *left, VNRecognizedTextObservation *right) {
            CGFloat vertical_delta = CGRectGetMidY(right.boundingBox) - CGRectGetMidY(left.boundingBox);
            if (fabs(vertical_delta) > 0.002) return vertical_delta > 0 ? NSOrderedAscending : NSOrderedDescending;
            CGFloat horizontal_delta = CGRectGetMinX(left.boundingBox) - CGRectGetMinX(right.boundingBox);
            if (horizontal_delta < 0) return NSOrderedAscending;
            if (horizontal_delta > 0) return NSOrderedDescending;
            return NSOrderedSame;
        }];

    NSMutableString *recognized = [NSMutableString string];
    for (VNRecognizedTextObservation *observation in ordered) {
        VNRecognizedText *candidate = [[observation topCandidates:1] firstObject];
        if (candidate == nil || candidate.string.length == 0) continue;
        if (recognized.length > 0) [recognized appendString:@"\n"];
        [recognized appendString:candidate.string];
    }

    const char *utf8 = recognized.UTF8String;
    if (utf8 == NULL) return 0;
    size_t length = strlen(utf8);
    if (length >= output_capacity) {
        memcpy(output, utf8, output_capacity - 1);
        output[output_capacity - 1] = '\0';
        return -3;
    }
    memcpy(output, utf8, length + 1);
    return 0;
}
