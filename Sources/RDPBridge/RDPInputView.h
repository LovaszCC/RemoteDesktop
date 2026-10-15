#import <MacFreeRDP/MRDPView.h>

/// MRDPView with Mac-style keyboard input (see `RDPKeyboardTranslator`): replaces the upstream key handling,
/// which sends Option as left Alt and ⌘ as the Windows key.
@interface RDPInputView : MRDPView
@end
