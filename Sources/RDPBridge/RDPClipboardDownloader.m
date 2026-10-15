#import "RDPClipboardDownloader.h"

#include <fcntl.h>
#include <sys/stat.h>
#include <sys/xattr.h>
#include <unistd.h>

static NSString *const CancelledMessage = @"cancelled";

@interface RDPClipboardDownloader ()
@property(atomic, readwrite) uint64_t bytesDone;
@property(atomic, readwrite) uint64_t bytesTotal;
@end

@implementation RDPClipboardDownloader
{
	NSURL *_root;
	RDPFileContentsSender _sender;
	uint32_t _chunkSize;
	BOOL _allowHugeFiles;
	NSTimeInterval _stallTimeout;
	dispatch_queue_t _queue;

	/* Job state, only touched on _queue. */
	NSArray<RDPRemoteFile *> *_files;
	void (^_completion)(NSURL *, NSString *);
	NSURL *_jobFolder;
	int _rootFd;
	NSUInteger _index;
	uint64_t _size;
	uint64_t _offset;
	int _fileFd;
	uint32_t _nextStreamId;
	uint32_t _pendingStreamId;
	RDPFileContentsKind _pendingKind;
	uint32_t _pendingLength;
	NSUInteger _job;
	NSDate *_lastProgress;
}

- (instancetype)initWithRoot:(NSURL *)root
                      sender:(RDPFileContentsSender)sender
                   chunkSize:(uint32_t)chunkSize
               allowHugeFiles:(BOOL)allowHugeFiles
              stallTimeout:(NSTimeInterval)stallTimeout
{
	self = [super init];
	if (self)
	{
		_root = root;
		_sender = [sender copy];
		_chunkSize = MAX(chunkSize, 1u);
		_allowHugeFiles = allowHugeFiles;
		_stallTimeout = stallTimeout;
		_queue = dispatch_queue_create("RDPClipboardDownloader", DISPATCH_QUEUE_SERIAL);
		_rootFd = -1;
		_fileFd = -1;
		_nextStreamId = 1;
	}
	return self;
}

- (void)dealloc
{
	if (_fileFd >= 0)
		close(_fileFd);
	if (_rootFd >= 0)
		close(_rootFd);
}

- (void)downloadFiles:(NSArray<RDPRemoteFile *> *)files
           completion:(void (^)(NSURL *, NSString *))completion
{
	dispatch_async(_queue, ^{
		[self finishWithError:CancelledMessage];
		[self startFiles:files completion:completion];
	});
}

- (void)cancel
{
	dispatch_async(_queue, ^{
		[self finishWithError:CancelledMessage];
	});
}

- (void)receiveStreamId:(uint32_t)streamId ok:(BOOL)ok data:(NSData *)data
{
	NSData *copy = [data copy];
	dispatch_async(_queue, ^{
		[self handleStreamId:streamId ok:ok data:copy];
	});
}

#pragma mark Queue

- (void)startFiles:(NSArray<RDPRemoteFile *> *)files completion:(void (^)(NSURL *, NSString *))completion
{
	_files = [files copy];
	_completion = [completion copy];
	_index = 0;
	_job++;
	self.bytesDone = 0;
	uint64_t total = 0;
	for (RDPRemoteFile *file in files)
	{
		if (!_allowHugeFiles && file.size > UINT32_MAX)
		{
			[self finishWithError:[NSString stringWithFormat:@"%@ is larger than 4 GB, which this server "
			                                                 @"does not support",
			                                                 file.path]];
			return;
		}
		total += file.size;
	}
	self.bytesTotal = total;

	NSFileManager *fm = NSFileManager.defaultManager;
	NSError *error = nil;
	if (![fm createDirectoryAtURL:_root
	          withIntermediateDirectories:YES
	                           attributes:@{ NSFilePosixPermissions : @0700 }
	                                error:&error])
	{
		[self finishWithError:error.localizedDescription];
		return;
	}
	NSNumber *available = nil;
	[_root getResourceValue:&available forKey:NSURLVolumeAvailableCapacityForImportantUsageKey error:nil];
	if (available && available.unsignedLongLongValue < total)
	{
		[self finishWithError:@"not enough free disk space"];
		return;
	}
	_jobFolder = [_root URLByAppendingPathComponent:NSUUID.UUID.UUIDString isDirectory:YES];
	if (mkdir(_jobFolder.fileSystemRepresentation, 0700) != 0 ||
	    (_rootFd = open(_jobFolder.fileSystemRepresentation, O_RDONLY | O_DIRECTORY | O_NOFOLLOW)) < 0)
	{
		[self finishWithError:[NSString stringWithFormat:@"cannot create download folder (%s)",
		                                                 strerror(errno)]];
		return;
	}
	[self touchProgress];
	[self scheduleWatchdogForJob:_job];
	[self startEntry];
}

- (void)touchProgress
{
	_lastProgress = [NSDate date];
}

- (void)scheduleWatchdogForJob:(NSUInteger)job
{
	__weak RDPClipboardDownloader *weakSelf = self;
	const NSTimeInterval timeout = _stallTimeout;
	dispatch_after(dispatch_time(DISPATCH_TIME_NOW, (int64_t)(timeout / 2 * NSEC_PER_SEC)), _queue, ^{
		RDPClipboardDownloader *self = weakSelf;
		if (!self || self->_job != job || !self->_completion)
			return;
		if (-self->_lastProgress.timeIntervalSinceNow > timeout)
			[self finishWithError:@"the server stopped sending the file"];
		else
			[self scheduleWatchdogForJob:job];
	});
}

/// Opens (creating if needed) the directory holding `path`'s last component, below the job folder, never
/// following symlinks. Returns the descriptor (caller closes) and the leaf name.
- (int)openParentOf:(NSString *)path leaf:(NSString **)leaf
{
	NSArray<NSString *> *components = path.pathComponents;
	int fd = dup(_rootFd);
	for (NSUInteger i = 0; fd >= 0 && i + 1 < components.count; i++)
	{
		const char *name = components[i].fileSystemRepresentation;
		if (mkdirat(fd, name, 0755) != 0 && errno != EEXIST)
		{
			close(fd);
			return -1;
		}
		const int next = openat(fd, name, O_RDONLY | O_DIRECTORY | O_NOFOLLOW);
		close(fd);
		fd = next;
	}
	*leaf = components.lastObject;
	return fd;
}

- (void)startEntry
{
	while (_index < _files.count)
	{
		RDPRemoteFile *file = _files[_index];
		NSString *leaf = nil;
		const int parent = [self openParentOf:file.path leaf:&leaf];
		if (parent < 0)
		{
			[self finishWithError:[NSString stringWithFormat:@"cannot create %@ (%s)", file.path,
			                                                 strerror(errno)]];
			return;
		}
		if (file.isDirectory)
		{
			const BOOL ok = mkdirat(parent, leaf.fileSystemRepresentation, 0755) == 0 || errno == EEXIST;
			close(parent);
			if (!ok)
			{
				[self finishWithError:[NSString stringWithFormat:@"cannot create %@ (%s)", file.path,
				                                                 strerror(errno)]];
				return;
			}
			_index++;
			continue;
		}
		_fileFd = openat(parent, leaf.fileSystemRepresentation,
		                 O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC, 0644);
		close(parent);
		if (_fileFd < 0)
		{
			[self finishWithError:[NSString stringWithFormat:@"cannot create %@ (%s)", file.path,
			                                                 strerror(errno)]];
			return;
		}
		_offset = 0;
		if (file.hasSize)
		{
			_size = file.size;
			[self requestNextRange];
		}
		else
			[self sendKind:RDPFileContentsKindSize range:(RDPFileRange){ 0, 0, 8 }];
		return;
	}
	[self finishWithError:nil];
}

- (void)sendKind:(RDPFileContentsKind)kind range:(RDPFileRange)range
{
	_pendingStreamId = _nextStreamId++;
	_pendingKind = kind;
	_pendingLength = range.length;
	_sender(_pendingStreamId, (uint32_t)_index, kind, range);
}

- (void)requestNextRange
{
	const RDPFileRange range = [RDPClipboardFileList rangeForSize:_size offset:_offset chunk:_chunkSize];
	if (range.length > 0)
	{
		[self sendKind:RDPFileContentsKindRange range:range];
		return;
	}
	[self closeFile];
	_index++;
	[self startEntry];
}

- (void)closeFile
{
	if (_fileFd < 0)
		return;
	RDPRemoteFile *file = _files[_index];
	if (file.modified)
	{
		const NSTimeInterval seconds = file.modified.timeIntervalSince1970;
		struct timespec times[2];
		times[0].tv_sec = 0;
		times[0].tv_nsec = UTIME_OMIT;
		times[1].tv_sec = (time_t)seconds;
		times[1].tv_nsec = (long)((seconds - floor(seconds)) * 1e9);
		(void)futimens(_fileFd, times);
	}
	char quarantine[64];
	snprintf(quarantine, sizeof(quarantine), "0081;%08lx;RemoteDesktop;", (unsigned long)time(NULL));
	(void)fsetxattr(_fileFd, "com.apple.quarantine", quarantine, strlen(quarantine), 0, 0);
	close(_fileFd);
	_fileFd = -1;
}

- (void)handleStreamId:(uint32_t)streamId ok:(BOOL)ok data:(NSData *)data
{
	if (!_completion || _pendingStreamId == 0 || streamId != _pendingStreamId)
		return; /* stale, or no download running */
	_pendingStreamId = 0;
	RDPRemoteFile *file = _files[_index];
	if (!ok)
	{
		[self finishWithError:[NSString stringWithFormat:@"the server refused to send %@", file.path]];
		return;
	}
	[self touchProgress];

	if (_pendingKind == RDPFileContentsKindSize)
	{
		if (data.length != 8)
		{
			[self finishWithError:@"invalid file size from the server"];
			return;
		}
		uint64_t size = 0;
		memcpy(&size, data.bytes, 8);
		size = CFSwapInt64LittleToHost(size);
		if (!_allowHugeFiles && size > UINT32_MAX)
		{
			[self finishWithError:[NSString stringWithFormat:@"%@ is larger than 4 GB, which this server "
			                                                 @"does not support",
			                                                 file.path]];
			return;
		}
		_size = size;
		self.bytesTotal += size;
		[self requestNextRange];
		return;
	}

	if (data.length == 0 || data.length > _pendingLength)
	{
		[self finishWithError:[NSString stringWithFormat:@"invalid data from the server for %@", file.path]];
		return;
	}
	const uint8_t *bytes = data.bytes;
	size_t left = data.length;
	while (left > 0)
	{
		const ssize_t written = write(_fileFd, bytes, left);
		if (written < 0 && errno == EINTR)
			continue;
		if (written <= 0)
		{
			[self finishWithError:[NSString stringWithFormat:@"cannot write %@ (%s)", file.path,
			                                                 strerror(errno)]];
			return;
		}
		bytes += written;
		left -= (size_t)written;
	}
	_offset += data.length;
	self.bytesDone += data.length;
	[self requestNextRange];
}

/// Ends the running job (no-op when idle): nil error = success, else partial files are removed.
- (void)finishWithError:(NSString *)error
{
	if (!_completion)
		return;
	if (_fileFd >= 0)
	{
		close(_fileFd);
		_fileFd = -1;
	}
	if (_rootFd >= 0)
	{
		close(_rootFd);
		_rootFd = -1;
	}
	NSURL *folder = _jobFolder;
	if (error && folder)
		[NSFileManager.defaultManager removeItemAtURL:folder error:nil];
	void (^completion)(NSURL *, NSString *) = _completion;
	_completion = nil;
	_files = nil;
	_jobFolder = nil;
	_pendingStreamId = 0;
	completion(error ? nil : folder, error);
}

@end
