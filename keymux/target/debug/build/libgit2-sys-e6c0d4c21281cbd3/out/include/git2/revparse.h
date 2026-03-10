/*
 * Copyright (C) the libgit2 contributors. All rights reserved.
 *
 * This file is part of libgit2, distributed under the GNU GPL v2 with
 * a Linking Exception. For full terms see the included COPYING file.
 */
#ifndef INCLUDE_git_revparse_h__
#define INCLUDE_git_revparse_h__

#include "common.h"
#include "types.h"

/**
 * @file git2/revparse.h
 * @brief Git revision parsing routines
 * @defgroup git_revparse Git revision parsing routines
 * @ingroup Git
 * @{
 */
GIT_BEGIN_DECL

/**
 * Find a single object, as specified by a revision string.
 *
 * See `man gitrevisions`, or
 * http://git-scm.com/docs/git-rev-parse.html#_specifying_revisions for
 * information on the syntax accepted.
 *
 * The returned object should be released with `git_object_free` when no
 * longer needed.
 *
 * @param out pointer to output object
 * @param repo the repository to search in
 * @param spec the textual specification for an object
 * @return 0 on success, GIT_ENOTFOUND, GIT_EAMBIGUOUS, GIT_EINVALIDSPEC or an error code
 */
GIT_EXTERN(int) git_revparse_single(
	git_object **out, git_repository *repo, const char *spec);

/**
 * Find a single object and intermediate reference by a revision string.
 *
 * See `man gitrevisions`, or
 * http://git-scm.com/docs/git-rev-parse.html#_specifying_revisions for
 * information on the syntax accepted.
 *
 * In some cases (`@{<-n>}` or `<branchname>@{upstream}`), the expression may
 * point to an intermediate reference. When such expressions are being passed
 * in, `reference_out` will be valued as well.
 *
 * The returned object should be released with `git_object_free` and the
 * returned reference with `git_reference_free` when no longer needed.
 *
 * @param object_out pointer to output object
 * @param reference_out pointer to output reference or NULL
 * @param repo the repository to search in
 * @param spec the textual specification for an object
 * @return 0 on success, GIT_ENOTFOUND, GIT_EAMBIGUOUS, GIT_EINVALIDSPEC
 * or an error code
 */
GIT_EXTERN(int) git_revparse_ext(
	git_object **object_out,
	git_reference **reference_out,
	git_repository *repo,
	const char *spec);

/**
 * Revparse flags.  These indicate the intended behavior of the spec passed to
 * git_revparse.
 */
typedef enum {
	/** The spec targeted a single object. */
	GIT_REVSPEC_SINGLE         = 1 << 0,
	/** The spec targeted a range of commits. */
	GIT_REVSPEC_RANGE          = 1 << 1,
	/** The spec used the '...' operator, which invokes special semantics. */
	GIT_REVSPEC_MERGE_BASE     = 1 << 2
} git_revspec_t;

/**
 * Git Revision Spec: output of a `git_revparse` operation
 */
typedef struct {
	/** The left element of the revspec; must be freed by the user */
	git_object *from;
	/** The right element of the revspec; must be freed by the user */
	git_object *to;
	/** The intent of the revspec (i.e. `git_revspec_mode_t` flags) */
	unsigned int flags;
} git_revspec;

/**
 * Parse a revision string for `from`, `to`, and intent.
 *
 * See `man gitrevisions` or
 * http://git-scm.com/docs/git-rev-parse.html#_specifying_revisions for
 * information on the syntax accepted.
 *
 * @param revspec Pointer to an user-allocated git_revspec struct where
 *	              the result of the rev-parse will be stored
 * @param repo the repository to search in
 * @param spec the rev-parse spec to parse
 * @return 0 on success, GIT_INVALIDSPEC, GIT_ENOTFOUND, GIT_EAMBIGUOUS or an error code
 */
GIT_EXTERN(int) git_revparse(
	git_revspec *revspec,
	git_repository *repo,
	const char *spec);


/** @} */
GIT_END_DECL
#endif
