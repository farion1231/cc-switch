/*
 * Copyright (C) the libgit2 contributors. All rights reserved.
 *
 * This file is part of libgit2, distributed under the GNU GPL v2 with
 * a Linking Exception. For full terms see the included COPYING file.
 */
#ifndef INCLUDE_git_cherrypick_h__
#define INCLUDE_git_cherrypick_h__

#include "common.h"
#include "types.h"
#include "merge.h"

/**
 * @file git2/cherrypick.h
 * @brief Git cherry-pick routines
 * @defgroup git_cherrypick Git cherry-pick routines
 * @ingroup Git
 * @{
 */
GIT_BEGIN_DECL

/**
 * Cherry-pick options
 */
typedef struct {
	unsigned int version;

	/** For merge commits, the "mainline" is treated as the parent. */
	unsigned int mainline;

	git_merge_options merge_opts; /**< Options for the merging */
	git_checkout_options checkout_opts; /**< Options for the checkout */
} git_cherrypick_options;

#define GIT_CHERRYPICK_OPTIONS_VERSION 1
#define GIT_CHERRYPICK_OPTIONS_INIT {GIT_CHERRYPICK_OPTIONS_VERSION, 0, GIT_MERGE_OPTIONS_INIT, GIT_CHECKOUT_OPTIONS_INIT}

/**
 * Initialize git_cherrypick_options structure
 *
 * Initializes a `git_cherrypick_options` with default values. Equivalent to creating
 * an instance with GIT_CHERRYPICK_OPTIONS_INIT.
 *
 * @param opts The `git_cherrypick_options` struct to initialize.
 * @param version The struct version; pass `GIT_CHERRYPICK_OPTIONS_VERSION`.
 * @return Zero on success; -1 on failure.
 */
GIT_EXTERN(int) git_cherrypick_options_init(
	git_cherrypick_options *opts,
	unsigned int version);

/**
 * Cherry-picks the given commit against the given "our" commit, producing an
 * index that reflects the result of the cherry-pick.
 *
 * The returned index must be freed explicitly with `git_index_free`.
 *
 * @param out pointer to store the index result in
 * @param repo the repository that contains the given commits
 * @param cherrypick_commit the commit to cherry-pick
 * @param our_commit the commit to cherry-pick against (eg, HEAD)
 * @param mainline the parent of the `cherrypick_commit`, if it is a merge
 * @param merge_options the merge options (or null for defaults)
 * @return zero on success, -1 on failure.
 */
GIT_EXTERN(int) git_cherrypick_commit(
	git_index **out,
	git_repository *repo,
	git_commit *cherrypick_commit,
	git_commit *our_commit,
	unsigned int mainline,
	const git_merge_options *merge_options);

/**
 * Cherry-pick the given commit, producing changes in the index and working directory.
 *
 * @param repo the repository to cherry-pick
 * @param commit the commit to cherry-pick
 * @param cherrypick_options the cherry-pick options (or null for defaults)
 * @return zero on success, -1 on failure.
 */
GIT_EXTERN(int) git_cherrypick(
	git_repository *repo,
	git_commit *commit,
	const git_cherrypick_options *cherrypick_options);

/** @} */
GIT_END_DECL

#endif

