/*
 * Copyright (C) the libgit2 contributors. All rights reserved.
 *
 * This file is part of libgit2, distributed under the GNU GPL v2 with
 * a Linking Exception. For full terms see the included COPYING file.
 */
#ifndef INCLUDE_git_apply_h__
#define INCLUDE_git_apply_h__

#include "common.h"
#include "types.h"
#include "oid.h"
#include "diff.h"

/**
 * @file git2/apply.h
 * @brief Git patch application routines
 * @defgroup git_apply Git patch application routines
 * @ingroup Git
 * @{
 */
GIT_BEGIN_DECL

/**
 * When applying a patch, callback that will be made per delta (file).
 *
 * When the callback:
 * - returns < 0, the apply process will be aborted.
 * - returns > 0, the delta will not be applied, but the apply process
 *      continues
 * - returns 0, the delta is applied, and the apply process continues.
 *
 * @param delta The delta to be applied
 * @param payload User-specified payload
 * @return 0 if the delta is applied, < 0 if the apply process will be aborted
 *	or > 0 if the delta will not be applied.
 */
typedef int GIT_CALLBACK(git_apply_delta_cb)(
	const git_diff_delta *delta,
	void *payload);

/**
 * When applying a patch, callback that will be made per hunk.
 *
 * When the callback:
 * - returns < 0, the apply process will be aborted.
 * - returns > 0, the hunk will not be applied, but the apply process
 *      continues
 * - returns 0, the hunk is applied, and the apply process continues.
 *
 * @param hunk The hunk to be applied
 * @param payload User-specified payload
 * @return 0 if the hunk is applied, < 0 if the apply process will be aborted
 *	or > 0 if the hunk will not be applied.
 */
typedef int GIT_CALLBACK(git_apply_hunk_cb)(
	const git_diff_hunk *hunk,
	void *payload);

/** Flags controlling the behavior of git_apply */
typedef enum {
	/**
	 * Don't actually make changes, just test that the patch applies.
	 * This is the equivalent of `git apply --check`.
	 */
	GIT_APPLY_CHECK = (1 << 0)
} git_apply_flags_t;

/**
 * Apply options structure
 *
 * Initialize with `GIT_APPLY_OPTIONS_INIT`. Alternatively, you can
 * use `git_apply_options_init`.
 *
 * @see git_apply_to_tree, git_apply
 */
typedef struct {
	unsigned int version; /**< The version */

	/** When applying a patch, callback that will be made per delta (file). */
	git_apply_delta_cb delta_cb;

	/** When applying a patch, callback that will be made per hunk. */
	git_apply_hunk_cb hunk_cb;

	/** Payload passed to both delta_cb & hunk_cb. */
	void *payload;

	/** Bitmask of git_apply_flags_t */
	unsigned int flags;
} git_apply_options;

#define GIT_APPLY_OPTIONS_VERSION 1
#define GIT_APPLY_OPTIONS_INIT {GIT_APPLY_OPTIONS_VERSION}

/**
 * Initialize git_apply_options structure
 *
 * Initialize a `git_apply_options` with default values. Equivalent to creating
 * an instance with GIT_APPLY_OPTIONS_INIT.
 *
 * @param opts The `git_apply_options` struct to initialize.
 * @param version The struct version; pass `GIT_APPLY_OPTIONS_VERSION`
 * @return 0 on success or -1 on failure.
 */
GIT_EXTERN(int) git_apply_options_init(git_apply_options *opts, unsigned int version);

/**
 * Apply a `git_diff` to a `git_tree`, and return the resulting image
 * as an index.
 *
 * @param out the postimage of the application
 * @param repo the repository to apply
 * @param preimage the tree to apply the diff to
 * @param diff the diff to apply
 * @param options the options for the apply (or null for defaults)
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_apply_to_tree(
	git_index **out,
	git_repository *repo,
	git_tree *preimage,
	git_diff *diff,
	const git_apply_options *options);

/** Possible application locations for git_apply */
typedef enum {
	/**
	 * Apply the patch to the workdir, leaving the index untouched.
	 * This is the equivalent of `git apply` with no location argument.
	 */
	GIT_APPLY_LOCATION_WORKDIR = 0,

	/**
	 * Apply the patch to the index, leaving the working directory
	 * untouched.  This is the equivalent of `git apply --cached`.
	 */
	GIT_APPLY_LOCATION_INDEX = 1,

	/**
	 * Apply the patch to both the working directory and the index.
	 * This is the equivalent of `git apply --index`.
	 */
	GIT_APPLY_LOCATION_BOTH = 2
} git_apply_location_t;

/**
 * Apply a `git_diff` to the given repository, making changes directly
 * in the working directory, the index, or both.
 *
 * @param repo the repository to apply to
 * @param diff the diff to apply
 * @param location the location to apply (workdir, index or both)
 * @param options the options for the apply (or null for defaults)
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_apply(
	git_repository *repo,
	git_diff *diff,
	git_apply_location_t location,
	const git_apply_options *options);

/** @} */
GIT_END_DECL
#endif
