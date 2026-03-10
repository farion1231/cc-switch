/*
 * Copyright (C) the libgit2 contributors. All rights reserved.
 *
 * This file is part of libgit2, distributed under the GNU GPL v2 with
 * a Linking Exception. For full terms see the included COPYING file.
 */
#ifndef INCLUDE_sys_git_email_h__
#define INCLUDE_sys_git_email_h__

/**
 * @file git2/sys/email.h
 * @brief Advanced git email creation routines
 * @defgroup git_email Advanced git email creation routines
 * @ingroup Git
 * @{
 */
GIT_BEGIN_DECL

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

/** @} */
GIT_END_DECL
#endif
