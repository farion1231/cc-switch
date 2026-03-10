/*
 * Copyright (C) the libgit2 contributors. All rights reserved.
 *
 * This file is part of libgit2, distributed under the GNU GPL v2 with
 * a Linking Exception. For full terms see the included COPYING file.
 */
#ifndef INCLUDE_git_proxy_h__
#define INCLUDE_git_proxy_h__

#include "common.h"

#include "cert.h"
#include "credential.h"

GIT_BEGIN_DECL

/**
 * The type of proxy to use.
 */
typedef enum {
	/**
	 * Do not attempt to connect through a proxy
	 *
	 * If built against libcurl, it itself may attempt to connect
	 * to a proxy if the environment variables specify it.
	 */
	GIT_PROXY_NONE,
	/**
	 * Try to auto-detect the proxy from the git configuration.
	 */
	GIT_PROXY_AUTO,
	/**
	 * Connect via the URL given in the options
	 */
	GIT_PROXY_SPECIFIED
} git_proxy_t;

/**
 * Options for connecting through a proxy
 *
 * Note that not all types may be supported, depending on the platform
 * and compilation options.
 */
typedef struct {
	unsigned int version;

	/**
	 * The type of proxy to use, by URL, auto-detect.
	 */
	git_proxy_t type;

	/**
	 * The URL of the proxy.
	 */
	const char *url;

	/**
	 * This will be called if the remote host requires
	 * authentication in order to connect to it.
	 *
	 * Returning GIT_PASSTHROUGH will make libgit2 behave as
	 * though this field isn't set.
	 */
	git_credential_acquire_cb credentials;

	/**
	 * If cert verification fails, this will be called to let the
	 * user make the final decision of whether to allow the
	 * connection to proceed. Returns 0 to allow the connection
	 * or a negative value to indicate an error.
	 */
	git_transport_certificate_check_cb certificate_check;

	/**
	 * Payload to be provided to the credentials and certificate
	 * check callbacks.
	 */
	void *payload;
} git_proxy_options;

#define GIT_PROXY_OPTIONS_VERSION 1
#define GIT_PROXY_OPTIONS_INIT {GIT_PROXY_OPTIONS_VERSION}

/**
 * Initialize git_proxy_options structure
 *
 * Initializes a `git_proxy_options` with default values. Equivalent to
 * creating an instance with `GIT_PROXY_OPTIONS_INIT`.
 *
 * @param opts The `git_proxy_options` struct to initialize.
 * @param version The struct version; pass `GIT_PROXY_OPTIONS_VERSION`.
 * @return Zero on success; -1 on failure.
 */
GIT_EXTERN(int) git_proxy_options_init(git_proxy_options *opts, unsigned int version);

GIT_END_DECL

#endif
