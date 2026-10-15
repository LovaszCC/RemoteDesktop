#import <Foundation/Foundation.h>

NS_ASSUME_NONNULL_BEGIN

/// One entry of a file list the server put on the clipboard ([MS-RDPECLIP] CLIPRDR_FILELIST).
@interface RDPRemoteFile : NSObject
/// Safe relative POSIX path (`dir/file.txt`); never absolute, never containing `.` or `..`.
@property(nonatomic, readonly, copy) NSString *path;
@property(nonatomic, readonly) BOOL isDirectory;
@property(nonatomic, readonly) BOOL hasSize;
@property(nonatomic, readonly) uint64_t size;
@property(nonatomic, readonly, nullable) NSDate *modified;
- (instancetype)initWithPath:(NSString *)path
                 isDirectory:(BOOL)isDirectory
                     hasSize:(BOOL)hasSize
                        size:(uint64_t)size
                    modified:(nullable NSDate *)modified;
@end

/// A `FILECONTENTS_RANGE` request: 64-bit position split in two, and the byte count.
typedef struct
{
	uint32_t positionLow;
	uint32_t positionHigh;
	uint32_t length;
} RDPFileRange;

/// Strict, server-distrusting parsing of remote file lists. Names come from the remote machine, so a
/// hostile server must not be able to write outside the download folder (`..\..\x`, `C:\x`, `\\srv\x` …).
@interface RDPClipboardFileList : NSObject
/// Size of one FILEDESCRIPTORW.
@property(class, nonatomic, readonly) NSUInteger descriptorSize;

/// The relative POSIX path for a Windows name, or nil when it is unsafe or invalid.
+ (nullable NSString *)sanitizedPathForRemoteName:(NSString *)name;

/// Parses `cItems` + descriptors. Fails as a whole (nil, `error` set) on any bad or duplicate entry,
/// a truncated buffer, more than `maxEntries` entries, or an entry below a file.
+ (nullable NSArray<RDPRemoteFile *> *)parse:(NSData *)data
                                  maxEntries:(NSUInteger)maxEntries
                                       error:(NSString *_Nullable *_Nullable)error;

/// Indices of entries without a parent folder: one pasteboard item each.
+ (NSIndexSet *)topLevelIndicesOf:(NSArray<RDPRemoteFile *> *)files;

/// Next chunk of a file of `size` bytes from `offset`; length 0 when done.
+ (RDPFileRange)rangeForSize:(uint64_t)size offset:(uint64_t)offset chunk:(uint32_t)chunk;
@end

NS_ASSUME_NONNULL_END
