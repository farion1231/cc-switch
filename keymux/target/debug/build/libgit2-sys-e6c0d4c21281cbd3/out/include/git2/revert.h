/*
 * Copyright (C) the libgit2 contributors. All rights reserved.
 *
 * This file is part of libgit2, distributed under the GNU GPL v2 with
 * a Linking Exception. For full terms see the included COPYING file.
 */
#ifndef INCLUDE_git_revert_h__
#define INCLUDE_git_revert_h__

#include "common.h"
#include "types.h"
#include "merge.h"

/**
 * @file git2/revert.h
 * @brief Git revert routines
 * @defgroup git_revert Git revert routines
 * @ingroup Git
 * @{
 */
GIT_BEGIN_DECL

/**
 * Options for revert
 */
typedef struct {
	unsigned int version;

	/** For merge commits, the "mainline" is treated as the parent. */
	unsigned int mainline;

	git_merge_options merge_opts; /**< Options for the merging */
	git_checkout_options checkout_opts; /**< Options for the checkout */
} git_revert_options;

#define GIT_REVERT_OPTIONS_VERSION 1
#define GIT_REVERT_OPTIONS_INIT {GIT_REVERT_OPTIONS_VERSION, 0, GIT_MERGE_OPTIONS_INIT, GIT_CHECKOUT_OPTIONS_INIT}

/**
 * Initialize git_revert_options structure
 *
 * Initializes a `git_revert_options` with default values. Equivalent to
 * creating an instance with `GIT_REVERT_OPTIONS_INIT`.
 *
 * @param opts The `git_revert_options` struct to initialize.
 * @param version The struct version; pass `GIT_REVERT_OPTIONS_VERSION`.
 * @return Zero on success; -1 on failure.
 */
GIT_EXTERN(int) git_revert_options_init(
	git_revert_options *opts,
	unsigned int version);

/**
 * Reverts the given commit against the given "our" commit, producing an
 * index that reflects the result of the revert.
 *
 * The returned index must be freed explicitly with `git_index_free`.
 *
 * @param out pointer to store the index result in
 * @param repo the repository that contains the given commits
 * @param revert_commit the commit to revert
 * @param our_commit the commit to revert against (eg, HEAD)
 * @param mainline the parent of the revert commit, if it is a merge
 * @param merge_options the merge options (or null for defaults)
 * @return zero on success, -1 on failure.
 */
GIT_EXTERN(int) git_revert_commit(
	git_index **out,
	git_repository *repo,
	git_commit *revert_commit,
	git_commit *our_commit,
	unsigned int mainline,
	const git_merge_options *merge_options);

/**
 * Reverts the given commit, producing changes in the index and working directory.
 *
 * @param repo the repository to revert
 * @param commit the commit to revert
 * @param given_opts the revert options (or null for defaults)
 * @return zero on success, -1 on failure.
 */
GIT_EXTERN(int) git_revert(
	git_repository *repo,
	git_commit *commit,
	const git_revert_options *given_opts);

/** @} */
GIT_END_DECL
#endif

