/*
 * FreeRDP for the Rust session engine; see rdshim.h. Modelled on FreeRDP's client/sample and the Swift
 * app's RDPBridge.
 */
#include "rdshim.h"

#include <stdlib.h>
#include <string.h>

#include <winpr/crt.h>
#include <winpr/synch.h>
#include <winpr/wlog.h>
#include <winpr/winsock.h>

#include <freerdp/freerdp.h>
#include <freerdp/version.h>
#include <freerdp/gdi/gdi.h>
#include <freerdp/graphics.h>
#include <freerdp/codec/color.h>
#include <freerdp/client.h>
#include <freerdp/client/channels.h>
#include <freerdp/client/cliprdr.h>
#include <freerdp/client/disp.h>
#include <freerdp/channels/channels.h>
#include <freerdp/channels/cliprdr.h>
#include <freerdp/channels/disp.h>
#include <freerdp/error.h>
#include <freerdp/update.h>

typedef struct
{
	rdpClientContext common;
	RdShim* shim;
} ShimContext;

typedef struct
{
	rdpPointer pointer;
	BYTE* rgba;
} ShimPointer;

struct RdShim
{
	RdShimCallbacks cb;
	CRITICAL_SECTION lock;    /* guards context against rdshim_abort() */
	CRITICAL_SECTION cb_lock; /* every callback runs under it; see rdshim_lock_callbacks() */
	HANDLE wake;
	rdpContext* context;
	DispClientContext* disp;
	CliprdrClientContext* clip;
	char* pending_user;
	char* pending_domain;
	char* pending_password;
};

/* FreeRDP's channels (cliprdr, and the graphics pipeline on the drdynvc thread) call back from their own
 * threads, so every callback runs under the shim's callback lock. */
#define CALLBACK(shim, name, ...)                                \
	do                                                           \
	{                                                            \
		EnterCriticalSection(&(shim)->cb_lock);                  \
		(shim)->cb.name((shim)->cb.user, __VA_ARGS__);           \
		LeaveCriticalSection(&(shim)->cb_lock);                  \
	} while (0)
#define CALLBACK0(shim, name)                                    \
	do                                                           \
	{                                                            \
		EnterCriticalSection(&(shim)->cb_lock);                  \
		(shim)->cb.name((shim)->cb.user);                        \
		LeaveCriticalSection(&(shim)->cb_lock);                  \
	} while (0)

static RdShim* shim_of(rdpContext* context)
{
	return context ? ((ShimContext*)context)->shim : NULL;
}

static char* dup_or_null(const char* s)
{
	return (s && *s) ? _strdup(s) : NULL;
}

static void wipe_free(char* s)
{
	if (s)
	{
		memset(s, 0, strlen(s));
		free(s);
	}
}

const char* rdshim_version(void)
{
	return FREERDP_VERSION_FULL;
}

/* ---- painting -------------------------------------------------------------------------------- */

static BOOL shim_begin_paint(rdpContext* context)
{
	rdpGdi* gdi = context->gdi;
	if (!gdi || !gdi->primary || !gdi->primary->hdc || !gdi->primary->hdc->hwnd)
		return TRUE;
	HGDI_WND hwnd = gdi->primary->hdc->hwnd;
	hwnd->invalid->null = TRUE;
	hwnd->ninvalid = 0;
	return TRUE;
}

static BOOL shim_end_paint(rdpContext* context)
{
	RdShim* shim = shim_of(context);
	rdpGdi* gdi = context->gdi;
	if (!shim || !gdi || !gdi->primary || !gdi->primary->hdc || !gdi->primary->hdc->hwnd)
		return TRUE;
	HGDI_WND hwnd = gdi->primary->hdc->hwnd;
	if (hwnd->invalid->null || hwnd->ninvalid <= 0)
		return TRUE;
	const INT32 count = hwnd->ninvalid;
	RdShimRect* rects = calloc((size_t)count, sizeof(RdShimRect));
	if (!rects)
		return TRUE;
	for (INT32 i = 0; i < count; i++)
	{
		rects[i].x = hwnd->cinvalid[i].x;
		rects[i].y = hwnd->cinvalid[i].y;
		rects[i].width = hwnd->cinvalid[i].w;
		rects[i].height = hwnd->cinvalid[i].h;
	}
	if (shim->cb.painted)
		CALLBACK(shim, painted, rects, (uint32_t)count);
	free(rects);
	return TRUE;
}

static BOOL shim_desktop_resize(rdpContext* context)
{
	RdShim* shim = shim_of(context);
	rdpSettings* settings = context->settings;
	const UINT32 width = freerdp_settings_get_uint32(settings, FreeRDP_DesktopWidth);
	const UINT32 height = freerdp_settings_get_uint32(settings, FreeRDP_DesktopHeight);
	/* The graphics pipeline resizes from the drdynvc thread; the buffer is reallocated under the update
	 * lock, which rdshim_lock_framebuffer() holds while the session thread reads it. */
	rdp_update_lock(context->update);
	const BOOL resized = gdi_resize(context->gdi, width, height);
	rdp_update_unlock(context->update);
	if (!resized)
		return FALSE;
	if (shim && shim->cb.desktop)
		CALLBACK(shim, desktop, width, height);
	return TRUE;
}

/* ---- pointer --------------------------------------------------------------------------------- */

static BOOL shim_pointer_new(rdpContext* context, rdpPointer* pointer)
{
	ShimPointer* p = (ShimPointer*)pointer;
	p->rgba = NULL;
	if (pointer->width == 0 || pointer->height == 0)
		return TRUE;
	const size_t size = (size_t)pointer->width * pointer->height * 4;
	p->rgba = calloc(1, size);
	if (!p->rgba)
		return FALSE;
	if (!freerdp_image_copy_from_pointer_data(p->rgba, PIXEL_FORMAT_RGBA32, pointer->width * 4, 0, 0,
	                                          pointer->width, pointer->height, pointer->xorMaskData,
	                                          pointer->lengthXorMask, pointer->andMaskData,
	                                          pointer->lengthAndMask, pointer->xorBpp,
	                                          &context->gdi->palette))
	{
		free(p->rgba);
		p->rgba = NULL;
	}
	return TRUE;
}

static void shim_pointer_free(rdpContext* context, rdpPointer* pointer)
{
	WINPR_UNUSED(context);
	ShimPointer* p = (ShimPointer*)pointer;
	free(p->rgba);
	p->rgba = NULL;
}

static BOOL shim_pointer_set(rdpContext* context, rdpPointer* pointer)
{
	RdShim* shim = shim_of(context);
	ShimPointer* p = (ShimPointer*)pointer;
	if (!shim)
		return TRUE;
	if (!p->rgba)
	{
		if (shim->cb.pointer_hidden)
			CALLBACK0(shim, pointer_hidden);
		return TRUE;
	}
	if (shim->cb.pointer_shape)
		CALLBACK(shim, pointer_shape, p->rgba, pointer->width, pointer->height,
		                       pointer->xPos, pointer->yPos);
	return TRUE;
}

static BOOL shim_pointer_set_null(rdpContext* context)
{
	RdShim* shim = shim_of(context);
	if (shim && shim->cb.pointer_hidden)
		CALLBACK0(shim, pointer_hidden);
	return TRUE;
}

static BOOL shim_pointer_set_default(rdpContext* context)
{
	RdShim* shim = shim_of(context);
	if (shim && shim->cb.pointer_default)
		CALLBACK0(shim, pointer_default);
	return TRUE;
}

static BOOL shim_pointer_set_position(rdpContext* context, UINT32 x, UINT32 y)
{
	WINPR_UNUSED(context);
	WINPR_UNUSED(x);
	WINPR_UNUSED(y);
	return TRUE;
}

/* ---- cliprdr, server to client -------------------------------------------------------------- */

static RdShim* clip_shim(CliprdrClientContext* clip)
{
	return clip ? (RdShim*)clip->custom : NULL;
}

static UINT clip_monitor_ready(CliprdrClientContext* clip, const CLIPRDR_MONITOR_READY* ready)
{
	WINPR_UNUSED(ready);
	RdShim* shim = clip_shim(clip);
	if (shim && shim->cb.clip_monitor_ready)
		CALLBACK0(shim, clip_monitor_ready);
	return CHANNEL_RC_OK;
}

static UINT clip_server_capabilities(CliprdrClientContext* clip, const CLIPRDR_CAPABILITIES* caps)
{
	RdShim* shim = clip_shim(clip);
	UINT32 flags = 0;
	for (UINT32 i = 0; caps && i < caps->cCapabilitiesSets; i++)
	{
		const CLIPRDR_CAPABILITY_SET* set = &caps->capabilitySets[i];
		if (set->capabilitySetType == CB_CAPSTYPE_GENERAL)
			flags = ((const CLIPRDR_GENERAL_CAPABILITY_SET*)set)->generalFlags;
	}
	if (shim && shim->cb.clip_capabilities)
		CALLBACK(shim, clip_capabilities, flags);
	return CHANNEL_RC_OK;
}

static UINT clip_server_format_list(CliprdrClientContext* clip, const CLIPRDR_FORMAT_LIST* list)
{
	RdShim* shim = clip_shim(clip);
	if (!shim || !shim->cb.clip_format_list)
		return CHANNEL_RC_OK;
	const UINT32 count = list->numFormats;
	uint32_t* ids = calloc(count + 1, sizeof(uint32_t));
	const char** names = calloc(count + 1, sizeof(char*));
	if (!ids || !names)
	{
		free(ids);
		free(names);
		return CHANNEL_RC_NO_MEMORY;
	}
	for (UINT32 i = 0; i < count; i++)
	{
		ids[i] = list->formats[i].formatId;
		names[i] = list->formats[i].formatName;
	}
	CALLBACK(shim, clip_format_list, ids, names, count);
	free(ids);
	free(names);
	return CHANNEL_RC_OK;
}

static UINT clip_server_format_list_response(CliprdrClientContext* clip,
                                             const CLIPRDR_FORMAT_LIST_RESPONSE* response)
{
	WINPR_UNUSED(clip);
	WINPR_UNUSED(response);
	return CHANNEL_RC_OK;
}

static UINT clip_server_lock(CliprdrClientContext* clip, const CLIPRDR_LOCK_CLIPBOARD_DATA* lock)
{
	RdShim* shim = clip_shim(clip);
	if (shim && shim->cb.clip_lock)
		CALLBACK(shim, clip_lock, lock->clipDataId, 1);
	return CHANNEL_RC_OK;
}

static UINT clip_server_unlock(CliprdrClientContext* clip,
                               const CLIPRDR_UNLOCK_CLIPBOARD_DATA* unlock)
{
	RdShim* shim = clip_shim(clip);
	if (shim && shim->cb.clip_lock)
		CALLBACK(shim, clip_lock, unlock->clipDataId, 0);
	return CHANNEL_RC_OK;
}

static UINT clip_server_data_request(CliprdrClientContext* clip,
                                     const CLIPRDR_FORMAT_DATA_REQUEST* request)
{
	RdShim* shim = clip_shim(clip);
	if (shim && shim->cb.clip_data_request)
		CALLBACK(shim, clip_data_request, request->requestedFormatId);
	return CHANNEL_RC_OK;
}

static UINT clip_server_data_response(CliprdrClientContext* clip,
                                      const CLIPRDR_FORMAT_DATA_RESPONSE* response)
{
	RdShim* shim = clip_shim(clip);
	if (shim && shim->cb.clip_data_response)
	{
		const int ok = (response->common.msgFlags & CB_RESPONSE_OK) != 0;
		CALLBACK(shim, clip_data_response, ok, response->requestedFormatData,
		                            ok ? response->common.dataLen : 0);
	}
	return CHANNEL_RC_OK;
}

static UINT clip_server_contents_request(CliprdrClientContext* clip,
                                         const CLIPRDR_FILE_CONTENTS_REQUEST* request)
{
	RdShim* shim = clip_shim(clip);
	if (shim && shim->cb.clip_contents_request)
	{
		const uint64_t position =
		    ((uint64_t)request->nPositionHigh << 32) | (uint64_t)request->nPositionLow;
		CALLBACK(shim, clip_contents_request, request->streamId, request->listIndex,
		                               request->dwFlags, position, request->cbRequested,
		                               request->haveClipDataId ? 1 : 0, request->clipDataId);
	}
	return CHANNEL_RC_OK;
}

static UINT clip_server_contents_response(CliprdrClientContext* clip,
                                          const CLIPRDR_FILE_CONTENTS_RESPONSE* response)
{
	RdShim* shim = clip_shim(clip);
	if (shim && shim->cb.clip_contents_response)
	{
		const int ok = (response->common.msgFlags & CB_RESPONSE_OK) != 0;
		CALLBACK(shim, clip_contents_response, response->streamId, ok,
		                                response->requestedData, ok ? response->cbRequested : 0);
	}
	return CHANNEL_RC_OK;
}

/* ---- channels -------------------------------------------------------------------------------- */

static UINT disp_caps(DispClientContext* disp, UINT32 max_monitors, UINT32 factor_a,
                      UINT32 factor_b)
{
	WINPR_UNUSED(max_monitors);
	WINPR_UNUSED(factor_a);
	WINPR_UNUSED(factor_b);
	RdShim* shim = (RdShim*)disp->custom;
	if (shim && shim->cb.display_control_ready)
		CALLBACK0(shim, display_control_ready);
	return CHANNEL_RC_OK;
}

static void on_channel_connected(void* ctx, const ChannelConnectedEventArgs* e)
{
	rdpContext* context = (rdpContext*)ctx;
	RdShim* shim = shim_of(context);
	if (strcmp(e->name, CLIPRDR_SVC_CHANNEL_NAME) == 0)
	{
		CliprdrClientContext* clip = (CliprdrClientContext*)e->pInterface;
		clip->custom = shim;
		clip->MonitorReady = clip_monitor_ready;
		clip->ServerCapabilities = clip_server_capabilities;
		clip->ServerFormatList = clip_server_format_list;
		clip->ServerFormatListResponse = clip_server_format_list_response;
		clip->ServerLockClipboardData = clip_server_lock;
		clip->ServerUnlockClipboardData = clip_server_unlock;
		clip->ServerFormatDataRequest = clip_server_data_request;
		clip->ServerFormatDataResponse = clip_server_data_response;
		clip->ServerFileContentsRequest = clip_server_contents_request;
		clip->ServerFileContentsResponse = clip_server_contents_response;
		if (shim)
			shim->clip = clip;
	}
	else if (strcmp(e->name, DISP_DVC_CHANNEL_NAME) == 0)
	{
		DispClientContext* disp = (DispClientContext*)e->pInterface;
		disp->custom = shim;
		disp->DisplayControlCaps = disp_caps;
		if (shim)
			shim->disp = disp;
	}
	else
		freerdp_client_OnChannelConnectedEventHandler(ctx, e);
}

static void on_channel_disconnected(void* ctx, const ChannelDisconnectedEventArgs* e)
{
	rdpContext* context = (rdpContext*)ctx;
	RdShim* shim = shim_of(context);
	if (strcmp(e->name, CLIPRDR_SVC_CHANNEL_NAME) == 0)
	{
		CliprdrClientContext* clip = (CliprdrClientContext*)e->pInterface;
		clip->custom = NULL;
		if (shim)
			shim->clip = NULL;
	}
	else if (strcmp(e->name, DISP_DVC_CHANNEL_NAME) == 0)
	{
		DispClientContext* disp = (DispClientContext*)e->pInterface;
		disp->custom = NULL;
		if (shim)
			shim->disp = NULL;
	}
	else
		freerdp_client_OnChannelDisconnectedEventHandler(ctx, e);
}

/* ---- connection callbacks -------------------------------------------------------------------- */

static int shim_verify_x509(freerdp* instance, const BYTE* data, size_t length, const char* host,
                            UINT16 port, DWORD flags)
{
	RdShim* shim = shim_of(instance->context);
	if (!shim || !shim->cb.verify_certificate)
		return 0;
	/* 2 = accept for this connection only: our own store (rdp_known_hosts) decides, FreeRDP's
	 * known_hosts stays empty. */
	EnterCriticalSection(&shim->cb_lock);
	const int accept = shim->cb.verify_certificate(shim->cb.user, data, length, host, port, flags);
	LeaveCriticalSection(&shim->cb_lock);
	return accept ? 2 : 0;
}

static BOOL shim_authenticate(freerdp* instance, char** username, char** password, char** domain,
                              rdp_auth_reason reason)
{
	RdShim* shim = shim_of(instance->context);
	/* Without NLA the server shows its own login screen; a gateway isn't supported. */
	if (reason != AUTH_NLA && reason != AUTH_RDSTLS)
		return (reason == AUTH_TLS || reason == AUTH_RDP) ? TRUE : FALSE;
	if (!shim || !shim->cb.authenticate)
		return FALSE;
	EnterCriticalSection(&shim->cb_lock);
	const int answered = shim->cb.authenticate(shim->cb.user, shim);
	LeaveCriticalSection(&shim->cb_lock);
	if (!answered)
		return FALSE;
	wipe_free(*username);
	wipe_free(*password);
	wipe_free(*domain);
	*username = shim->pending_user;
	*password = shim->pending_password;
	*domain = shim->pending_domain;
	shim->pending_user = shim->pending_password = shim->pending_domain = NULL;
	return TRUE;
}

static BOOL shim_pre_connect(freerdp* instance)
{
	rdpContext* context = instance->context;
	rdpSettings* settings = context->settings;
	if (!freerdp_settings_set_uint32(settings, FreeRDP_OsMajorType, OSMAJORTYPE_UNSPECIFIED))
		return FALSE;
	if (PubSub_SubscribeChannelConnected(context->pubSub, on_channel_connected) < 0)
		return FALSE;
	if (PubSub_SubscribeChannelDisconnected(context->pubSub, on_channel_disconnected) < 0)
		return FALSE;
	return TRUE;
}

static BOOL shim_post_connect(freerdp* instance)
{
	rdpContext* context = instance->context;
	RdShim* shim = shim_of(context);
	if (!gdi_init(instance, PIXEL_FORMAT_RGBA32))
		return FALSE;
	context->update->BeginPaint = shim_begin_paint;
	context->update->EndPaint = shim_end_paint;
	context->update->DesktopResize = shim_desktop_resize;

	rdpPointer pointer = { 0 };
	pointer.size = sizeof(ShimPointer);
	pointer.New = shim_pointer_new;
	pointer.Free = shim_pointer_free;
	pointer.Set = shim_pointer_set;
	pointer.SetNull = shim_pointer_set_null;
	pointer.SetDefault = shim_pointer_set_default;
	pointer.SetPosition = shim_pointer_set_position;
	graphics_register_pointer(context->graphics, &pointer);

	if (shim && shim->cb.desktop)
		CALLBACK(shim, desktop, context->gdi->width, context->gdi->height);
	return TRUE;
}

static void shim_post_disconnect(freerdp* instance)
{
	rdpContext* context = instance->context;
	if (!context)
		return;
	PubSub_UnsubscribeChannelConnected(context->pubSub, on_channel_connected);
	PubSub_UnsubscribeChannelDisconnected(context->pubSub, on_channel_disconnected);
	gdi_free(instance);
}

static BOOL shim_client_new(freerdp* instance, rdpContext* context)
{
	WINPR_UNUSED(context);
	instance->PreConnect = shim_pre_connect;
	instance->PostConnect = shim_post_connect;
	instance->PostDisconnect = shim_post_disconnect;
	instance->AuthenticateEx = shim_authenticate;
	instance->VerifyX509Certificate = shim_verify_x509;
	return TRUE;
}

static void shim_client_free(freerdp* instance, rdpContext* context)
{
	WINPR_UNUSED(instance);
	WINPR_UNUSED(context);
}

static int shim_client_entry(RDP_CLIENT_ENTRY_POINTS* entry)
{
	ZeroMemory(entry, sizeof(RDP_CLIENT_ENTRY_POINTS));
	entry->Version = RDP_CLIENT_INTERFACE_VERSION;
	entry->Size = sizeof(RDP_CLIENT_ENTRY_POINTS_V1);
	entry->ContextSize = sizeof(ShimContext);
	entry->ClientNew = shim_client_new;
	entry->ClientFree = shim_client_free;
	return 0;
}

/* ---- lifecycle ------------------------------------------------------------------------------- */

RdShim* rdshim_new(const RdShimCallbacks* callbacks)
{
	/* FreeRDP logs to stdout, also about certificates its own store doesn't know (the engine checks
	 * them against rdp_known_hosts); quiet unless WLOG_LEVEL asks for more. */
	if (!getenv("WLOG_LEVEL"))
		WLog_SetLogLevel(WLog_GetRoot(), WLOG_FATAL);
	RdShim* shim = calloc(1, sizeof(RdShim));
	if (!shim)
		return NULL;
	shim->cb = *callbacks;
	InitializeCriticalSection(&shim->lock);
	InitializeCriticalSection(&shim->cb_lock);
	shim->wake = CreateEventA(NULL, TRUE, FALSE, NULL);
	if (!shim->wake)
	{
		DeleteCriticalSection(&shim->cb_lock);
		DeleteCriticalSection(&shim->lock);
		free(shim);
		return NULL;
	}
	return shim;
}

static void free_context(RdShim* shim)
{
	EnterCriticalSection(&shim->lock);
	rdpContext* context = shim->context;
	shim->context = NULL;
	shim->disp = NULL;
	shim->clip = NULL;
	LeaveCriticalSection(&shim->lock);
	if (context)
	{
		freerdp_client_stop(context);
		freerdp_client_context_free(context);
	}
}

void rdshim_free(RdShim* shim)
{
	if (!shim)
		return;
	free_context(shim);
	wipe_free(shim->pending_user);
	wipe_free(shim->pending_domain);
	wipe_free(shim->pending_password);
	CloseHandle(shim->wake);
	DeleteCriticalSection(&shim->cb_lock);
	DeleteCriticalSection(&shim->lock);
	free(shim);
}

int rdshim_set_credentials(RdShim* shim, const char* username, const char* domain,
                           const char* password)
{
	wipe_free(shim->pending_user);
	wipe_free(shim->pending_domain);
	wipe_free(shim->pending_password);
	shim->pending_user = dup_or_null(username);
	shim->pending_domain = dup_or_null(domain);
	shim->pending_password = dup_or_null(password);
	return 1;
}

static BOOL apply_config(rdpSettings* s, const RdShimConfig* c)
{
	BOOL ok = freerdp_settings_set_string(s, FreeRDP_ServerHostname, c->host);
	ok = ok && freerdp_settings_set_uint32(s, FreeRDP_ServerPort, c->port);
	if (c->username && *c->username)
		ok = ok && freerdp_settings_set_string(s, FreeRDP_Username, c->username);
	if (c->domain && *c->domain)
		ok = ok && freerdp_settings_set_string(s, FreeRDP_Domain, c->domain);
	if (c->password && *c->password)
		ok = ok && freerdp_settings_set_string(s, FreeRDP_Password, c->password);
	if (c->client_name && *c->client_name)
		ok = ok && freerdp_settings_set_string(s, FreeRDP_ClientHostname, c->client_name);
	/* FreeRDP's known_hosts would accept a certificate stored there (e.g. by the Swift app) without
	 * asking; the engine's own store decides, so FreeRDP gets a folder of its own. */
	ok = ok && freerdp_settings_set_string(s, FreeRDP_ConfigPath, c->config_dir);
	ok = ok && freerdp_settings_set_uint32(s, FreeRDP_DesktopWidth, c->width);
	ok = ok && freerdp_settings_set_uint32(s, FreeRDP_DesktopHeight, c->height);
	ok = ok && freerdp_settings_set_uint32(s, FreeRDP_ColorDepth, 32);
	ok = ok && freerdp_settings_set_bool(s, FreeRDP_ConsoleSession, c->console_session ? TRUE : FALSE);
	ok = ok && freerdp_settings_set_bool(s, FreeRDP_SupportDisplayControl, TRUE);
	ok = ok && freerdp_settings_set_bool(s, FreeRDP_DynamicResolutionUpdate, TRUE);
	ok = ok && freerdp_settings_set_bool(s, FreeRDP_SupportGraphicsPipeline, TRUE);
	ok = ok && freerdp_settings_set_bool(s, FreeRDP_RedirectClipboard, c->clipboard ? TRUE : FALSE);
	/* The engine reconnects itself (with its own attempts and events). */
	ok = ok && freerdp_settings_set_bool(s, FreeRDP_AutoReconnectionEnabled, FALSE);
	ok = ok && freerdp_settings_set_bool(s, FreeRDP_IgnoreCertificate, FALSE);
	ok = ok && freerdp_settings_set_bool(s, FreeRDP_CertificateCallbackPreferPEM, TRUE);
	return ok;
}

/* Windows sockets need WSAStartup before FreeRDP resolves the host; without it getaddrinfo fails and the
 * connection ends with "DNS host name not found". FreeRDP's own clients do this in main(). Once per process,
 * never cleaned up. */
#ifdef _WIN32
static INIT_ONCE winsock_once = INIT_ONCE_STATIC_INIT;

static BOOL init_winsock(PINIT_ONCE once, PVOID param, PVOID* context)
{
	WINPR_UNUSED(once);
	WINPR_UNUSED(param);
	WINPR_UNUSED(context);
	WSADATA data;
	return WSAStartup(MAKEWORD(2, 2), &data) == 0;
}
#endif

int rdshim_connect(RdShim* shim, const RdShimConfig* config)
{
#ifdef _WIN32
	if (!InitOnceExecuteOnce(&winsock_once, init_winsock, NULL, NULL))
		return 0;
#endif
	free_context(shim);
	RDP_CLIENT_ENTRY_POINTS entry = { 0 };
	shim_client_entry(&entry);
	rdpContext* context = freerdp_client_context_new(&entry);
	if (!context)
		return 0;
	((ShimContext*)context)->shim = shim;
	if (!apply_config(context->settings, config) || freerdp_client_start(context) != 0)
	{
		freerdp_client_context_free(context);
		return 0;
	}
	EnterCriticalSection(&shim->lock);
	shim->context = context;
	LeaveCriticalSection(&shim->lock);
	return freerdp_connect(context->instance) ? 1 : 0;
}

void rdshim_abort(RdShim* shim)
{
	EnterCriticalSection(&shim->lock);
	if (shim->context)
		freerdp_abort_connect_context(shim->context);
	LeaveCriticalSection(&shim->lock);
	SetEvent(shim->wake);
}

void rdshim_wake(RdShim* shim)
{
	SetEvent(shim->wake);
}

int rdshim_poll(RdShim* shim, uint32_t timeout_ms)
{
	rdpContext* context = shim->context;
	if (!context || freerdp_shall_disconnect_context(context))
		return 0;
	HANDLE handles[MAXIMUM_WAIT_OBJECTS] = { 0 };
	DWORD count = freerdp_get_event_handles(context, handles, ARRAYSIZE(handles) - 1);
	if (count == 0)
		return 0;
	handles[count++] = shim->wake;
	if (WaitForMultipleObjects(count, handles, FALSE, timeout_ms) == WAIT_FAILED)
		return 0;
	ResetEvent(shim->wake);
	if (!freerdp_check_event_handles(context))
		return 0;
	return freerdp_shall_disconnect_context(context) ? 0 : 1;
}

void rdshim_disconnect(RdShim* shim)
{
	if (shim->context)
		freerdp_disconnect(shim->context->instance);
	free_context(shim);
}

uint32_t rdshim_last_error(RdShim* shim)
{
	return shim->context ? freerdp_get_last_error(shim->context) : 0;
}

uint32_t rdshim_error_info(RdShim* shim)
{
	return shim->context ? freerdp_error_info(shim->context->instance) : 0;
}

const char* rdshim_last_error_string(uint32_t code)
{
	return freerdp_get_last_error_string(code);
}

const char* rdshim_error_info_string(uint32_t code)
{
	return freerdp_get_error_info_string(code);
}

int rdshim_lock_framebuffer(RdShim* shim, const uint8_t** data, uint32_t* stride, uint32_t* width,
                            uint32_t* height)
{
	rdpContext* context = shim->context;
	if (!context || !context->update)
		return 0;
	rdp_update_lock(context->update);
	rdpGdi* gdi = context->gdi;
	if (!gdi || !gdi->primary_buffer)
	{
		rdp_update_unlock(context->update);
		return 0;
	}
	*data = gdi->primary_buffer;
	*stride = gdi->stride;
	*width = (uint32_t)gdi->width;
	*height = (uint32_t)gdi->height;
	return 1;
}

void rdshim_unlock_framebuffer(RdShim* shim)
{
	if (shim->context && shim->context->update)
		rdp_update_unlock(shim->context->update);
}

void rdshim_lock_callbacks(RdShim* shim)
{
	EnterCriticalSection(&shim->cb_lock);
}

void rdshim_unlock_callbacks(RdShim* shim)
{
	LeaveCriticalSection(&shim->cb_lock);
}

/* ---- input ----------------------------------------------------------------------------------- */

static rdpInput* input_of(RdShim* shim)
{
	return shim->context ? shim->context->input : NULL;
}

int rdshim_send_key(RdShim* shim, uint16_t flags, uint8_t code)
{
	rdpInput* input = input_of(shim);
	return input && freerdp_input_send_keyboard_event(input, flags, code);
}

int rdshim_send_unicode(RdShim* shim, uint16_t flags, uint16_t code)
{
	rdpInput* input = input_of(shim);
	return input && freerdp_input_send_unicode_keyboard_event(input, flags, code);
}

int rdshim_send_mouse(RdShim* shim, uint16_t flags, uint16_t x, uint16_t y)
{
	rdpInput* input = input_of(shim);
	return input && freerdp_input_send_mouse_event(input, flags, x, y);
}

int rdshim_send_mouse_ex(RdShim* shim, uint16_t flags, uint16_t x, uint16_t y)
{
	rdpInput* input = input_of(shim);
	return input && freerdp_input_send_extended_mouse_event(input, flags, x, y);
}

int rdshim_resize(RdShim* shim, uint32_t width, uint32_t height)
{
	DispClientContext* disp = shim->disp;
	if (!disp || !disp->SendMonitorLayout)
		return 0;
	DISPLAY_CONTROL_MONITOR_LAYOUT layout = { 0 };
	layout.Flags = DISPLAY_CONTROL_MONITOR_PRIMARY;
	layout.Width = width;
	layout.Height = height;
	layout.Orientation = ORIENTATION_LANDSCAPE;
	layout.DesktopScaleFactor = 100;
	layout.DeviceScaleFactor = 100;
	return disp->SendMonitorLayout(disp, 1, &layout) == CHANNEL_RC_OK;
}

/* ---- cliprdr, client to server -------------------------------------------------------------- */

int rdshim_clip_capabilities(RdShim* shim, uint32_t general_flags)
{
	CliprdrClientContext* clip = shim->clip;
	if (!clip)
		return 0;
	CLIPRDR_GENERAL_CAPABILITY_SET general = { 0 };
	general.capabilitySetType = CB_CAPSTYPE_GENERAL;
	general.capabilitySetLength = CB_CAPSTYPE_GENERAL_LEN;
	general.version = CB_CAPS_VERSION_2;
	general.generalFlags = general_flags;
	CLIPRDR_CAPABILITIES caps = { 0 };
	caps.cCapabilitiesSets = 1;
	caps.capabilitySets = (CLIPRDR_CAPABILITY_SET*)&general;
	return clip->ClientCapabilities(clip, &caps) == CHANNEL_RC_OK;
}

int rdshim_clip_format_list(RdShim* shim, const uint32_t* ids, const char* const* names,
                            uint32_t count)
{
	CliprdrClientContext* clip = shim->clip;
	if (!clip)
		return 0;
	CLIPRDR_FORMAT* formats = count ? calloc(count, sizeof(CLIPRDR_FORMAT)) : NULL;
	if (count && !formats)
		return 0;
	for (uint32_t i = 0; i < count; i++)
	{
		formats[i].formatId = ids[i];
		formats[i].formatName = (char*)names[i];
	}
	CLIPRDR_FORMAT_LIST list = { 0 };
	list.common.msgType = CB_FORMAT_LIST;
	list.numFormats = count;
	list.formats = formats;
	const UINT rc = clip->ClientFormatList(clip, &list);
	free(formats);
	return rc == CHANNEL_RC_OK;
}

int rdshim_clip_format_list_response(RdShim* shim, int ok)
{
	CliprdrClientContext* clip = shim->clip;
	if (!clip)
		return 0;
	CLIPRDR_FORMAT_LIST_RESPONSE response = { 0 };
	response.common.msgType = CB_FORMAT_LIST_RESPONSE;
	response.common.msgFlags = ok ? CB_RESPONSE_OK : CB_RESPONSE_FAIL;
	return clip->ClientFormatListResponse(clip, &response) == CHANNEL_RC_OK;
}

int rdshim_clip_data_request(RdShim* shim, uint32_t format_id)
{
	CliprdrClientContext* clip = shim->clip;
	if (!clip)
		return 0;
	CLIPRDR_FORMAT_DATA_REQUEST request = { 0 };
	request.common.msgType = CB_FORMAT_DATA_REQUEST;
	request.requestedFormatId = format_id;
	return clip->ClientFormatDataRequest(clip, &request) == CHANNEL_RC_OK;
}

int rdshim_clip_data_response(RdShim* shim, int ok, const uint8_t* data, uint32_t len)
{
	CliprdrClientContext* clip = shim->clip;
	if (!clip)
		return 0;
	CLIPRDR_FORMAT_DATA_RESPONSE response = { 0 };
	response.common.msgType = CB_FORMAT_DATA_RESPONSE;
	response.common.msgFlags = ok ? CB_RESPONSE_OK : CB_RESPONSE_FAIL;
	response.common.dataLen = ok ? len : 0;
	response.requestedFormatData = ok ? data : NULL;
	return clip->ClientFormatDataResponse(clip, &response) == CHANNEL_RC_OK;
}

int rdshim_clip_contents_request(RdShim* shim, uint32_t stream_id, uint32_t list_index,
                                 uint32_t flags, uint64_t position, uint32_t requested,
                                 int has_data_id, uint32_t data_id)
{
	CliprdrClientContext* clip = shim->clip;
	if (!clip)
		return 0;
	CLIPRDR_FILE_CONTENTS_REQUEST request = { 0 };
	request.common.msgType = CB_FILECONTENTS_REQUEST;
	request.streamId = stream_id;
	request.listIndex = list_index;
	request.dwFlags = flags;
	request.nPositionLow = (UINT32)(position & 0xFFFFFFFF);
	request.nPositionHigh = (UINT32)(position >> 32);
	request.cbRequested = requested;
	request.haveClipDataId = has_data_id ? TRUE : FALSE;
	request.clipDataId = data_id;
	return clip->ClientFileContentsRequest(clip, &request) == CHANNEL_RC_OK;
}

int rdshim_clip_contents_response(RdShim* shim, uint32_t stream_id, int ok, const uint8_t* data,
                                  uint32_t len)
{
	CliprdrClientContext* clip = shim->clip;
	if (!clip)
		return 0;
	CLIPRDR_FILE_CONTENTS_RESPONSE response = { 0 };
	response.common.msgType = CB_FILECONTENTS_RESPONSE;
	response.common.msgFlags = ok ? CB_RESPONSE_OK : CB_RESPONSE_FAIL;
	response.common.dataLen = 4 + (ok ? len : 0);
	response.streamId = stream_id;
	response.cbRequested = ok ? len : 0;
	response.requestedData = ok ? data : NULL;
	return clip->ClientFileContentsResponse(clip, &response) == CHANNEL_RC_OK;
}
