/*
 * Copyright (C) the libgit2 contributors. All rights reserved.
 *
 * This file is part of libgit2, distributed under the GNU GPL v2 with
 * a Linking Exception. For full terms see the included COPYING file.
 */
#ifndef INCLUDE_git_describe_h__
#define INCLUDE_git_describe_h__

#include "common.h"
#include "types.h"
#include "buffer.h"

/**
 * @file git2/describe.h
 * @brief Git describing routines
 * @defgroup git_describe Git describing routines
 * @ingroup Git
 * @{
 */
GIT_BEGIN_DECL

/**
 * Reference lookup strategy
 *
 * These behave like the --tags and --all options to git-describe,
 * namely they say to look for any reference in either refs/tags/ or
 * refs/ respectively.
 */
typedef enum {
	GIT_DESCRIBE_DEFAULT,
	GIT_DESCRIBE_TAGS,
	GIT_DESCRIBE_ALL
} git_describe_strategy_t;

/**
 * Describe options structure
 *
 * Initialize with `GIT_DESCRIBE_OPTIONS_INIT`. Alternatively, you can
 * use `git_describe_options_init`.
 *
 */
typedef struct git_describe_options {
	unsigned int version;

	unsigned int max_candidates_tags; /**< default: 10 */
	unsigned int describe_strategy; /**< default: GIT_DESCRIBE_DEFAULT */
	const char *pattern;
	/**
	 * When calculating the distance from the matching tag or
	 * reference, only walk down the first-parent ancestry.
	 */
	int only_follow_first_parent;
	/**
	 * If no matching tag or reference is found, the describe
	 * operation would normally fail. If this option is set, it
	 * will instead fall back to showing the full id of the
	 * commit.
	 */
	int show_commit_oid_as_fallback;
} git_describe_options;

#define GIT_DESCRIBE_DEFAULT_MAX_CANDIDATES_TAGS 10
#define GIT_DESCRIBE_DEFAULT_ABBREVIATED_SIZE 7

#define GIT_DESCRIBE_OPTIONS_VERSION 1
#define GIT_DESCRIBE_OPTIONS_INIT { \
	GIT_DESCRIBE_OPTIONS_VERSION, \
	GIT_DESCRIBE_DEFAULT_MAX_CANDIDATES_TAGS, \
}

/**
 * Initialize git_describe_options structure
 *
 * Initializes a `git_describe_options` with default values. Equivalent to creating
 * an instance with GIT_DESCRIBE_OPTIONS_INIT.
 *
 * @param opts The `git_describe_options` struct to initialize.
 * @param version The struct version; pass `GIT_DESCRIBE_OPTIONS_VERSION`.
 * @return Zero on success; -1 on failure.
 */
GIT_EXTERN(int) git_describe_options_init(git_describe_options *opts, unsigned int version);

/**
 * Describe format options structure
 *
 * Initialize with `GIT_DESCRIBE_FORMAT_OPTIONS_INIT`. Alternatively, you can
 * use `git_describe_format_options_init`.
 *
 */
typedef struct {
	unsigned int version;

	/**
	 * Size of the abbreviated commit id to use. This value is the
	 * lower bound for the length of the abbreviated string. The
	 * default is 7.
	 */
	unsigned int abbreviated_size;

	/**
	 * Set to use the long format even when a shorter name could be used.
	 */
	int always_use_long_format;

	/**
	 * If the workdir is dirty and this is set, this string will
	 * be appended to the description string.
	 */
	const char *dirty_suffix;
} git_describe_format_options;

#define GIT_DESCRIBE_FORMAT_OPTIONS_VERSION 1
#define GIT_DESCRIBE_FORMAT_OPTIONS_INIT { \
		GIT_DESCRIBE_FORMAT_OPTIONS_VERSION,   \
		GIT_DESCRIBE_DEFAULT_ABBREVIATED_SIZE, \
 }

/**
 * Initialize git_describe_format_options structure
 *
 * Initializes a `git_describe_format_options` with default values. Equivalent to creating
 * an instance with GIT_DESCRIBE_FORMAT_OPTIONS_INIT.
 *
 * @param opts The `git_describe_format_options` struct to initialize.
 * @param version The struct version; pass `GIT_DESCRIBE_FORMAT_OPTIONS_VERSION`.
 * @return Zero on success; -1 on failure.
 */
GIT_EXTERN(int) git_describe_format_options_init(git_describe_format_options *opts, unsigned int version);

/**
 * A struct that stores the result of a describe operation.
 */
typedef struct git_describe_result git_describe_result;

/**
 * Describe a commit
 *
 * Perform the describe operation on the given committish object.
 *
 * @param result pointer to store the result. You must free this once
 * you're done with it.
 * @param committish a committish to describe
 * @param opts the lookup options (or NULL for defaults)
 * @return 0 or an error code.
 */
GIT_EXTERN(int) git_describe_commit(
	git_describe_result **result,
	git_object *committish,
	git_describe_options *opts);

/**
 * Describe a commit
 *
 * Perform the describe operation on the current commit and the
 * worktree. After performing describe on HEAD, a status is run and the
 * description is considered to be dirty if there are.
 *
 * @param out pointer to store the result. You must free this once
 * you're done with it.
 * @param repo the repository in which to perform the describe
 * @param opts the lookup options (or NULL for defaults)
 * @return 0 or an error code.
 */
GIT_EXTERN(int) git_describe_workdir(
	git_describe_result **out,
	git_repository *repo,
	git_describe_options *opts);

/**
 * Print the describe result to a buffer
 *
 * @param out The buffer to store the result
 * @param result the result from `git_describe_commit()` or
 * `git_describe_workdir()`.
 * @param opts the formatting options (or NULL for defaults)
 * @return 0 or an error code.
 */
GIT_EXTERN(int) git_describe_format(
	git_buf *out,
	const git_describe_result *result,
	const git_describe_format_options *opts);

/**
 * Free the describe result.
 *
 * @param result The result to free.
 */
GIT_EXTERN(void) git_describe_result_free(git_describe_result *result);

/** @} */
GIT_END_DECL

#endif
