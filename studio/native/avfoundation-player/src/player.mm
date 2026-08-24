#include <node_api.h>
#include <cmath>
#include <atomic>
#include <string>
#import <AppKit/AppKit.h>
#import <AVFoundation/AVFoundation.h>
#import <VideoToolbox/VideoToolbox.h>

static NSView *gHostView = nil;
static NSView *gPlayerView = nil;
static AVPlayer *gPlayer = nil;
static AVPlayerLayer *gLayer = nil;
static AVAsset *gCurrentAsset = nil;
static NSArray<AVAsset *> *gSourceAssets = nil;
static id gTimeObserver = nil;
static id gEndObserver = nil;
static double gObservedSeconds = 0;
static BOOL gEnded = NO;
static CATextLayer *gSubtitleLayer = nil;
static CATextLayer *gStatusLayer = nil;
static NSDictionary *gSubtitleConfig = nil;
static NSDictionary *gPresentationConfig = nil;
static NSString *gError = nil;
static BOOL gRequestedPlaying = NO;
static BOOL gLoaded = NO;
static BOOL gPrerolling = NO;
static std::atomic<uint64_t> gLoadGeneration{0};
static void StartPlayback() {
  if (!gPlayer || !gRequestedPlaying || gPrerolling || gPlayer.status != AVPlayerStatusReadyToPlay) return;
  AVPlayer *player = gPlayer;
  gPrerolling = YES;
  [player prerollAtRate:1.0 completionHandler:^(BOOL finished) {
    dispatch_async(dispatch_get_main_queue(), ^{
      gPrerolling = NO;
      if (finished && gRequestedPlaying && gPlayer == player) [player playImmediatelyAtRate:1.0];
    });
  }];
}
static void ObservePlayer() {
  if (gTimeObserver && gPlayer) [gPlayer removeTimeObserver:gTimeObserver];
  if (gEndObserver) [[NSNotificationCenter defaultCenter] removeObserver:gEndObserver];
  gObservedSeconds = 0;
  gEnded = NO;
  AVPlayer *player = gPlayer;
  gTimeObserver = [player addPeriodicTimeObserverForInterval:CMTimeMake(1, 120) queue:dispatch_get_main_queue() usingBlock:^(CMTime time) {
    if (gPlayer == player && CMTIME_IS_NUMERIC(time)) gObservedSeconds = CMTimeGetSeconds(time);
  }];
  gEndObserver = [[NSNotificationCenter defaultCenter] addObserverForName:AVPlayerItemDidPlayToEndTimeNotification object:player.currentItem queue:NSOperationQueue.mainQueue usingBlock:^(NSNotification *note) {
    if (gPlayer == player) { gEnded = YES; gRequestedPlaying = NO; }
  }];
}

static napi_value Undefined(napi_env env) { napi_value v; napi_get_undefined(env, &v); return v; }
static double NumberArg(napi_env env, napi_value v) { double n = 0; napi_get_value_double(env, v, &n); return n; }
static std::string StringArg(napi_env env, napi_value v) {
  size_t size = 0; napi_get_value_string_utf8(env, v, nullptr, 0, &size);
  std::string out(size, '\0'); napi_get_value_string_utf8(env, v, out.data(), size + 1, &size); return out;
}
static void SetString(napi_env env, napi_value object, const char *key, NSString *value) {
  napi_value text; napi_create_string_utf8(env, value.UTF8String ?: "", NAPI_AUTO_LENGTH, &text); napi_set_named_property(env, object, key, text);
}
static NSArray<AVAssetTrack *> *LoadTracks(AVAsset *asset, AVMediaType type, NSError **error) {
  __block NSArray<AVAssetTrack *> *loaded = nil;
  __block NSError *loadError = nil;
  dispatch_semaphore_t semaphore = dispatch_semaphore_create(0);
  [asset loadTracksWithMediaType:type completionHandler:^(NSArray<AVAssetTrack *> *tracks, NSError *trackError) {
    loaded = tracks;
    loadError = trackError;
    dispatch_semaphore_signal(semaphore);
  }];
  if (dispatch_semaphore_wait(semaphore, dispatch_time(DISPATCH_TIME_NOW, 30 * NSEC_PER_SEC)) != 0) {
    if (error) *error = [NSError errorWithDomain:@"DoubleLovePlayer" code:1 userInfo:@{NSLocalizedDescriptionKey: @"读取媒体轨道超时"}];
    return @[];
  }
  if (error) *error = loadError;
  return loaded ?: @[];
}
static NSColor *Color(NSString *value, NSColor *fallback) {
  if (![value isKindOfClass:NSString.class] || ![value hasPrefix:@"#"] || (value.length != 7 && value.length != 9)) return fallback;
  unsigned int rgba = 0;
  if (![[NSScanner scannerWithString:[value substringFromIndex:1]] scanHexInt:&rgba]) return fallback;
  CGFloat red, green, blue, alpha = 1;
  if (value.length == 9) {
    red = ((rgba >> 24) & 0xff) / 255.0; green = ((rgba >> 16) & 0xff) / 255.0; blue = ((rgba >> 8) & 0xff) / 255.0; alpha = (rgba & 0xff) / 255.0;
  } else {
    red = ((rgba >> 16) & 0xff) / 255.0; green = ((rgba >> 8) & 0xff) / 255.0; blue = (rgba & 0xff) / 255.0;
  }
  return [NSColor colorWithRed:red green:green blue:blue alpha:alpha];
}
static void ApplySubtitle() {
  if (!gSubtitleLayer || !gPlayerView) return;
  NSString *text = gSubtitleConfig[@"text"];
  if (![text isKindOfClass:NSString.class] || text.length == 0) { gSubtitleLayer.hidden = YES; return; }
  gSubtitleLayer.hidden = NO;
  CGFloat canvasWidth = MAX(1, [gSubtitleConfig[@"canvasWidth"] doubleValue]);
  CGFloat scale = NSWidth(gPlayerView.bounds) / canvasWidth;
  CGFloat fontSize = MAX(10, [gSubtitleConfig[@"fontSize"] doubleValue] * scale);
  CGFloat paddingY = [gSubtitleConfig[@"paddingY"] doubleValue] * scale;
  CGFloat maxWidth = NSWidth(gPlayerView.bounds) * MAX(0.1, MIN(1, [gSubtitleConfig[@"maxWidth"] doubleValue]));
  CGFloat boxHeight = MIN(NSHeight(gPlayerView.bounds), fontSize * 3.2 + paddingY * 2);
  CGFloat centerX = NSWidth(gPlayerView.bounds) * [gSubtitleConfig[@"x"] doubleValue];
  CGFloat centerY = NSHeight(gPlayerView.bounds) * (1 - [gSubtitleConfig[@"y"] doubleValue]);
  gSubtitleLayer.frame = NSMakeRect(centerX - maxWidth / 2, centerY - boxHeight / 2, maxWidth, boxHeight);
  NSFont *font = [NSFont fontWithName:gSubtitleConfig[@"fontFamily"] size:fontSize] ?: [NSFont systemFontOfSize:fontSize weight:NSFontWeightSemibold];
  NSShadow *shadow = [[NSShadow alloc] init];
  shadow.shadowColor = Color(gSubtitleConfig[@"shadowColor"], NSColor.clearColor);
  shadow.shadowOffset = NSMakeSize([gSubtitleConfig[@"shadowX"] doubleValue] * scale, -[gSubtitleConfig[@"shadowY"] doubleValue] * scale);
  shadow.shadowBlurRadius = [gSubtitleConfig[@"shadowBlur"] doubleValue] * scale;
  CGFloat outline = [gSubtitleConfig[@"outlineWidth"] doubleValue] * scale;
  NSDictionary *attributes = @{
    NSFontAttributeName: font,
    NSForegroundColorAttributeName: Color(gSubtitleConfig[@"textColor"], NSColor.whiteColor),
    NSStrokeColorAttributeName: Color(gSubtitleConfig[@"outlineColor"], NSColor.blackColor),
    NSStrokeWidthAttributeName: @(-MAX(0, outline / fontSize * 100)),
    NSShadowAttributeName: shadow,
  };
  gSubtitleLayer.string = [[NSAttributedString alloc] initWithString:text attributes:attributes];
  gSubtitleLayer.backgroundColor = Color(gSubtitleConfig[@"backgroundColor"], NSColor.clearColor).CGColor;
  gSubtitleLayer.cornerRadius = [gSubtitleConfig[@"radius"] doubleValue] * scale;
}
static void ApplyPresentation() {
  if (!gLayer || !gPlayerView) return;
  [gLayer setAffineTransform:CGAffineTransformIdentity];
  gLayer.frame = gPlayerView.bounds;
  NSString *fit = gPresentationConfig[@"fit"];
  if ([fit isEqualToString:@"cover"]) gLayer.videoGravity = AVLayerVideoGravityResizeAspectFill;
  else gLayer.videoGravity = AVLayerVideoGravityResizeAspect;
  gLayer.opacity = gPresentationConfig[@"opacity"] ? MAX(0, MIN(1, [gPresentationConfig[@"opacity"] doubleValue])) : 1;
  CGFloat canvasWidth = MAX(1, [gPresentationConfig[@"canvasWidth"] doubleValue]);
  CGFloat canvasHeight = MAX(1, [gPresentationConfig[@"canvasHeight"] doubleValue]);
  CGFloat translateX = [gPresentationConfig[@"positionX"] doubleValue] * NSWidth(gPlayerView.bounds) / canvasWidth;
  CGFloat translateY = -[gPresentationConfig[@"positionY"] doubleValue] * NSHeight(gPlayerView.bounds) / canvasHeight;
  CGFloat scale = gPresentationConfig[@"scale"] ? MAX(0.01, [gPresentationConfig[@"scale"] doubleValue]) : 1;
  CGFloat radians = [gPresentationConfig[@"rotation"] doubleValue] * M_PI / 180.0;
  CGAffineTransform transform = CGAffineTransformMakeTranslation(translateX, translateY);
  transform = CGAffineTransformRotate(transform, radians);
  transform = CGAffineTransformScale(transform, scale, scale);
  [gLayer setAffineTransform:transform];
  gPlayerView.layer.backgroundColor = Color(gPresentationConfig[@"background"], NSColor.blackColor).CGColor;
}
static void LayoutStatus() {
  if (!gStatusLayer || !gPlayerView) return;
  CGFloat height = MIN(56, MAX(0, NSHeight(gPlayerView.bounds)));
  gStatusLayer.frame = NSMakeRect(24, (NSHeight(gPlayerView.bounds) - height) / 2, MAX(0, NSWidth(gPlayerView.bounds) - 48), height);
}
static void ApplyStatus(NSString *text, BOOL isError) {
  if (!gStatusLayer || !gPlayerView) return;
  if (text.length == 0) { gStatusLayer.hidden = YES; return; }
  gStatusLayer.hidden = NO;
  LayoutStatus();
  gStatusLayer.string = text;
  gStatusLayer.font = (__bridge CFTypeRef)[NSFont systemFontOfSize:13 weight:NSFontWeightMedium];
  gStatusLayer.fontSize = 13;
  gStatusLayer.foregroundColor = (isError ? NSColor.systemRedColor : NSColor.whiteColor).CGColor;
  gStatusLayer.backgroundColor = [NSColor colorWithWhite:0 alpha:0.72].CGColor;
  gStatusLayer.cornerRadius = 7;
}
static void EnsurePlayerView() {
  if (!gHostView || gPlayerView) return;
  gPlayerView = [[NSView alloc] initWithFrame:NSZeroRect];
  gPlayerView.wantsLayer = YES;
  gPlayerView.layer.backgroundColor = NSColor.blackColor.CGColor;
  gLayer = [AVPlayerLayer playerLayerWithPlayer:nil];
  gLayer.videoGravity = AVLayerVideoGravityResizeAspect;
  [gPlayerView.layer addSublayer:gLayer];
  gSubtitleLayer = [CATextLayer layer];
  gSubtitleLayer.contentsScale = NSScreen.mainScreen.backingScaleFactor;
  gSubtitleLayer.alignmentMode = kCAAlignmentCenter;
  gSubtitleLayer.wrapped = YES;
  gSubtitleLayer.truncationMode = kCATruncationEnd;
  [gPlayerView.layer addSublayer:gSubtitleLayer];
  gStatusLayer = [CATextLayer layer];
  gStatusLayer.contentsScale = NSScreen.mainScreen.backingScaleFactor;
  gStatusLayer.alignmentMode = kCAAlignmentCenter;
  [gPlayerView.layer addSublayer:gStatusLayer];
  [gHostView addSubview:gPlayerView positioned:NSWindowAbove relativeTo:nil];
}
static napi_value Attach(napi_env env, napi_callback_info info) {
  size_t argc = 1; napi_value argv[1]; napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  void *data = nullptr; size_t length = 0; napi_get_buffer_info(env, argv[0], &data, &length);
  if (length >= sizeof(void *)) { void *pointer = nullptr; memcpy(&pointer, data, sizeof(void *)); gHostView = (__bridge NSView *)pointer; }
  dispatch_async(dispatch_get_main_queue(), ^{ EnsurePlayerView(); });
  return Undefined(env);
}
static napi_value SetFrame(napi_env env, napi_callback_info info) {
  size_t argc = 4; napi_value argv[4]; napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  double x=NumberArg(env,argv[0]), y=NumberArg(env,argv[1]), w=NumberArg(env,argv[2]), h=NumberArg(env,argv[3]);
  dispatch_async(dispatch_get_main_queue(), ^{
    EnsurePlayerView(); if (!gPlayerView || !gHostView) return;
    CGFloat nativeY = NSHeight(gHostView.bounds) - y - h;
    gPlayerView.frame = NSMakeRect(x, nativeY, w, h); ApplyPresentation(); ApplySubtitle(); LayoutStatus();
  });
  return Undefined(env);
}
static napi_value SetSubtitle(napi_env env, napi_callback_info info) {
  size_t argc=1; napi_value argv[1]; napi_get_cb_info(env,info,&argc,argv,nullptr,nullptr);
  std::string encoded=StringArg(env,argv[0]); NSData *data=[[NSData alloc] initWithBytes:encoded.data() length:encoded.size()];
  NSDictionary *config=[NSJSONSerialization JSONObjectWithData:data options:0 error:nil];
  dispatch_async(dispatch_get_main_queue(), ^{ gSubtitleConfig=[config isKindOfClass:NSDictionary.class] ? config : @{}; ApplySubtitle(); });
  return Undefined(env);
}
static napi_value SetPresentation(napi_env env, napi_callback_info info) {
  size_t argc=1; napi_value argv[1]; napi_get_cb_info(env,info,&argc,argv,nullptr,nullptr);
  std::string encoded=StringArg(env,argv[0]); NSData *data=[[NSData alloc] initWithBytes:encoded.data() length:encoded.size()];
  NSDictionary *config=[NSJSONSerialization JSONObjectWithData:data options:0 error:nil];
  dispatch_async(dispatch_get_main_queue(), ^{ gPresentationConfig=[config isKindOfClass:NSDictionary.class] ? config : @{}; ApplyPresentation(); });
  return Undefined(env);
}
static napi_value LoadTimeline(napi_env env, napi_callback_info info) {
  size_t argc = 2; napi_value argv[2]; napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  std::string encoded = StringArg(env, argv[0]); double seconds = argc > 1 ? NumberArg(env, argv[1]) : 0;
  NSData *data = [[NSData alloc] initWithBytes:encoded.data() length:encoded.size()];
  NSError *jsonError = nil;
  NSArray *clips = [NSJSONSerialization JSONObjectWithData:data options:0 error:&jsonError];
  if (jsonError || ![clips isKindOfClass:NSArray.class]) {
    gError = @"播放器时间线无效";
    return Undefined(env);
  }
  uint64_t generation = ++gLoadGeneration;
  dispatch_async(dispatch_get_main_queue(), ^{ gLoaded = NO; gError = nil; gPrerolling = NO; [gPlayer pause]; });
  dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^{
    if (@available(macOS 15.0, *)) VTRegisterProfessionalVideoWorkflowVideoDecoders();
    AVMutableComposition *composition = [AVMutableComposition composition];
    NSMutableArray<AVAsset *> *sourceAssets = [NSMutableArray arrayWithCapacity:clips.count];
    AVMutableCompositionTrack *videoTrack = nil;
    AVMutableCompositionTrack *audioTrack = nil;
    NSError *insertError = nil;
    for (NSDictionary *clip in clips) {
      if (![clip isKindOfClass:NSDictionary.class]) continue;
      NSString *path = clip[@"path"];
      NSNumber *sourceStartValue = clip[@"sourceStartSeconds"];
      NSNumber *sourceDurationValue = clip[@"sourceDurationSeconds"];
      NSNumber *outputStartValue = clip[@"outputStartSeconds"];
      NSNumber *outputDurationValue = clip[@"outputDurationSeconds"];
      if (![path isKindOfClass:NSString.class] || !sourceStartValue || !sourceDurationValue || !outputStartValue || !outputDurationValue) continue;
      AVURLAsset *asset = [AVURLAsset URLAssetWithURL:[NSURL fileURLWithPath:path] options:nil];
      [sourceAssets addObject:asset];
      CMTime sourceStart = CMTimeMakeWithSeconds(MAX(0, sourceStartValue.doubleValue), 120000);
      CMTime sourceDuration = CMTimeMakeWithSeconds(MAX(0, sourceDurationValue.doubleValue), 120000);
      CMTime outputStart = CMTimeMakeWithSeconds(MAX(0, outputStartValue.doubleValue), 120000);
      CMTime outputDuration = CMTimeMakeWithSeconds(MAX(0, outputDurationValue.doubleValue), 120000);
      CMTimeRange sourceRange = CMTimeRangeMake(sourceStart, sourceDuration);
      NSError *videoLoadError = nil;
      NSArray<AVAssetTrack *> *videos = LoadTracks(asset, AVMediaTypeVideo, &videoLoadError);
      if (videoLoadError) { insertError = videoLoadError; break; }
      if (videos.count > 0) {
        if (!videoTrack) videoTrack = [composition addMutableTrackWithMediaType:AVMediaTypeVideo preferredTrackID:kCMPersistentTrackID_Invalid];
        if (![videoTrack insertTimeRange:sourceRange ofTrack:videos.firstObject atTime:outputStart error:&insertError]) break;
        videoTrack.preferredTransform = videos.firstObject.preferredTransform;
        if (CMTimeCompare(sourceDuration, outputDuration) != 0) {
          [videoTrack scaleTimeRange:CMTimeRangeMake(outputStart, sourceDuration) toDuration:outputDuration];
        }
      }
      NSArray<AVAssetTrack *> *audios = LoadTracks(asset, AVMediaTypeAudio, nullptr);
      if (audios.count > 0) {
        if (!audioTrack) audioTrack = [composition addMutableTrackWithMediaType:AVMediaTypeAudio preferredTrackID:kCMPersistentTrackID_Invalid];
        NSError *audioError = nil;
        if ([audioTrack insertTimeRange:sourceRange ofTrack:audios.firstObject atTime:outputStart error:&audioError] && CMTimeCompare(sourceDuration, outputDuration) != 0) {
          [audioTrack scaleTimeRange:CMTimeRangeMake(outputStart, sourceDuration) toDuration:outputDuration];
        }
      }
    }
    dispatch_async(dispatch_get_main_queue(), ^{
      if (generation != gLoadGeneration.load()) return;
      EnsurePlayerView(); gError = nil; gLoaded = YES;
      if (insertError) {
        gError = insertError.localizedDescription ?: @"无法构建播放时间线";
        return;
      }
      AVPlayerItem *item = [AVPlayerItem playerItemWithAsset:composition];
      gCurrentAsset = composition;
      gSourceAssets = [sourceAssets copy];
      gPlayer = [AVPlayer playerWithPlayerItem:item]; gLayer.player = gPlayer;
      ObservePlayer();
      if (seconds > 0) {
        [gPlayer seekToTime:CMTimeMakeWithSeconds(seconds, 120000) toleranceBefore:kCMTimeZero toleranceAfter:kCMTimeZero completionHandler:^(BOOL finished) {
          if (finished && gRequestedPlaying) StartPlayback();
        }];
      } else if (gRequestedPlaying) StartPlayback();
    });
  });
  return Undefined(env);
}
static napi_value IsPlayable(napi_env env, napi_callback_info info) {
  size_t argc=1; napi_value argv[1]; napi_get_cb_info(env,info,&argc,argv,nullptr,nullptr);
  std::string path=StringArg(env,argv[0]); AVURLAsset *asset=[AVURLAsset URLAssetWithURL:[NSURL fileURLWithPath:[NSString stringWithUTF8String:path.c_str()]] options:nil];
  NSError *error = nil;
  NSArray<AVAssetTrack *> *tracks = LoadTracks(asset, AVMediaTypeVideo, &error);
  CMTime duration = tracks.count > 0 ? tracks.firstObject.timeRange.duration : kCMTimeInvalid;
  BOOL playable = error == nil && tracks.count > 0 && asset.playable && CMTIME_IS_NUMERIC(duration) && CMTimeGetSeconds(duration) > 0;
  napi_value result; napi_get_boolean(env, playable, &result); return result;
}
static napi_value Play(napi_env env, napi_callback_info info) { dispatch_async(dispatch_get_main_queue(), ^{ gRequestedPlaying = YES; StartPlayback(); }); return Undefined(env); }
static napi_value Pause(napi_env env, napi_callback_info info) { dispatch_async(dispatch_get_main_queue(), ^{ gRequestedPlaying = NO; gPrerolling = NO; [gPlayer cancelPendingPrerolls]; [gPlayer pause]; }); return Undefined(env); }
static napi_value Seek(napi_env env, napi_callback_info info) {
  size_t argc=1; napi_value argv[1]; napi_get_cb_info(env,info,&argc,argv,nullptr,nullptr); double seconds=NumberArg(env,argv[0]);
  dispatch_async(dispatch_get_main_queue(), ^{ gObservedSeconds=MAX(0,seconds); gEnded=NO; [gPlayer seekToTime:CMTimeMakeWithSeconds(MAX(0, seconds),120000) toleranceBefore:kCMTimeZero toleranceAfter:kCMTimeZero]; }); return Undefined(env);
}
static napi_value State(napi_env env, napi_callback_info info) {
  napi_value object; napi_create_object(env,&object);
  NSString *state = @"loading";
  if (gError) state=@"error";
  else if (!gLoaded || !gPlayer || !gPlayer.currentItem) state=@"loading";
  else if (gPlayer.currentItem.status == AVPlayerItemStatusFailed) { state=@"error"; gError=gPlayer.currentItem.error.localizedDescription; }
  else if (gPlayer.currentItem.status == AVPlayerItemStatusReadyToPlay) {
    if (gRequestedPlaying && gPlayer.rate == 0) StartPlayback();
    double current = gObservedSeconds;
    double duration = CMTimeGetSeconds(gPlayer.currentItem.duration);
    if (isfinite(duration) && duration > 0 && current >= duration - 0.001) state = @"ended";
    else if (gPlayer.timeControlStatus == AVPlayerTimeControlStatusPlaying) state = @"playing";
    else if (gRequestedPlaying || gPlayer.timeControlStatus == AVPlayerTimeControlStatusWaitingToPlayAtSpecifiedRate) state = @"waiting";
    else state = @"ready";
  }
  SetString(env,object,"state",state); SetString(env,object,"error",gError ?: @"");
  if ([state isEqualToString:@"loading"]) ApplyStatus(@"正在载入视频…", NO);
  else if ([state isEqualToString:@"waiting"]) ApplyStatus(@"正在准备播放…", NO);
  else if ([state isEqualToString:@"error"]) ApplyStatus(gError ?: @"视频无法播放", YES);
  else ApplyStatus(@"", NO);
  if (gEnded) state = @"ended";
  napi_value time, duration, rate, ready; napi_create_double(env,gObservedSeconds,&time); napi_set_named_property(env,object,"seconds",time);
  napi_create_double(env,CMTimeGetSeconds(gPlayer.currentItem.duration),&duration); napi_set_named_property(env,object,"duration",duration);
  napi_create_double(env,gPlayer.rate,&rate); napi_set_named_property(env,object,"rate",rate);
  napi_get_boolean(env,gLayer.readyForDisplay,&ready); napi_set_named_property(env,object,"ready_for_display",ready);
  return object;
}
static napi_value Dispose(napi_env env, napi_callback_info info) {
  ++gLoadGeneration;
  dispatch_async(dispatch_get_main_queue(), ^{ [gPlayer cancelPendingPrerolls]; [gPlayer pause]; if (gTimeObserver) [gPlayer removeTimeObserver:gTimeObserver]; if (gEndObserver) [[NSNotificationCenter defaultCenter] removeObserver:gEndObserver]; gTimeObserver=nil; gEndObserver=nil; gLayer.player=nil; [gPlayerView removeFromSuperview]; gPlayer=nil; gCurrentAsset=nil; gSourceAssets=nil; gLayer=nil; gSubtitleLayer=nil; gStatusLayer=nil; gSubtitleConfig=nil; gPresentationConfig=nil; gPlayerView=nil; gHostView=nil; gLoaded=NO; gRequestedPlaying=NO; gPrerolling=NO; gObservedSeconds=0; gEnded=NO; }); return Undefined(env);
}
static napi_value Init(napi_env env, napi_value exports) {
  struct { const char *name; napi_callback fn; } methods[]={{"attach",Attach},{"setFrame",SetFrame},{"setSubtitle",SetSubtitle},{"setPresentation",SetPresentation},{"loadTimeline",LoadTimeline},{"isPlayable",IsPlayable},{"play",Play},{"pause",Pause},{"seek",Seek},{"state",State},{"dispose",Dispose}};
  for (auto &method:methods) { napi_value fn; napi_create_function(env,method.name,NAPI_AUTO_LENGTH,method.fn,nullptr,&fn); napi_set_named_property(env,exports,method.name,fn); }
  return exports;
}
NAPI_MODULE(NODE_GYP_MODULE_NAME, Init)
