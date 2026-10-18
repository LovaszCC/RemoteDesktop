/*
 * A small C interface over FreeRDP for the Rust session engine (crates/rd-rdp). The shim owns every
 * FreeRDP structure; Rust only sees this header (declared again in src/lib.rs).
 *
 * Threading: everything except rdshim_abort() and rdshim_wake() must be called on one thread, the
 * session thread. The callbacks run on that thread from inside rdshim_connect() and rdshim_poll(), and
 * on FreeRDP's channel threads (cliprdr, the graphics pipeline), always under the callback lock.
 */
#ifndef RDSHIM_H
#define RDSHIM_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct RdShim RdShim;

/* A dirty rectangle, exclusive right/bottom. */
typedef struct
{
	int32_t x;
	int32_t y;
	int32_t width;
	int32_t height;
} RdShimRect;

typedef struct
{
	void* user;
	/* The certificate wasn't verified by OpenSSL (or its name doesn't match): PEM, flags
	 * VERIFY_CERT_FLAG_*. Returns 1 to accept (for this connection), 0 to reject. */
	int (*verify_certificate)(void* user, const uint8_t* pem, size_t pem_len, const char* host,
	                          uint16_t port, uint32_t flags);
	/* NLA needs credentials it doesn't have: answer with rdshim_set_credentials() and return 1, or
	 * return 0 to cancel. */
	int (*authenticate)(void* user, RdShim* shim);
	/* Connected and the desktop is set up (or reactivated) at this size. */
	void (*desktop)(void* user, uint32_t width, uint32_t height);
	void (*painted)(void* user, const RdShimRect* rects, uint32_t count);
	/* RGBA (not premultiplied), width * height * 4 bytes. */
	void (*pointer_shape)(void* user, const uint8_t* rgba, uint32_t width, uint32_t height,
	                      uint32_t hotspot_x, uint32_t hotspot_y);
	void (*pointer_hidden)(void* user);
	void (*pointer_default)(void* user);
	/* Display Control is open: resizes can go out. */
	void (*display_control_ready)(void* user);

	/* cliprdr, server to client ([MS-RDPECLIP]). */
	void (*clip_monitor_ready)(void* user);
	void (*clip_capabilities)(void* user, uint32_t general_flags);
	/* names[i] may be NULL. */
	void (*clip_format_list)(void* user, const uint32_t* ids, const char* const* names,
	                         uint32_t count);
	void (*clip_data_request)(void* user, uint32_t format_id);
	void (*clip_data_response)(void* user, int ok, const uint8_t* data, uint32_t len);
	void (*clip_contents_request)(void* user, uint32_t stream_id, uint32_t list_index,
	                              uint32_t flags, uint64_t position, uint32_t requested,
	                              int has_data_id, uint32_t data_id);
	void (*clip_contents_response)(void* user, uint32_t stream_id, int ok, const uint8_t* data,
	                               uint32_t len);
	void (*clip_lock)(void* user, uint32_t data_id, int locked);
} RdShimCallbacks;

typedef struct
{
	const char* host;
	uint16_t port;
	const char* username;
	const char* domain;
	const char* password;
	const char* client_name;
	/* FreeRDP's own folder (its certificate store and license cache); never the user's ~/.config. */
	const char* config_dir;
	uint32_t width;
	uint32_t height;
	int console_session;
	int clipboard;
} RdShimConfig;

const char* rdshim_version(void);

RdShim* rdshim_new(const RdShimCallbacks* callbacks);
void rdshim_free(RdShim* shim);

/* Connects (blocking; the callbacks may run meanwhile). 1 when connected. */
int rdshim_connect(RdShim* shim, const RdShimConfig* config);
/* Inside the authenticate callback: the credentials to use. */
int rdshim_set_credentials(RdShim* shim, const char* username, const char* domain,
                           const char* password);
/* Any thread: stops a running connect or session. */
void rdshim_abort(RdShim* shim);
/* Any thread: makes a waiting rdshim_poll() return. */
void rdshim_wake(RdShim* shim);
/* Waits up to timeout_ms for network data (or a wake) and processes it. 1 while connected. */
int rdshim_poll(RdShim* shim, uint32_t timeout_ms);
/* Sends the disconnect and frees the connection (the shim can connect again). */
void rdshim_disconnect(RdShim* shim);

/* freerdp_get_last_error() and the server's error info, with their texts. */
uint32_t rdshim_last_error(RdShim* shim);
uint32_t rdshim_error_info(RdShim* shim);
const char* rdshim_last_error_string(uint32_t code);
const char* rdshim_error_info_string(uint32_t code);

/* The desktop: RGBA32 rows. 1 when there is one; then FreeRDP's update lock is held (painting and
 * resizing wait) until rdshim_unlock_framebuffer(). */
int rdshim_lock_framebuffer(RdShim* shim, const uint8_t** data, uint32_t* stride, uint32_t* width,
                            uint32_t* height);
void rdshim_unlock_framebuffer(RdShim* shim);

/* The lock every callback runs under (FreeRDP calls some from its channel threads): hold it while
 * touching state the callbacks write. */
void rdshim_lock_callbacks(RdShim* shim);
void rdshim_unlock_callbacks(RdShim* shim);

/* Input with protocol flags (KBD_FLAGS_*, PTR_FLAGS_*, PTR_XFLAGS_*). */
int rdshim_send_key(RdShim* shim, uint16_t flags, uint8_t code);
int rdshim_send_unicode(RdShim* shim, uint16_t flags, uint16_t code);
int rdshim_send_mouse(RdShim* shim, uint16_t flags, uint16_t x, uint16_t y);
int rdshim_send_mouse_ex(RdShim* shim, uint16_t flags, uint16_t x, uint16_t y);

/* A Display Control monitor layout. 1 when sent (0: the channel isn't open). */
int rdshim_resize(RdShim* shim, uint32_t width, uint32_t height);

/* cliprdr, client to server. 1 when sent. */
int rdshim_clip_capabilities(RdShim* shim, uint32_t general_flags);
int rdshim_clip_format_list(RdShim* shim, const uint32_t* ids, const char* const* names,
                            uint32_t count);
int rdshim_clip_format_list_response(RdShim* shim, int ok);
int rdshim_clip_data_request(RdShim* shim, uint32_t format_id);
int rdshim_clip_data_response(RdShim* shim, int ok, const uint8_t* data, uint32_t len);
int rdshim_clip_contents_request(RdShim* shim, uint32_t stream_id, uint32_t list_index,
                                 uint32_t flags, uint64_t position, uint32_t requested,
                                 int has_data_id, uint32_t data_id);
int rdshim_clip_contents_response(RdShim* shim, uint32_t stream_id, int ok, const uint8_t* data,
                                  uint32_t len);

#ifdef __cplusplus
}
#endif

#endif /* RDSHIM_H */
