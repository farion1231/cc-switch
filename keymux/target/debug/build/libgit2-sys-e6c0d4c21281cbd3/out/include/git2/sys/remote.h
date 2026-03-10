/*
 * Copyright (C) the libgit2 contributors. All rights reserved.
 *
 * This file is part of libgit2, distributed under the GNU GPL v2 with
 * a Linking Exception. For full terms see the included COPYING file.
 */

#ifndef INCLUDE_sys_git_remote_h
#define INCLUDE_sys_git_remote_h

#include "git2/remote.h"

/**
 * @file git2/sys/remote.h
 * @brief Low-level remote functionality for custom transports
 * @defgroup git_remote Low-level remote functionality
 * @ingroup Git
 * @{
*/

GIT_BEGIN_DECL

typedef enum {
	/** Remote supports fetching an advertised object by ID. */
	GIT_REMOTE_CAPABILITY_TIP_OID = (1 << 0),

	/** Remote supports fetching an individual reachable object. */
	GIT_REMOTE_CAPABILITY_REACHABLE_OID = (1 << 1),
} git_remote_capability_t;

/**
 * Disposes libgit2-initialized fields from a git_remote_connect_options.
 * This should only be used for git_remote_connect_options returned by
 * git_transport_remote_connect_options.
 *
 * Note that this does not free the `git_remote_connect_options` itself, just
 * the memory pointed to by it.
 *
 * @param opts The `git_remote_connect_options` struct to dispose.
 */
GIT_EXTERN(void) git_remote_connect_options_dispose(
		git_remote_connect_options *opts);

/** @} */
GIT_END_DECL
#endif
