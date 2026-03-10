/*
 * Copyright (C) the libgit2 contributors. All rights reserved.
 *
 * This file is part of libgit2, distributed under the GNU GPL v2 with
 * a Linking Exception. For full terms see the included COPYING file.
 */
#ifndef INCLUDE_git_buf_h__
#define INCLUDE_git_buf_h__

#include "common.h"

/**
 * @file git2/buffer.h
 * @brief Buffer export structure
 *
 * @ingroup Git
 * @{
 */
GIT_BEGIN_DECL

/**
 * A data buffer for exporting data from libgit2
 *
 * Sometimes libgit2 wants to return an allocated data buffer to the
 * caller and have the caller take responsibility for freeing that memory.
 * To make ownership clear in these cases, libgit2 uses  `git_buf` to
 * return this data.  Callers should use `git_buf_dispose()` to release
 * the memory when they are done.
 *
 * A `git_buf` contains a pointer to a NUL-terminated C string, and
 * the length of the string (not including the NUL terminator).
 */
typedef struct {
	/**
	 * The buffer contents.  `ptr` points to the start of the buffer
	 * being returned.  The buffer's length (in bytes) is specified
	 * by the `size` member of the structure, and contains a NUL
	 * terminator at position `(size + 1)`.
	 */
	char *ptr;

	/**
	 * This field is reserved and unused.
	 */
	size_t reserved;

	/**
	 * The length (in bytes) of the buffer pointed to by `ptr`,
	 * not including a NUL terminator.
	 */
	size_t size;
} git_buf;

/**
 * Use to initialize a `git_buf` before passing it to a function that
 * will populate it.
 */
#define GIT_BUF_INIT { NULL, 0, 0 }

/**
 * Free the memory referred to by the git_buf.
 *
 * Note that this does not free the `git_buf` itself, just the memory
 * pointed to by `buffer->ptr`.
 *
 * @param buffer The buffer to deallocate
 */
GIT_EXTERN(void) git_buf_dispose(git_buf *buffer);

GIT_END_DECL

/** @} */

#endif
