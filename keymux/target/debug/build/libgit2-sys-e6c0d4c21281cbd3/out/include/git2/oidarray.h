/*
 * Copyright (C) the libgit2 contributors. All rights reserved.
 *
 * This file is part of libgit2, distributed under the GNU GPL v2 with
 * a Linking Exception. For full terms see the included COPYING file.
 */
#ifndef INCLUDE_git_oidarray_h__
#define INCLUDE_git_oidarray_h__

#include "common.h"
#include "oid.h"

GIT_BEGIN_DECL

/** Array of object ids */
typedef struct git_oidarray {
	git_oid *ids;
	size_t count;
} git_oidarray;

/**
 * Free the object IDs contained in an oid_array.  This method should
 * be called on `git_oidarray` objects that were provided by the
 * library.  Not doing so will result in a memory leak.
 *
 * This does not free the `git_oidarray` itself, since the library will
 * never allocate that object directly itself.
 *
 * @param array git_oidarray from which to free oid data
 */
GIT_EXTERN(void) git_oidarray_dispose(git_oidarray *array);

/** @} */
GIT_END_DECL

#endif

