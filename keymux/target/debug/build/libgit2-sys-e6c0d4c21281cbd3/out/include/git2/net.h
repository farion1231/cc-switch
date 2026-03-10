/*
 * Copyright (C) the libgit2 contributors. All rights reserved.
 *
 * This file is part of libgit2, distributed under the GNU GPL v2 with
 * a Linking Exception. For full terms see the included COPYING file.
 */
#ifndef INCLUDE_git_net_h__
#define INCLUDE_git_net_h__

#include "common.h"
#include "oid.h"
#include "types.h"

/**
 * @file git2/net.h
 * @brief Git networking declarations
 * @ingroup Git
 * @{
 */
GIT_BEGIN_DECL

#define GIT_DEFAULT_PORT "9418"

/**
 * Direction of the connection.
 *
 * We need this because we need to know whether we should call
 * git-upload-pack or git-receive-pack on the remote end when get_refs
 * gets called.
 */
typedef enum {
	GIT_DIRECTION_FETCH = 0,
	GIT_DIRECTION_PUSH  = 1
} git_direction;

/**
 * Description of a reference advertised by a remote server, given out
 * on `ls` calls.
 */
struct git_remote_head {
	int local; /* available locally */
	git_oid oid;
	git_oid loid;
	char *name;
	/**
	 * If the server send a symref mapping for this ref, this will
	 * point to the target.
	 */
	char *symref_target;
};

/** @} */
GIT_END_DECL
#endif
