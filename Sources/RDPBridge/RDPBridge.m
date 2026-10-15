#import "RDPBridge.h"
#import "RDPInputView.h"

#include <freerdp/freerdp.h>
#include <freerdp/client.h>
#include <freerdp/client/disp.h>
#include <freerdp/channels/disp.h>
#include <freerdp/error.h>
#include <freerdp/event.h>
#include <freerdp/input.h>
#include <freerdp/scancode.h>
#include <freerdp/settings.h>

#import <MacFreeRDP/MRDPView.h>
#import <MacFreeRDP/mf_client.h>
#import <MacFreeRDP/mfreerdp.h>

/// FreeRDP context of one bridged session: the Mac client's context plus a back pointer.
typedef struct
{
	mfContext mf;
	void *bridge; /* unretained RDPSessionBridge */
} BridgeContext;

@implementation RDPCertificateInfo
@end

@implementation RDPConnectionConfiguration
@end

@implementation RDPCredentials
@end

@interface RDPSessionBridge ()
- (void)channelConnected:(const ChannelConnectedEventArgs *)event;
- (void)channelDisconnected:(const ChannelDisconnectedEventArgs *)event;
- (void)connectionResult:(int)result;
- (void)desktopResized;
- (DWORD)verifyCertificate:(RDPCertificateInfo *)info;
- (nullable RDPCredentials *)requestCredentials:(RDPCredentials *)current;
@end

static pRdpClientNew macClientNew;

static RDPSessionBridge *bridge_of(void *context)
{
	return (__bridge RDPSessionBridge *)((BridgeContext *)context)->bridge;
}

static NSString *string_or_empty(const char *value)
{
	return value ? ([NSString stringWithUTF8String:value] ?: @"") : @"";
}

static void bridge_channel_connected(void *context, const ChannelConnectedEventArgs *e)
{
	[bridge_of(context) channelConnected:e];
}

static void bridge_channel_disconnected(void *context, const ChannelDisconnectedEventArgs *e)
{
	[bridge_of(context) channelDisconnected:e];
}

static void bridge_connection_result(void *context, const ConnectionResultEventArgs *e)
{
	[bridge_of(context) connectionResult:e->result];
}

static void bridge_resize_window(void *context, const ResizeWindowEventArgs *e)
{
	(void)e;
	[bridge_of(context) desktopResized];
}

static RDPCertificateInfo *certificate_info(const char *host, UINT16 port, const char *common_name,
                                            const char *subject, const char *issuer,
                                            const char *fingerprint, DWORD flags)
{
	RDPCertificateInfo *info = [RDPCertificateInfo new];
	info.host = string_or_empty(host);
	info.port = port;
	info.commonName = string_or_empty(common_name);
	info.subject = string_or_empty(subject);
	info.issuer = string_or_empty(issuer);
	info.fingerprint = string_or_empty(fingerprint);
	info.hostMismatch = (flags & VERIFY_CERT_FLAG_MISMATCH) != 0;
	info.changed = (flags & VERIFY_CERT_FLAG_CHANGED) != 0;
	return info;
}

static DWORD bridge_verify_certificate(freerdp *instance, const char *host, UINT16 port,
                                       const char *common_name, const char *subject,
                                       const char *issuer, const char *fingerprint, DWORD flags)
{
	RDPCertificateInfo *info =
	    certificate_info(host, port, common_name, subject, issuer, fingerprint, flags);
	return [bridge_of(instance->context) verifyCertificate:info];
}

static DWORD bridge_verify_changed_certificate(freerdp *instance, const char *host, UINT16 port,
                                               const char *common_name, const char *subject,
                                               const char *issuer, const char *fingerprint,
                                               const char *old_subject, const char *old_issuer,
                                               const char *old_fingerprint, DWORD flags)
{
	(void)old_subject;
	(void)old_issuer;
	(void)old_fingerprint;
	RDPCertificateInfo *info = certificate_info(host, port, common_name, subject, issuer,
	                                            fingerprint, flags | VERIFY_CERT_FLAG_CHANGED);
	return [bridge_of(instance->context) verifyCertificate:info];
}

static BOOL replace_string(char **target, NSString *value)
{
	char *copy = _strdup(value.UTF8String);
	if (!copy)
		return FALSE;
	free(*target);
	*target = copy;
	return TRUE;
}

/// FreeRDP asks only when the username is empty or no password is set. Only NLA gets a prompt: TLS
/// and RDP security fall back to the server's own logon screen, as before.
static BOOL bridge_authenticate(freerdp *instance, char **username, char **password,
                                char **domain, rdp_auth_reason reason)
{
	if (reason != AUTH_NLA)
		return TRUE;
	RDPCredentials *current = [RDPCredentials new];
	current.username = string_or_empty(*username);
	current.domain = string_or_empty(*domain);
	current.password = string_or_empty(*password);
	RDPCredentials *answer = [bridge_of(instance->context) requestCredentials:current];
	if (!answer)
		return FALSE;
	return replace_string(username, answer.username) && replace_string(domain, answer.domain) &&
	       replace_string(password, answer.password);
}

static BOOL bridge_client_new(freerdp *instance, rdpContext *context)
{
	if (!macClientNew(instance, context))
		return FALSE;
	instance->VerifyCertificateEx = bridge_verify_certificate;
	instance->VerifyChangedCertificateEx = bridge_verify_changed_certificate;
	instance->AuthenticateEx = bridge_authenticate;
	return TRUE;
}

@implementation RDPSessionBridge
{
	RDPConnectionConfiguration *_configuration;
	rdpContext *_context;
	RDPInputView *_view;
	DispClientContext *_disp; /* written on the connection thread, used on main */
	BOOL _connected;
	BOOL _stopping;
	BOOL _reportedEnd;
	NSSize _requestedSize;
	dispatch_queue_t _typingQueue;
}

+ (NSString *)freeRDPVersion
{
	return string_or_empty(freerdp_get_version_string());
}

- (instancetype)initWithConfiguration:(RDPConnectionConfiguration *)configuration
{
	self = [super init];
	if (self)
	{
		_configuration = configuration;
		_view = [[RDPInputView alloc]
		    initWithFrame:NSMakeRect(0, 0, configuration.width, configuration.height)];
		_view.autoresizingMask = NSViewWidthSizable | NSViewHeightSizable;
		_typingQueue = dispatch_queue_create("RDPSessionBridge.typing", DISPATCH_QUEUE_SERIAL);
	}
	return self;
}

- (void)dealloc
{
	[[NSNotificationCenter defaultCenter] removeObserver:self];
}

- (NSView *)view
{
	return _view;
}

- (BOOL)isConnected
{
	return _connected;
}

- (BOOL)applySettings:(rdpSettings *)settings
{
	RDPConnectionConfiguration *c = _configuration;
	BOOL ok = freerdp_settings_set_string(settings, FreeRDP_ServerHostname, c.host.UTF8String);
	ok = ok && freerdp_settings_set_uint32(settings, FreeRDP_ServerPort, c.port);
	ok = ok && freerdp_settings_set_string(settings, FreeRDP_Username, c.username.UTF8String);
	if (c.domain.length > 0)
		ok = ok && freerdp_settings_set_string(settings, FreeRDP_Domain, c.domain.UTF8String);
	if (c.password.length > 0)
		ok = ok && freerdp_settings_set_string(settings, FreeRDP_Password, c.password.UTF8String);
	ok = ok && freerdp_settings_set_uint32(settings, FreeRDP_DesktopWidth, c.width);
	ok = ok && freerdp_settings_set_uint32(settings, FreeRDP_DesktopHeight, c.height);
	ok = ok && freerdp_settings_set_uint32(settings, FreeRDP_ColorDepth, 32);
	ok = ok && freerdp_settings_set_bool(settings, FreeRDP_ConsoleSession, c.adminSession);
	// The view always fills its tab; scale the desktop into it until a resize catches up.
	ok = ok && freerdp_settings_set_bool(settings, FreeRDP_SmartSizing, TRUE);
	ok = ok && freerdp_settings_set_bool(settings, FreeRDP_SupportDisplayControl, c.dynamicResolution);
	ok = ok && freerdp_settings_set_bool(settings, FreeRDP_DynamicResolutionUpdate, c.dynamicResolution);
	ok = ok && freerdp_settings_set_bool(settings, FreeRDP_SupportGraphicsPipeline, TRUE);
	ok = ok && freerdp_settings_set_bool(settings, FreeRDP_RedirectClipboard, TRUE);
	ok = ok && freerdp_settings_set_bool(settings, FreeRDP_AutoReconnectionEnabled, TRUE);
	ok = ok && freerdp_settings_set_bool(settings, FreeRDP_IgnoreCertificate, FALSE);
	return ok;
}

- (BOOL)start
{
	RDP_CLIENT_ENTRY_POINTS entry = { 0 };
	RdpClientEntry(&entry);
	macClientNew = entry.ClientNew;
	entry.ClientNew = bridge_client_new;
	entry.ContextSize = sizeof(BridgeContext);

	_context = freerdp_client_context_new(&entry);
	if (!_context)
		return NO;
	((BridgeContext *)_context)->bridge = (__bridge void *)self;
	((mfContext *)_context)->view = (__bridge void *)_view;
	((mfContext *)_context)->view_ownership = FALSE;

	if (![self applySettings:_context->settings])
	{
		freerdp_client_context_free(_context);
		_context = NULL;
		return NO;
	}

	PubSub_SubscribeChannelConnected(_context->pubSub, bridge_channel_connected);
	PubSub_SubscribeChannelDisconnected(_context->pubSub, bridge_channel_disconnected);
	PubSub_SubscribeConnectionResult(_context->pubSub, bridge_connection_result);
	PubSub_SubscribeResizeWindow(_context->pubSub, bridge_resize_window);
	[[NSNotificationCenter defaultCenter] addObserver:self
	                                         selector:@selector(viewDidDisconnect:)
	                                             name:MRDPViewDidDisconnectNotification
	                                           object:_view];

	if (freerdp_client_start(_context) != 0)
	{
		freerdp_client_context_free(_context);
		_context = NULL;
		return NO;
	}
	return YES;
}

- (void)stop
{
	if (!_context || _stopping)
		return;
	_stopping = YES;
	_connected = NO;
	rdpContext *context = _context;
	MRDPView *view = _view;
	[view removeFromSuperview];
	// The connection thread dispatch_syncs onto main while painting, so it must not be joined here.
	dispatch_queue_t typingQueue = _typingQueue;
	dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^{
		// Typing checks _stopping per keystroke; let it drain before the context goes away.
		dispatch_sync(typingQueue, ^{
		});
		freerdp_client_stop(context);
		dispatch_async(dispatch_get_main_queue(), ^{
			[view releaseResources];
			freerdp_client_context_free(context);
			self->_context = NULL;
		});
	});
}

- (void)viewDidResize
{
	if (!_context)
		return;
	NSSize size = _view.bounds.size;
	[_view setScrollOffset:0 y:0 w:(int)size.width h:(int)size.height];
	if (!_configuration.dynamicResolution)
		return;
	_requestedSize = size;
	[NSObject cancelPreviousPerformRequestsWithTarget:self selector:@selector(sendMonitorLayout) object:nil];
	[self performSelector:@selector(sendMonitorLayout) withObject:nil afterDelay:0.3];
}

- (void)typeKeystrokes:(NSArray<NSNumber *> *)keystrokes
            completion:(void (^)(BOOL finished))completion
{
	rdpContext *context = _context;
	if (!context || !_connected || _stopping)
	{
		if (completion)
			completion(NO);
		return;
	}
	NSArray<NSNumber *> *keys = [keystrokes copy];
	dispatch_async(_typingQueue, ^{
		BOOL finished = YES;
		for (NSNumber *key in keys)
		{
			if (self->_stopping || !self->_connected)
			{
				finished = NO;
				break;
			}
			rdpInput *input = context->input;
			const int32_t value = key.intValue;
			if (value == -1 || value == -2)
			{
				const UINT32 scancode = value == -1 ? RDP_SCANCODE_RETURN : RDP_SCANCODE_TAB;
				(void)freerdp_input_send_keyboard_event_ex(input, TRUE, FALSE, scancode);
				(void)freerdp_input_send_keyboard_event_ex(input, FALSE, FALSE, scancode);
			}
			else if (value >= 0)
			{
				(void)freerdp_input_send_unicode_keyboard_event(input, 0, (UINT16)value);
				(void)freerdp_input_send_unicode_keyboard_event(input, KBD_FLAGS_RELEASE, (UINT16)value);
			}
			usleep(2000);
		}
		if (completion)
			dispatch_async(dispatch_get_main_queue(), ^{
				completion(finished);
			});
	});
}

/// Display Control: widths must be even, both dimensions within 200...8192.
- (void)sendMonitorLayout
{
	DispClientContext *disp = _disp;
	if (!disp || !_connected || _stopping)
		return;
	UINT32 width = (UINT32)MAX(200, MIN(8192, (int)_requestedSize.width)) & ~1u;
	UINT32 height = (UINT32)MAX(200, MIN(8192, (int)_requestedSize.height));
	rdpSettings *settings = _context->settings;
	if (width == freerdp_settings_get_uint32(settings, FreeRDP_DesktopWidth) &&
	    height == freerdp_settings_get_uint32(settings, FreeRDP_DesktopHeight))
		return;
	DISPLAY_CONTROL_MONITOR_LAYOUT layout = { 0 };
	layout.Flags = DISPLAY_CONTROL_MONITOR_PRIMARY;
	layout.Width = width;
	layout.Height = height;
	layout.DesktopScaleFactor = 100;
	layout.DeviceScaleFactor = 100;
	(void)disp->SendMonitorLayout(disp, 1, &layout);
}

#pragma mark FreeRDP callbacks (connection thread)

- (void)channelConnected:(const ChannelConnectedEventArgs *)event
{
	if (strcmp(event->name, DISP_DVC_CHANNEL_NAME) == 0)
	{
		DispClientContext *disp = (DispClientContext *)event->pInterface;
		dispatch_async(dispatch_get_main_queue(), ^{
			self->_disp = disp;
			[self viewDidResize];
		});
	}
}

- (void)channelDisconnected:(const ChannelDisconnectedEventArgs *)event
{
	if (strcmp(event->name, DISP_DVC_CHANNEL_NAME) == 0)
		dispatch_async(dispatch_get_main_queue(), ^{
			self->_disp = NULL;
		});
}

- (void)connectionResult:(int)result
{
	if (result != 0)
		return;
	dispatch_async(dispatch_get_main_queue(), ^{
		if (self->_stopping)
			return;
		self->_connected = YES;
		[self viewDidResize];
		[self.delegate rdpSessionDidConnect:self];
	});
}

- (void)desktopResized
{
	dispatch_async(dispatch_get_main_queue(), ^{
		if (!self->_context)
			return;
		NSSize size = self->_view.bounds.size;
		[self->_view setScrollOffset:0 y:0 w:(int)size.width h:(int)size.height];
		[self->_view setNeedsDisplay:YES];
	});
}

- (DWORD)verifyCertificate:(RDPCertificateInfo *)info
{
	__block RDPCertificateDecision decision = RDPCertificateDecisionReject;
	dispatch_sync(dispatch_get_main_queue(), ^{
		id<RDPSessionBridgeDelegate> delegate = self.delegate;
		if (delegate && !self->_stopping)
			decision = [delegate rdpSession:self verifyCertificate:info];
	});
	return (DWORD)decision;
}

- (RDPCredentials *)requestCredentials:(RDPCredentials *)current
{
	__block RDPCredentials *answer = nil;
	dispatch_sync(dispatch_get_main_queue(), ^{
		id<RDPSessionBridgeDelegate> delegate = self.delegate;
		if (!delegate || self->_stopping)
			return;
		answer = [delegate respondsToSelector:@selector(rdpSession:requestCredentials:)]
		             ? [delegate rdpSession:self requestCredentials:current]
		             : current;
	});
	return answer;
}

#pragma mark Main thread

- (void)viewDidDisconnect:(NSNotification *)notification
{
	if (_reportedEnd)
		return;
	_reportedEnd = YES;
	_connected = NO;
	if (_stopping)
		return;
	uint32_t lastError = [notification.userInfo[@"lastError"] unsignedIntValue];
	uint32_t errorInfo = [notification.userInfo[@"errorInfo"] unsignedIntValue];
	NSString *message = @"";
	if (errorInfo != 0)
		message = string_or_empty(freerdp_get_error_info_string(errorInfo));
	else if (lastError != 0)
		message = string_or_empty(freerdp_get_last_error_string(lastError));
	[self.delegate rdpSession:self
	    didDisconnectWithLastError:lastError
	                     errorInfo:errorInfo
	                       message:message];
}

@end
