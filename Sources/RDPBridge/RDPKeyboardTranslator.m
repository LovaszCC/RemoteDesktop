#import "RDPKeyboardTranslator.h"

#import <AppKit/AppKit.h>

/* RDP scancodes (freerdp/scancode.h), 0x100 = extended. */
enum
{
	SC_LSHIFT = 0x2A,
	SC_RSHIFT = 0x36,
	SC_LCONTROL = 0x1D,
	SC_RCONTROL = 0x11D,
	SC_LMENU = 0x38,
	SC_RMENU = 0x138,
	SC_LWIN = 0x15B,
	SC_CAPSLOCK = 0x3A,
};

/* Device-dependent modifier bits of NSEvent.modifierFlags (IOKit NX_DEVICE*KEYMASK). */
enum
{
	DEV_LCTL = 0x0001,
	DEV_LSHIFT = 0x0002,
	DEV_RSHIFT = 0x0004,
	DEV_LCMD = 0x0008,
	DEV_RCMD = 0x0010,
	DEV_LALT = 0x0020,
	DEV_RALT = 0x0040,
	DEV_RCTL = 0x2000,
};

@implementation RDPKeyAction

- (instancetype)initWithKind:(RDPKeyActionKind)kind code:(uint32_t)code down:(BOOL)down
{
	self = [super init];
	if (self)
	{
		_kind = kind;
		_code = code;
		_down = down;
	}
	return self;
}

+ (instancetype)scancode:(uint32_t)code down:(BOOL)down
{
	return [[self alloc] initWithKind:RDPKeyActionKindScancode code:code down:down];
}

+ (instancetype)unicode:(uint16_t)unit
{
	return [[self alloc] initWithKind:RDPKeyActionKindUnicode code:unit down:YES];
}

- (BOOL)isEqual:(id)other
{
	if (![other isKindOfClass:[RDPKeyAction class]])
		return NO;
	RDPKeyAction *action = other;
	return action.kind == _kind && action.code == _code && action.down == _down;
}

- (NSUInteger)hash
{
	return (NSUInteger)_kind << 20 ^ _code << 1 ^ _down;
}

- (NSString *)description
{
	return [NSString stringWithFormat:@"%@ 0x%X %@", _kind == RDPKeyActionKindUnicode ? @"unicode" : @"scancode",
	                                  _code, _down ? @"down" : @"up"];
}

@end

/// Whether `characters` is text to type: no control characters, no function-key private-use codes.
static BOOL is_typable(NSString *characters)
{
	if (characters.length == 0)
		return NO;
	for (NSUInteger i = 0; i < characters.length; i++)
	{
		const unichar unit = [characters characterAtIndex:i];
		if (unit < 0x20 || unit == 0x7F || (unit >= 0xF700 && unit <= 0xF8FF))
			return NO;
	}
	return YES;
}

@implementation RDPKeyboardTranslator
{
	NSUInteger _flags;
	/* Modifier scancodes sent down and not yet released, in press order. */
	NSMutableOrderedSet<NSNumber *> *_heldModifiers;
	/* Ordinary keys sent down and not yet released. */
	NSMutableOrderedSet<NSNumber *> *_heldKeys;
	/* Key codes whose press went out as Unicode (or was a dead key): their release sends nothing. */
	NSMutableSet<NSNumber *> *_swallowedKeys;
	BOOL _commandHeld;
	BOOL _commandUsed;
	BOOL _leftOptionHeld;
	BOOL _rightOptionHeld;
	BOOL _deadKeyPending;
}

- (instancetype)init
{
	self = [super init];
	if (self)
	{
		_heldModifiers = [NSMutableOrderedSet new];
		_heldKeys = [NSMutableOrderedSet new];
		_swallowedKeys = [NSMutableSet new];
	}
	return self;
}

/// Device bit when the event carries left/right bits, else the generic flag (synthetic events).
static BOOL side_pressed(NSUInteger flags, NSUInteger deviceBit, NSUInteger allDeviceBits,
                         NSEventModifierFlags generic, BOOL isDefaultSide)
{
	if (flags & allDeviceBits)
		return (flags & deviceBit) != 0;
	return isDefaultSide && (flags & generic) != 0;
}

- (void)set:(uint32_t)scancode down:(BOOL)down into:(NSMutableArray<RDPKeyAction *> *)actions
{
	NSNumber *key = @(scancode);
	const BOOL held = [_heldModifiers containsObject:key];
	if (down == held)
		return;
	if (down)
		[_heldModifiers addObject:key];
	else
		[_heldModifiers removeObject:key];
	[actions addObject:[RDPKeyAction scancode:scancode down:down]];
}

- (NSArray<RDPKeyAction *> *)modifiersChanged:(NSUInteger)flags
{
	NSMutableArray<RDPKeyAction *> *actions = [NSMutableArray new];
	const NSUInteger previous = _flags;
	_flags = flags;

	const NSUInteger shiftBits = DEV_LSHIFT | DEV_RSHIFT;
	[self set:SC_LSHIFT
	      down:side_pressed(flags, DEV_LSHIFT, shiftBits, NSEventModifierFlagShift, YES)
	      into:actions];
	[self set:SC_RSHIFT
	      down:side_pressed(flags, DEV_RSHIFT, shiftBits, NSEventModifierFlagShift, NO)
	      into:actions];
	[self set:SC_LCONTROL down:(flags & NSEventModifierFlagControl) != 0 into:actions];

	/* Either Option is held back: it types the Mac character, or becomes Alt with a key that types none.
	 * A dead key's pending state survives releasing Option (⌥E, release, E → é). */
	const NSUInteger optionBits = DEV_LALT | DEV_RALT;
	_leftOptionHeld = side_pressed(flags, DEV_LALT, optionBits, NSEventModifierFlagOption, YES);
	_rightOptionHeld = side_pressed(flags, DEV_RALT, optionBits, NSEventModifierFlagOption, NO);
	if (!_leftOptionHeld)
		[self set:SC_LMENU down:NO into:actions];
	if (!_rightOptionHeld)
		[self set:SC_RMENU down:NO into:actions];

	const BOOL command = (flags & NSEventModifierFlagCommand) != 0;
	if (command && !_commandHeld)
		_commandUsed = NO;
	else if (!command && _commandHeld)
	{
		if ([_heldModifiers containsObject:@(SC_RCONTROL)])
			[self set:SC_RCONTROL down:NO into:actions];
		else if (!_commandUsed)
		{
			[actions addObject:[RDPKeyAction scancode:SC_LWIN down:YES]];
			[actions addObject:[RDPKeyAction scancode:SC_LWIN down:NO]];
		}
	}
	_commandHeld = command;

	if ((flags ^ previous) & NSEventModifierFlagCapsLock)
	{
		[actions addObject:[RDPKeyAction scancode:SC_CAPSLOCK down:YES]];
		[actions addObject:[RDPKeyAction scancode:SC_CAPSLOCK down:NO]];
	}
	return actions;
}

- (NSArray<RDPKeyAction *> *)typeCharacters:(NSString *)characters keyCode:(uint16_t)keyCode
{
	[_swallowedKeys addObject:@(keyCode)];
	NSMutableArray<RDPKeyAction *> *actions = [NSMutableArray new];
	for (NSUInteger i = 0; i < characters.length; i++)
		[actions addObject:[RDPKeyAction unicode:[characters characterAtIndex:i]]];
	return actions;
}

- (NSArray<RDPKeyAction *> *)keyDown:(uint16_t)keyCode
                            scancode:(uint32_t)scancode
                          characters:(NSString *)characters
{
	const BOOL control = (_flags & NSEventModifierFlagControl) != 0;
	if (_commandHeld)
		_commandUsed = YES;
	else if ((_leftOptionHeld || _rightOptionHeld) && !control)
	{
		if (characters.length == 0)
		{
			/* Dead key (e.g. ⌥E): the next key brings the composed character. */
			_deadKeyPending = YES;
			[_swallowedKeys addObject:@(keyCode)];
			return @[];
		}
		if (is_typable(characters))
		{
			_deadKeyPending = NO;
			return [self typeCharacters:characters keyCode:keyCode];
		}
	}
	else if (_deadKeyPending && !control && is_typable(characters))
	{
		_deadKeyPending = NO;
		return [self typeCharacters:characters keyCode:keyCode];
	}
	_deadKeyPending = NO;

	NSMutableArray<RDPKeyAction *> *actions = [NSMutableArray new];
	if (_commandHeld)
		[self set:SC_RCONTROL down:YES into:actions];
	if (_leftOptionHeld)
		[self set:SC_LMENU down:YES into:actions];
	if (_rightOptionHeld)
		[self set:SC_RMENU down:YES into:actions];
	[actions addObject:[RDPKeyAction scancode:scancode down:YES]];
	if (_commandHeld)
	{
		/* AppKit sends no keyUp for keys pressed with ⌘ held: release right away (repeats press again). */
		[_swallowedKeys addObject:@(keyCode)];
		[actions addObject:[RDPKeyAction scancode:scancode down:NO]];
	}
	else
		[_heldKeys addObject:@(scancode)];
	return actions;
}

- (NSArray<RDPKeyAction *> *)keyUp:(uint16_t)keyCode scancode:(uint32_t)scancode
{
	if ([_swallowedKeys containsObject:@(keyCode)])
	{
		[_swallowedKeys removeObject:@(keyCode)];
		return @[];
	}
	[_heldKeys removeObject:@(scancode)];
	return @[ [RDPKeyAction scancode:scancode down:NO] ];
}

- (NSArray<RDPKeyAction *> *)releaseAll
{
	NSMutableArray<RDPKeyAction *> *actions = [NSMutableArray new];
	for (NSNumber *key in _heldKeys.reverseObjectEnumerator)
		[actions addObject:[RDPKeyAction scancode:key.unsignedIntValue down:NO]];
	for (NSNumber *key in _heldModifiers.reverseObjectEnumerator)
		[actions addObject:[RDPKeyAction scancode:key.unsignedIntValue down:NO]];
	[_heldKeys removeAllObjects];
	[_heldModifiers removeAllObjects];
	[_swallowedKeys removeAllObjects];
	_flags = 0;
	_commandHeld = NO;
	_commandUsed = NO;
	_leftOptionHeld = NO;
	_rightOptionHeld = NO;
	_deadKeyPending = NO;
	return actions;
}

@end
