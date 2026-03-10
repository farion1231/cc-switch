/*
 * Copyright (C) the libgit2 contributors. All rights reserved.
 *
 * This file is part of libgit2, distributed under the GNU GPL v2 with
 * a Linking Exception. For full terms see the included COPYING file.
 */
#ifndef INCLUDE_git_ignore_h__
#define INCLUDE_git_ignore_h__

#include "common.h"
#include "types.h"

GIT_BEGIN_DECL

/**
 * Add ignore rules for a repository.
 *
 * Excludesfile rules (i.e. .gitignore rules) are generally read from
 * .gitignore files in the repository tree or from a shared system file
 * only if a "core.excludesfile" config value is set.  The library also
 * keeps a set of per-repository internal ignores that can be configured
 * in-memory and will not persist.  This function allows you to add to
 * that internal rules list.
 *
 * Example usage:
 *
 *     error = git_ignore_add_rule(myrepo, "*.c\ndir/\nFile with space\n");
 *
 * This would add three rules to the ignores.
 *
 * @param repo The repository to add ignore rules to.
 * @param rules Text of rules, the contents to add on a .gitignore file.
 *              It is okay to have multiple rules in the text; if so,
 *              each rule should be terminated with a newline.
 * @return 0 on success
 */
GIT_EXTERN(int) git_ignore_add_rule(
	git_repository *repo,
	const char *rules);

/**
 * Clear ignore rules that were explicitly added.
 *
 * Resets to the default internal ignore rules.  This will not turn off
 * rules in .gitignore files that actually exist in the filesystem.
 *
 * The default internal ignores ignore ".", ".." and ".git" entries.
 *
 * @param repo The repository to remove ignore rules from.
 * @return 0 on success
 */
GIT_EXTERN(int) git_ignore_clear_internal_rules(
	git_repository *repo);

/**
 * Test if the ignore rules apply to a given path.
 *
 * This function checks the ignore rules to see if they would apply to the
 * given file.  This indicates if the file would be ignored regardless of
 * whether the file is already in the index or committed to the repository.
 *
 * One way to think of this is if you were to do "git check-ignore --no-index"
 * on the given file, would it be shown or not?
 *
 * @param ignored boolean returning 0 if the file is not ignored, 1 if it is
 * @param repo a repository object
 * @param path the file to check ignores for, relative to the repo's workdir.
 * @return 0 if ignore rules could be processed for the file (regardless
 *         of whether it exists or not), or an error < 0 if they could not.
 */
GIT_EXTERN(int) git_ignore_path_is_ignored(
	int *ignored,
	git_repository *repo,
	const char *path);

GIT_END_DECL

#endif
