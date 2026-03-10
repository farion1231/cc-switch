/*
 * Copyright (C) the libgit2 contributors. All rights reserved.
 *
 * This file is part of libgit2, distributed under the GNU GPL v2 with
 * a Linking Exception. For full terms see the included COPYING file.
 */
#ifndef INCLUDE_sys_hashsig_h__
#define INCLUDE_sys_hashsig_h__

#include "git2/common.h"

GIT_BEGIN_DECL

/**
 * Similarity signature of arbitrary text content based on line hashes
 */
typedef struct git_hashsig git_hashsig;

/**
 * Options for hashsig computation
 *
 * The options GIT_HASHSIG_NORMAL, GIT_HASHSIG_IGNORE_WHITESPACE,
 * GIT_HASHSIG_SMART_WHITESPACE are exclusive and should not be combined.
 */
typedef enum {
	/**
	 * Use all data
	 */
	GIT_HASHSIG_NORMAL = 0,

	/**
	 * Ignore whitespace
	 */
	GIT_HASHSIG_IGNORE_WHITESPACE = (1 << 0),

	/**
	 * Ignore \r and all space after \n
	 */
	GIT_HASHSIG_SMART_WHITESPACE = (1 << 1),

	/**
	 * Allow hashing of small files
	 */
	GIT_HASHSIG_ALLOW_SMALL_FILES = (1 << 2)
} git_hashsig_option_t;

/**
 * Compute a similarity signature for a text buffer
 *
 * If you have passed the option GIT_HASHSIG_IGNORE_WHITESPACE, then the
 * whitespace will be removed from the buffer while it is being processed,
 * modifying the buffer in place. Sorry about that!
 *
 * @param out The computed similarity signature.
 * @param buf The input buffer.
 * @param buflen The input buffer size.
 * @param opts The signature computation options (see above).
 * @return 0 on success, GIT_EBUFS if the buffer doesn't contain enough data to
 * compute a valid signature (unless GIT_HASHSIG_ALLOW_SMALL_FILES is set), or
 * error code.
 */
GIT_EXTERN(int) git_hashsig_create(
	git_hashsig **out,
	const char *buf,
	size_t buflen,
	git_hashsig_option_t opts);

/**
 * Compute a similarity signature for a text file
 *
 * This walks through the file, only loading a maximum of 4K of file data at
 * a time. Otherwise, it acts just like `git_hashsig_create`.
 *
 * @param out The computed similarity signature.
 * @param path The path to the input file.
 * @param opts The signature computation options (see above).
 * @return 0 on success, GIT_EBUFS if the buffer doesn't contain enough data to
 * compute a valid signature (unless GIT_HASHSIG_ALLOW_SMALL_FILES is set), or
 * error code.
 */
GIT_EXTERN(int) git_hashsig_create_fromfile(
	git_hashsig **out,
	const char *path,
	git_hashsig_option_t opts);

/**
 * Release memory for a content similarity signature
 *
 * @param sig The similarity signature to free.
 */
GIT_EXTERN(void) git_hashsig_free(git_hashsig *sig);

/**
 * Measure similarity score between two similarity signatures
 *
 * @param a The first similarity signature to compare.
 * @param b The second similarity signature to compare.
 * @return [0 to 100] on success as the similarity score, or error code.
 */
GIT_EXTERN(int) git_hashsig_compare(
	const git_hashsig *a,
	const git_hashsig *b);

GIT_END_DECL

#endif
