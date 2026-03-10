/*
 * Copyright (C) the libgit2 contributors. All rights reserved.
 *
 * This file is part of libgit2, distributed under the GNU GPL v2 with
 * a Linking Exception. For full terms see the included COPYING file.
 */
#ifndef INCLUDE_sys_git_commit_h__
#define INCLUDE_sys_git_commit_h__

#include "git2/common.h"
#include "git2/types.h"
#include "git2/oid.h"

/**
 * @file git2/sys/commit.h
 * @brief Low-level Git commit creation
 * @defgroup git_backend Git custom backend APIs
 * @ingroup Git
 * @{
 */
GIT_BEGIN_DECL

/**
 * Create new commit in the repository from a list of `git_oid` values.
 *
 * See documentation for `git_commit_create()` for information about the
 * parameters, as the meaning is identical excepting that `tree` and
 * `parents` now take `git_oid`.  This is a dangerous API in that nor
 * the `tree`, neither the `parents` list of `git_oid`s are checked for
 * validity.
 *
 * @see git_commit_create
 */
GIT_EXTERN(int) git_commit_create_from_ids(
	git_oid *id,
	git_repository *repo,
	const char *update_ref,
	const git_signature *author,
	const git_signature *committer,
	const char *message_encoding,
	const char *message,
	const git_oid *tree,
	size_t parent_count,
	const git_oid *parents[]);

/**
 * Callback function to return parents for commit.
 *
 * This is invoked with the count of the number of parents processed so far
 * along with the user supplied payload.  This should return a git_oid of
 * the next parent or NULL if all parents have been provided.
 */
typedef const git_oid * GIT_CALLBACK(git_commit_parent_callback)(size_t idx, void *payload);

/**
 * Create a new commit in the repository with an callback to supply parents.
 *
 * See documentation for `git_commit_create()` for information about the
 * parameters, as the meaning is identical excepting that `tree` takes a
 * `git_oid` and doesn't check for validity, and `parent_cb` is invoked
 * with `parent_payload` and should return `git_oid` values or NULL to
 * indicate that all parents are accounted for.
 *
 * @see git_commit_create
 */
GIT_EXTERN(int) git_commit_create_from_callback(
	git_oid *id,
	git_repository *repo,
	const char *update_ref,
	const git_signature *author,
	const git_signature *committer,
	const char *message_encoding,
	const char *message,
	const git_oid *tree,
	git_commit_parent_callback parent_cb,
	void *parent_payload);

/** @} */
GIT_END_DECL
#endif
