/*
 * Copyright (C) the libgit2 contributors. All rights reserved.
 *
 * This file is part of libgit2, distributed under the GNU GPL v2 with
 * a Linking Exception. For full terms see the included COPYING file.
 */
#ifndef INCLUDE_sys_git_stream_h__
#define INCLUDE_sys_git_stream_h__

#include "git2/common.h"
#include "git2/types.h"
#include "git2/proxy.h"

GIT_BEGIN_DECL

#define GIT_STREAM_VERSION 1

/**
 * Every stream must have this struct as its first element, so the
 * API can talk to it. You'd define your stream as
 *
 *     struct my_stream {
 *             git_stream parent;
 *             ...
 *     }
 *
 * and fill the functions
 */
typedef struct git_stream {
	int version;

	int encrypted : 1,
	    proxy_support : 1;

	/**
	 * Timeout for read and write operations; can be set to `0` to
	 * block indefinitely.
	 */
	int timeout;

	/**
	 * Timeout to connect to the remote server; can be set to `0`
	 * to use the system defaults. This can be shorter than the
	 * system default - often 75 seconds - but cannot be longer.
	 */
	int connect_timeout;

	int GIT_CALLBACK(connect)(struct git_stream *);
	int GIT_CALLBACK(certificate)(git_cert **, struct git_stream *);
	int GIT_CALLBACK(set_proxy)(struct git_stream *, const git_proxy_options *proxy_opts);
	ssize_t GIT_CALLBACK(read)(struct git_stream *, void *, size_t);
	ssize_t GIT_CALLBACK(write)(struct git_stream *, const char *, size_t, int);
	int GIT_CALLBACK(close)(struct git_stream *);
	void GIT_CALLBACK(free)(struct git_stream *);
} git_stream;

typedef struct {
	/** The `version` field should be set to `GIT_STREAM_VERSION`. */
	int version;

	/**
	 * Called to create a new connection to a given host.
	 *
	 * @param out The created stream
	 * @param host The hostname to connect to; may be a hostname or
	 *             IP address
	 * @param port The port to connect to; may be a port number or
	 *             service name
	 * @return 0 or an error code
	 */
	int GIT_CALLBACK(init)(git_stream **out, const char *host, const char *port);

	/**
	 * Called to create a new connection on top of the given stream.  If
	 * this is a TLS stream, then this function may be used to proxy a
	 * TLS stream over an HTTP CONNECT session.  If this is unset, then
	 * HTTP CONNECT proxies will not be supported.
	 *
	 * @param out The created stream
	 * @param in An existing stream to add TLS to
	 * @param host The hostname that the stream is connected to,
	 *             for certificate validation
	 * @return 0 or an error code
	 */
	int GIT_CALLBACK(wrap)(git_stream **out, git_stream *in, const char *host);
} git_stream_registration;

/**
 * The type of stream to register.
 */
typedef enum {
	/** A standard (non-TLS) socket. */
	GIT_STREAM_STANDARD = 1,

	/** A TLS-encrypted socket. */
	GIT_STREAM_TLS = 2
} git_stream_t;

/**
 * Register stream constructors for the library to use
 *
 * If a registration structure is already set, it will be overwritten.
 * Pass `NULL` in order to deregister the current constructor and return
 * to the system defaults.
 *
 * The type parameter may be a bitwise AND of types.
 *
 * @param type the type or types of stream to register
 * @param registration the registration data
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_stream_register(
	git_stream_t type, git_stream_registration *registration);

#ifndef GIT_DEPRECATE_HARD

/** @name Deprecated TLS Stream Registration Functions
 *
 * These functions are retained for backward compatibility.  The newer
 * versions of these values should be preferred in all new code.
 *
 * There is no plan to remove these backward compatibility values at
 * this time.
 */
/**@{*/

/**
 * @deprecated Provide a git_stream_registration to git_stream_register
 * @see git_stream_registration
 */
typedef int GIT_CALLBACK(git_stream_cb)(git_stream **out, const char *host, const char *port);

/**
 * Register a TLS stream constructor for the library to use.  This stream
 * will not support HTTP CONNECT proxies.  This internally calls
 * `git_stream_register` and is preserved for backward compatibility.
 *
 * This function is deprecated, but there is no plan to remove this
 * function at this time.
 *
 * @deprecated Provide a git_stream_registration to git_stream_register
 * @see git_stream_register
 */
GIT_EXTERN(int) git_stream_register_tls(git_stream_cb ctor);

/**@}*/

#endif

GIT_END_DECL

#endif
