#import "RDPInputView.h"
#import "RDPKeyboardTranslator.h"

#include <freerdp/input.h>
#include <winpr/input.h>

@implementation RDPInputView
{
	RDPKeyboardTranslator *_translator;
}

- (instancetype)initWithFrame:(NSRect)frameRect
{
	self = [super initWithFrame:frameRect];
	if (self)
		_translator = [RDPKeyboardTranslator new];
	return self;
}

/// Upstream `fixKeyCode`: ISO keyboards report the § and ` keys swapped.
static DWORD fixed_key_code(DWORD keyCode, enum APPLE_KEYBOARD_TYPE type)
{
	if (type != APPLE_KEYBOARD_TYPE_ISO)
		return keyCode;
	if (keyCode == APPLE_VK_ANSI_Grave)
		return APPLE_VK_ISO_Section;
	if (keyCode == APPLE_VK_ISO_Section)
		return APPLE_VK_ANSI_Grave;
	return keyCode;
}

- (uint32_t)scancodeForEvent:(NSEvent *)event
{
	const DWORD keyCode = fixed_key_code(event.keyCode, mfc->appleKeyboardType);
	const DWORD vkcode = GetVirtualKeyCodeFromKeycode(keyCode, WINPR_KEYCODE_TYPE_APPLE);
	return (uint32_t)GetVirtualScanCodeFromVirtualKeyCode(vkcode, 4);
}

- (void)send:(NSArray<RDPKeyAction *> *)actions
{
	if (!self.is_connected || actions.count == 0)
		return;
	rdpInput *input = instance->context->input;
	for (RDPKeyAction *action in actions)
	{
		if (action.kind == RDPKeyActionKindUnicode)
		{
			(void)freerdp_input_send_unicode_keyboard_event(input, 0, (UINT16)action.code);
			(void)freerdp_input_send_unicode_keyboard_event(input, KBD_FLAGS_RELEASE, (UINT16)action.code);
			continue;
		}
		const UINT16 flags = (action.down ? KBD_FLAGS_DOWN : KBD_FLAGS_RELEASE) |
		                     ((action.code & KBDEXT) ? KBD_FLAGS_EXTENDED : 0);
		(void)freerdp_input_send_keyboard_event(input, flags, (UINT8)(action.code & 0xFF));
	}
}

- (void)flagsChanged:(NSEvent *)event
{
	if (!self.is_connected)
		return;
	[self send:[_translator modifiersChanged:event.modifierFlags]];
}

- (void)keyDown:(NSEvent *)event
{
	if (!self.is_connected)
		return;
	[self send:[_translator modifiersChanged:event.modifierFlags]];
	[self send:[_translator keyDown:event.keyCode
	                       scancode:[self scancodeForEvent:event]
	                     characters:event.characters ?: @""]];
}

- (void)keyUp:(NSEvent *)event
{
	if (!self.is_connected)
		return;
	[self send:[_translator keyUp:event.keyCode scancode:[self scancodeForEvent:event]]];
}

- (void)pause
{
	if (self.is_connected)
		[self send:[_translator releaseAll]];
	else
		(void)[_translator releaseAll];
	[super pause];
}

- (void)resume
{
	(void)[_translator releaseAll];
	[super resume];
}

@end
