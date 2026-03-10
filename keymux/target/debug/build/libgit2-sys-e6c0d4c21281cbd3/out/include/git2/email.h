/*
 * Copyright (C) the libgit2 contributors. All rights reserved.
 *
 * This file is part of libgit2, distributed under the GNU GPL v2 with
 * a Linking Exception. For full terms see the included COPYING file.
 */
#ifndef INCLUDE_git_email_h__
#define INCLUDE_git_email_h__

#include "common.h"

/**
 * @file git2/email.h
 * @brief Git email formatting and application routines.
 * @ingroup Git
 * @{
 */
GIT_BEGIN_DECL

/**
 * Formatting options for diff e-mail generation
 */
typedef enum {
	/** Normal patch, the default */
	GIT_EMAIL_CREATE_DEFAULT = 0,

	/** Do not include patch numbers in the subject prefix. */
	GIT_EMAIL_CREATE_OMIT_NUMBERS = (1u << 0),

	/**
	 * Include numbers in the subject prefix even when the
	 * patch is for a single commit (1/1).
	 */
	GIT_EMAIL_CREATE_ALWAYS_NUMBER = (1u << 1),

	/** Do not perform rename or similarity detection. */
	GIT_EMAIL_CREATE_NO_RENAMES = (1u << 2)
} git_email_create_flags_t;

/**
 * Options for controlling the formatting of the generated e-mail.
 */
typedef struct {
	unsigned int version;

	/** see `git_email_create_flags_t` above */
	uint32_t flags;

	/** Options to use when creating diffs */
	git_diff_options diff_opts;

	/** Options for finding similarities within diffs */
	git_diff_find_options diff_find_opts;

	/**
	 * The subject prefix, by default "PATCH".  If set to an empty
	 * string ("") then only the patch numbers will be shown in the
	 * prefix.  If the subject_prefix is empty and patch numbers
	 * are not being shown, the prefix will be omitted entirely.
	 */
	const char *subject_prefix;

	/**
	 * The starting patch number; this cannot be 0.  By default,
	 * this is 1.
	 */
	size_t start_number;

	/** The "re-roll" number.  By default, there is no re-roll. */
	size_t reroll_number;
} git_email_create_options;

/*
 * By default, our options include rename detection and binary
 * diffs to match `git format-patch`.
 */
#define GIT_EMAIL_CREATE_OPTIONS_VERSION 1
#define GIT_EMAIL_CREATE_OPTIONS_INIT \
{ \
	GIT_EMAIL_CREATE_OPTIONS_VERSION, \
	GIT_EMAIL_CREATE_DEFAULT, \
	{ GIT_DIFF_OPTIONS_VERSION, GIT_DIFF_SHOW_BINARY, GIT_SUBMODULE_IGNORE_UNSPECIFIED, {NULL,0}, NULL, NULL, NULL, 3 }, \
	GIT_DIFF_FIND_OPTIONS_INIT \
}

/**
 * Create a diff for a commit in mbox format for sending via email.
 *
 * @param out buffer to store the e-mail patch in
 * @param diff the changes to include in the email
 * @param patch_idx the patch index
 * @param patch_count the total number of patches that will be included
 * @param commit_id the commit id for this change
 * @param summary the commit message for this change
 * @param body optional text to include above the diffstat
 * @param author the person who authored this commit
 * @param opts email creation options
 */
GIT_EXTERN(int) git_email_create_from_diff(
	git_buf *out,
	git_diff *diff,
	size_t patch_idx,
	size_t patch_count,
	const git_oid *commit_id,
	const char *summary,
	const char *body,
	const git_signature *author,
	const git_email_create_options *opts);

/**
 * Create a diff for a commit in mbox format for sending via email.
 * The commit must not be a merge commit.
 *
 * @param out buffer to store the e-mail patch in
 * @param commit commit to create a patch for
 * @param opts email creation options
 */
GIT_EXTERN(int) git_email_create_from_commit(
	git_buf *out,
	git_commit *commit,
	const git_email_create_options *opts);

GIT_END_DECL

/** @} */

#endif
