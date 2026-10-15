#import <AppKit/AppKit.h>

#include <freerdp/client/cliprdr.h>

NS_ASSUME_NONNULL_BEGIN

/// Texts the clipboard shows while it waits for remote files (supplied localized by the app).
@interface RDPClipboardTexts : NSObject
@property(nonatomic, copy) NSString *progressTitle;
@property(nonatomic, copy) NSString *cancel;
@end

/// The session's clipboard channel, replacing FreeRDP's text-only Mac clipboard: text both ways, and files
/// both ways ([MS-RDPECLIP] file streams). Mac → Windows files are served by FreeRDP's
/// `client_cliprdr_file.c`; Windows → Mac files are put on the pasteboard as promises and downloaded when
/// Finder pastes them.
@interface RDPClipboard : NSObject <NSPasteboardItemDataProvider>
/// `folder`: this session's private download folder (deleted by `stop`).
- (instancetype)initWithFolder:(NSURL *)folder texts:(RDPClipboardTexts *)texts;
- (instancetype)init NS_UNAVAILABLE;

/// Called on the main thread when a file copy fails (`error`) or finished after Finder gave up (`nil`).
@property(nonatomic, copy, nullable) void (^onFileTransferEnded)(NSString *_Nullable error);

/// Channel thread: takes over every server callback of `cliprdr`.
- (void)attach:(CliprdrClientContext *)cliprdr;
/// Channel thread: the channel is going away.
- (void)detach;
/// Main thread, from the view's pasteboard poll: announces local clipboard changes to the server.
- (void)pasteboardChanged;
/// Main thread: session over — cancels downloads, withdraws our pasteboard promises, deletes the folder.
- (void)stop;

/// Removes download folders left behind by app instances that are no longer running.
+ (void)purgeStaleFoldersIn:(NSURL *)root;
@end

NS_ASSUME_NONNULL_END
