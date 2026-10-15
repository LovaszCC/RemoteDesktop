#import <Foundation/Foundation.h>

#import "RDPClipboardFileList.h"

NS_ASSUME_NONNULL_BEGIN

/// [MS-RDPECLIP] FILECONTENTS_SIZE / FILECONTENTS_RANGE.
typedef NS_ENUM(uint32_t, RDPFileContentsKind) {
    RDPFileContentsKindSize = 1,
    RDPFileContentsKindRange = 2,
};

/// Sends one File Contents Request to the server (any thread).
typedef void (^RDPFileContentsSender)(uint32_t streamId, uint32_t listIndex, RDPFileContentsKind kind,
                                      RDPFileRange range);

/// Downloads a server file list into a fresh folder, one request at a time. Writes only below that folder
/// (no-follow, exclusive create, relative to the folder's descriptor), stamps the modification time and
/// marks files as quarantined, so Gatekeeper checks executables that came from the remote machine.
@interface RDPClipboardDownloader : NSObject
/// `root`: created if missing; each job gets its own `<root>/<job>` subfolder.
- (instancetype)initWithRoot:(NSURL *)root
                      sender:(RDPFileContentsSender)sender
                   chunkSize:(uint32_t)chunkSize
               allowHugeFiles:(BOOL)allowHugeFiles
              stallTimeout:(NSTimeInterval)stallTimeout;
- (instancetype)init NS_UNAVAILABLE;

/// Starts a download, cancelling any running one. `completion` (on a private queue) gets the folder holding
/// the files (paths as in `files`), or an error message.
- (void)downloadFiles:(NSArray<RDPRemoteFile *> *)files
           completion:(void (^)(NSURL *_Nullable folder, NSString *_Nullable error))completion;

/// A File Contents Response from the server (any thread). Responses for another stream are ignored.
- (void)receiveStreamId:(uint32_t)streamId ok:(BOOL)ok data:(NSData *)data;

/// Stops the running download, removes its partial files and reports "cancelled".
- (void)cancel;

/// Bytes written / expected in the running download (0 / 0 when idle; expected grows when sizes are asked).
@property(atomic, readonly) uint64_t bytesDone;
@property(atomic, readonly) uint64_t bytesTotal;
@end

NS_ASSUME_NONNULL_END
