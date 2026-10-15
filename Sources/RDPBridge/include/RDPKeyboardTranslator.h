#import <Foundation/Foundation.h>

NS_ASSUME_NONNULL_BEGIN

typedef NS_ENUM(NSInteger, RDPKeyActionKind) {
    /// `code` is an RDP scancode; 0x100 marks an extended key (e.g. 0x138 = right Alt).
    RDPKeyActionKindScancode,
    /// `code` is a UTF-16 code unit, sent as a press and a release.
    RDPKeyActionKindUnicode,
};

/// One keyboard event to send to the server.
@interface RDPKeyAction : NSObject
@property(nonatomic, readonly) RDPKeyActionKind kind;
@property(nonatomic, readonly) uint32_t code;
/// Press or release (always YES for Unicode actions).
@property(nonatomic, readonly) BOOL down;
+ (instancetype)scancode:(uint32_t)code down:(BOOL)down;
+ (instancetype)unicode:(uint16_t)unit;
@end

/// Maps Mac keyboard input to RDP keyboard events the way a Mac user expects:
/// - left Option + a key that types a character on the Mac sends that character (Unicode), so the Mac
///   layout's special characters arrive whatever the server's layout is; with other keys it is Alt;
/// - right Option is Alt (AltGr on layouts that have it);
/// - ⌘ + key is Ctrl + key (⌘C copies), ⌘ tapped alone is the Windows key.
/// Pure state machine: the view feeds it events and sends the returned actions.
@interface RDPKeyboardTranslator : NSObject
/// `flags`: NSEvent.modifierFlags including the device-dependent left/right bits.
- (NSArray<RDPKeyAction *> *)modifiersChanged:(NSUInteger)flags;
/// `characters`: NSEvent.characters (with modifiers applied); empty for a dead key.
- (NSArray<RDPKeyAction *> *)keyDown:(uint16_t)keyCode
                            scancode:(uint32_t)scancode
                          characters:(NSString *)characters;
- (NSArray<RDPKeyAction *> *)keyUp:(uint16_t)keyCode scancode:(uint32_t)scancode;
/// Releases everything still held on the server (focus lost, session paused).
- (NSArray<RDPKeyAction *> *)releaseAll;
@end

NS_ASSUME_NONNULL_END
