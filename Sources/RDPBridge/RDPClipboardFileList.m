#import "RDPClipboardFileList.h"

/* [MS-RDPECLIP] 2.2.5.2.3.1 File Descriptor (FILEDESCRIPTORW): 592 bytes, little endian. */
enum
{
	DESCRIPTOR_SIZE = 592,
	OFFSET_FLAGS = 0,
	OFFSET_ATTRIBUTES = 36,
	OFFSET_WRITE_TIME = 60,
	OFFSET_SIZE_HIGH = 68,
	OFFSET_SIZE_LOW = 72,
	OFFSET_NAME = 76,
	NAME_UNITS = 260,
	FD_ATTRIBUTES = 0x04,
	FD_WRITESTIME = 0x20,
	FD_FILESIZE = 0x40,
	FILE_ATTRIBUTE_DIRECTORY = 0x10,
	MAX_COMPONENT_BYTES = 255,
	MAX_DEPTH = 64,
	MAX_PATH_BYTES = 1024,
};

/* FILETIME (100 ns since 1601-01-01) to Unix seconds. */
static const uint64_t FILETIME_UNIX_EPOCH = 116444736000000000ULL;

@implementation RDPRemoteFile

- (instancetype)initWithPath:(NSString *)path
                 isDirectory:(BOOL)isDirectory
                     hasSize:(BOOL)hasSize
                        size:(uint64_t)size
                    modified:(NSDate *)modified
{
	self = [super init];
	if (self)
	{
		_path = [path copy];
		_isDirectory = isDirectory;
		_hasSize = hasSize;
		_size = size;
		_modified = modified;
	}
	return self;
}

- (NSString *)description
{
	return [NSString stringWithFormat:@"%@%@ (%llu)", _path, _isDirectory ? @"/" : @"", _size];
}

@end

static uint32_t read_u32(const uint8_t *p)
{
	return (uint32_t)p[0] | (uint32_t)p[1] << 8 | (uint32_t)p[2] << 16 | (uint32_t)p[3] << 24;
}

static uint64_t read_u64(const uint8_t *p)
{
	return (uint64_t)read_u32(p) | (uint64_t)read_u32(p + 4) << 32;
}

/// Rejects lone surrogates and C0 / DEL controls.
static BOOL units_are_valid(NSString *name)
{
	const NSUInteger length = name.length;
	for (NSUInteger i = 0; i < length; i++)
	{
		const unichar unit = [name characterAtIndex:i];
		if (unit < 0x20 || unit == 0x7F)
			return NO;
		if (CFStringIsSurrogateHighCharacter(unit))
		{
			if (i + 1 >= length || !CFStringIsSurrogateLowCharacter([name characterAtIndex:i + 1]))
				return NO;
			i++;
		}
		else if (CFStringIsSurrogateLowCharacter(unit))
			return NO;
	}
	return YES;
}

@implementation RDPClipboardFileList

+ (NSUInteger)descriptorSize
{
	return DESCRIPTOR_SIZE;
}

+ (NSString *)sanitizedPathForRemoteName:(NSString *)name
{
	if (name.length == 0 || !units_are_valid(name))
		return nil;
	const unichar first = [name characterAtIndex:0];
	if (first == '\\' || first == '/')
		return nil; /* absolute or UNC */
	if ([name rangeOfString:@":"].location != NSNotFound)
		return nil; /* drive letter or NTFS alternate data stream */

	NSCharacterSet *separators = [NSCharacterSet characterSetWithCharactersInString:@"\\/"];
	NSCharacterSet *trailing = [NSCharacterSet characterSetWithCharactersInString:@" ."];
	NSArray<NSString *> *parts = [name componentsSeparatedByCharactersInSet:separators];
	if (parts.count > MAX_DEPTH)
		return nil;

	NSMutableArray<NSString *> *components = [NSMutableArray arrayWithCapacity:parts.count];
	for (NSString *part in parts)
	{
		if (part.length == 0 || [part isEqualToString:@"."] || [part isEqualToString:@".."])
			return nil;
		/* Windows ignores trailing spaces and dots ("x. " is "x"); "..." would collapse to nothing. */
		NSString *component = part;
		while (component.length > 0 &&
		       [trailing characterIsMember:[component characterAtIndex:component.length - 1]])
			component = [component substringToIndex:component.length - 1];
		if (component.length == 0)
			return nil;
		if ([component lengthOfBytesUsingEncoding:NSUTF8StringEncoding] > MAX_COMPONENT_BYTES)
			return nil;
		[components addObject:component];
	}
	NSString *path = [components componentsJoinedByString:@"/"];
	if ([path lengthOfBytesUsingEncoding:NSUTF8StringEncoding] > MAX_PATH_BYTES)
		return nil;
	return path;
}

+ (NSArray<RDPRemoteFile *> *)parse:(NSData *)data
                         maxEntries:(NSUInteger)maxEntries
                              error:(NSString **)error
{
#define FAIL(message)          \
	do                         \
	{                          \
		if (error)             \
			*error = (message); \
		return nil;            \
	} while (0)

	const uint8_t *bytes = data.bytes;
	if (data.length < 4)
		FAIL(@"truncated file list");
	const uint32_t count = read_u32(bytes);
	if (count == 0)
		FAIL(@"empty file list");
	if (count > maxEntries)
		FAIL(@"too many files");
	if ((data.length - 4) / DESCRIPTOR_SIZE < count)
		FAIL(@"truncated file list");

	NSMutableArray<RDPRemoteFile *> *files = [NSMutableArray arrayWithCapacity:count];
	/* Folded path → is directory; APFS is usually case-insensitive and normalization-insensitive. */
	NSMutableDictionary<NSString *, NSNumber *> *seen = [NSMutableDictionary dictionaryWithCapacity:count];
	for (uint32_t index = 0; index < count; index++)
	{
		const uint8_t *d = bytes + 4 + (size_t)index * DESCRIPTOR_SIZE;
		const uint32_t flags = read_u32(d + OFFSET_FLAGS);

		NSUInteger units = 0;
		while (units < NAME_UNITS && (d[OFFSET_NAME + units * 2] | d[OFFSET_NAME + units * 2 + 1]) != 0)
			units++;
		if (units == NAME_UNITS)
			FAIL(@"file name not terminated");
		/* Raw code units (no decoder that might turn a lone surrogate into U+FFFD unnoticed). */
		unichar raw[NAME_UNITS];
		for (NSUInteger i = 0; i < units; i++)
			raw[i] = (unichar)(d[OFFSET_NAME + i * 2] | d[OFFSET_NAME + i * 2 + 1] << 8);
		NSString *name = [NSString stringWithCharacters:raw length:units];
		NSString *path = [self sanitizedPathForRemoteName:name];
		if (!path)
			FAIL(([NSString stringWithFormat:@"unsafe file name: %@", name]));

		NSString *key = path.lowercaseString.precomposedStringWithCanonicalMapping;
		if (seen[key])
			FAIL(([NSString stringWithFormat:@"duplicate file name: %@", path]));
		NSString *parent = key.stringByDeletingLastPathComponent;
		for (; parent.length > 0; parent = parent.stringByDeletingLastPathComponent)
		{
			NSNumber *parentIsDirectory = seen[parent];
			if (parentIsDirectory && !parentIsDirectory.boolValue)
				FAIL(([NSString stringWithFormat:@"file inside a file: %@", path]));
		}

		const BOOL isDirectory =
		    (flags & FD_ATTRIBUTES) && (read_u32(d + OFFSET_ATTRIBUTES) & FILE_ATTRIBUTE_DIRECTORY);
		/* Explorer sometimes omits FD_FILESIZE; then the size is asked with FILECONTENTS_SIZE. */
		const BOOL hasSize = isDirectory || (flags & FD_FILESIZE);
		const uint64_t size = isDirectory ? 0
		                                  : ((uint64_t)read_u32(d + OFFSET_SIZE_HIGH) << 32 |
		                                     read_u32(d + OFFSET_SIZE_LOW));
		NSDate *modified = nil;
		const uint64_t filetime = read_u64(d + OFFSET_WRITE_TIME);
		if ((flags & FD_WRITESTIME) && filetime > FILETIME_UNIX_EPOCH)
			modified = [NSDate
			    dateWithTimeIntervalSince1970:(double)(filetime - FILETIME_UNIX_EPOCH) / 10000000.0];

		seen[key] = @(isDirectory);
		[files addObject:[[RDPRemoteFile alloc] initWithPath:path
		                                         isDirectory:isDirectory
		                                             hasSize:hasSize
		                                                size:size
		                                            modified:modified]];
	}
	return files;
#undef FAIL
}

+ (NSIndexSet *)topLevelIndicesOf:(NSArray<RDPRemoteFile *> *)files
{
	return [files indexesOfObjectsPassingTest:^BOOL(RDPRemoteFile *file, NSUInteger idx, BOOL *stop) {
		return [file.path rangeOfString:@"/"].location == NSNotFound;
	}];
}

+ (RDPFileRange)rangeForSize:(uint64_t)size offset:(uint64_t)offset chunk:(uint32_t)chunk
{
	RDPFileRange range = { (uint32_t)(offset & 0xFFFFFFFFu), (uint32_t)(offset >> 32), 0 };
	if (offset < size)
		range.length = (uint32_t)MIN((uint64_t)chunk, size - offset);
	return range;
}

@end
