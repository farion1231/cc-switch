/*
 * Copyright (C) the libgit2 contributors. All rights reserved.
 *
 * This file is part of libgit2, distributed under the GNU GPL v2 with
 * a Linking Exception. For full terms see the included COPYING file.
 */
#ifndef INCLUDE_sys_git_diff_h__
#define INCLUDE_sys_git_diff_h__

#include "git2/common.h"
#include "git2/types.h"
#include "git2/oid.h"
#include "git2/diff.h"
#include "git2/status.h"

/**
 * @file git2/sys/diff.h
 * @brief Low-level Git diff utilities
 * @ingroup Git
 * @{
 */
GIT_BEGIN_DECL

/**
 * Diff print callback that writes to a git_buf.
 *
 * This function is provided not for you to call it directly, but instead
 * so you can use it as a function pointer to the `git_diff_print` or
 * `git_patch_print` APIs.  When using those APIs, you specify a callback
 * to actually handle the diff and/or patch data.
 *
 * Use this callback to easily write that data to a `git_buf` buffer.  You
 * must pass a `git_buf *` value as the payload to the `git_diff_print`
 * and/or `git_patch_print` function.  The data will be appended to the
 * buffer (after any existing content).
 */
GIT_EXTERN(int) git_diff_print_callback__to_buf(
	const git_diff_delta *delta,
	const git_diff_hunk *hunk,
	const git_diff_line *line,
	void *payload); /**< payload must be a `git_buf *` */

/**
 * Diff print callback that writes to stdio FILE handle.
 *
 * This function is provided not for you to call it directly, but instead
 * so you can use it as a function pointer to the `git_diff_print` or
 * `git_patch_print` APIs.  When using those APIs, you specify a callback
 * to actually handle the diff and/or patch data.
 *
 * Use this callback to easily write that data to a stdio FILE handle.  You
 * must pass a `FILE *` value (such as `stdout` or `stderr` or the return
 * value from `fopen()`) as the payload to the `git_diff_print`
 * and/or `git_patch_print` function.  If you pass NULL, this will write
 * data to `stdout`.
 */
GIT_EXTERN(int) git_diff_print_callback__to_file_handle(
	const git_diff_delta *delta,
	const git_diff_hunk *hunk,
	const git_diff_line *line,
	void *payload); /**< payload must be a `FILE *` */


/**
 * Performance data from diffing
 */
typedef struct {
	unsigned int version;
	size_t stat_calls; /**< Number of stat() calls performed */
	size_t oid_calculations; /**< Number of ID calculations */
} git_diff_perfdata;

#define GIT_DIFF_PERFDATA_VERSION 1
#define GIT_DIFF_PERFDATA_INIT {GIT_DIFF_PERFDATA_VERSION,0,0}

/**
 * Get performance data for a diff object.
 *
 * @param out Structure to be filled with diff performance data
 * @param diff Diff to read performance data from
 * @return 0 for success, <0 for error
 */
GIT_EXTERN(int) git_diff_get_perfdata(
	git_diff_perfdata *out, const git_diff *diff);

/**
 * Get performance data for diffs from a git_status_list
 */
GIT_EXTERN(int) git_status_list_get_perfdata(
	git_diff_perfdata *out, const git_status_list *status);

/** @} */
GIT_END_DECL
#endif
