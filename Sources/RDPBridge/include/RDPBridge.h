#import <AppKit/AppKit.h>

NS_ASSUME_NONNULL_BEGIN

/// Answer to a certificate prompt; values match FreeRDP's VerifyCertificateEx contract.
typedef NS_ENUM(NSInteger, RDPCertificateDecision) {
    RDPCertificateDecisionReject = 0,
    RDPCertificateDecisionAcceptPermanently = 1,
    RDPCertificateDecisionAcceptOnce = 2,
};

/// Certificate details shown to the user before trusting a server.
@interface RDPCertificateInfo : NSObject
@property(nonatomic, copy) NSString *host;
@property(nonatomic) uint16_t port;
@property(nonatomic, copy) NSString *commonName;
@property(nonatomic, copy) NSString *subject;
@property(nonatomic, copy) NSString *issuer;
@property(nonatomic, copy) NSString *fingerprint;
/// The server presented a different certificate than the one trusted before.
@property(nonatomic) BOOL changed;
/// The certificate does not match the host name.
@property(nonatomic) BOOL hostMismatch;
@end

/// Sign-in details asked for when the server requires NLA and the connection lacks them.
@interface RDPCredentials : NSObject
@property(nonatomic, copy) NSString *username;
@property(nonatomic, copy) NSString *domain;
@property(nonatomic, copy) NSString *password;
@end

/// Everything needed to open one session; secrets are copied into FreeRDP settings, never argv.
@interface RDPConnectionConfiguration : NSObject
@property(nonatomic, copy) NSString *host;
@property(nonatomic) uint16_t port;
@property(nonatomic, copy) NSString *username;
@property(nonatomic, copy) NSString *domain;
@property(nonatomic, copy, nullable) NSString *password;
@property(nonatomic) BOOL adminSession;
@property(nonatomic) uint32_t width;
@property(nonatomic) uint32_t height;
@property(nonatomic) BOOL dynamicResolution;
@end

@class RDPSessionBridge;

/// All callbacks arrive on the main thread.
@protocol RDPSessionBridgeDelegate <NSObject>
- (void)rdpSessionDidConnect:(RDPSessionBridge *)session;
/// `lastError` is FreeRDP's last error code, `errorInfo` the server's disconnect reason.
- (void)rdpSession:(RDPSessionBridge *)session
    didDisconnectWithLastError:(uint32_t)lastError
                     errorInfo:(uint32_t)errorInfo
                       message:(NSString *)message;
/// Blocks the connection thread until answered.
- (RDPCertificateDecision)rdpSession:(RDPSessionBridge *)session
                   verifyCertificate:(RDPCertificateInfo *)certificate;
@optional
/// NLA needs a username or password the configuration does not have; `current` holds what is set.
/// Blocks the connection thread until answered; nil cancels the connection. Without it the
/// connection continues with what it has.
- (nullable RDPCredentials *)rdpSession:(RDPSessionBridge *)session
                     requestCredentials:(RDPCredentials *)current
    NS_SWIFT_NAME(rdpSession(_:requestCredentials:));
@end

/// One embedded FreeRDP session rendering into `view`.
@interface RDPSessionBridge : NSObject
@property(nonatomic, readonly) NSView *view;
@property(nonatomic, weak, nullable) id<RDPSessionBridgeDelegate> delegate;
@property(nonatomic, readonly) BOOL isConnected;

- (instancetype)initWithConfiguration:(RDPConnectionConfiguration *)configuration;
- (instancetype)init NS_UNAVAILABLE;

/// Starts connecting on FreeRDP's own thread; returns NO if the session could not be created.
- (BOOL)start;
/// Disconnects and waits for the connection thread to finish.
- (void)stop;
/// Keeps the view's input mapping in sync with its size and, with dynamic resolution, asks the
/// server for a matching desktop size.
- (void)viewDidResize;

/// Types keystrokes into the session in order (values >= 0: UTF-16 units sent as Unicode key
/// events; -1: Enter; -2: Tab), paced so slow applications keep up. Runs off the main thread;
/// `completion` is called on the main thread with NO if the session ended before finishing.
- (void)typeKeystrokes:(NSArray<NSNumber *> *)keystrokes
            completion:(nullable void (^)(BOOL finished))completion;

+ (NSString *)freeRDPVersion;
@end

NS_ASSUME_NONNULL_END
