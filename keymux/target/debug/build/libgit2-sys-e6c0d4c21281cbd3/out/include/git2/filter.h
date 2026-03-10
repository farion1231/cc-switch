/*
 * Copyright (C) the libgit2 contributors. All rights reserved.
 *
 * This file is part of libgit2, distributed under the GNU GPL v2 with
 * a Linking Exception. For full terms see the included COPYING file.
 */
#ifndef INCLUDE_git_filter_h__
#define INCLUDE_git_filter_h__

#include "common.h"
#include "types.h"
#include "oid.h"
#include "buffer.h"

/**
 * @file git2/filter.h
 * @brief Git filter APIs
 *
 * @ingroup Git
 * @{
 */
GIT_BEGIN_DECL

/**
 * Filters are applied in one of two directions: smudging - which is
 * exporting a file from the Git object database to the working directory,
 * and cleaning - which is importing a file from the working directory to
 * the Git object database.  These values control which direction of
 * change is being applied.
 */
typedef enum {
	GIT_FILTER_TO_WORKTREE = 0,
	GIT_FILTER_SMUDGE = GIT_FILTER_TO_WORKTREE,
	GIT_FILTER_TO_ODB = 1,
	GIT_FILTER_CLEAN = GIT_FILTER_TO_ODB
} git_filter_mode_t;

/**
 * Filter option flags.
 */
typedef enum {
	GIT_FILTER_DEFAULT = 0u,

	/** Don't error for `safecrlf` violations, allow them to continue. */
	GIT_FILTER_ALLOW_UNSAFE = (1u << 0),

	/** Don't load `/etc/gitattributes` (or the system equivalent) */
	GIT_FILTER_NO_SYSTEM_ATTRIBUTES = (1u << 1),

	/** Load attributes from `.gitattributes` in the root of HEAD */
	GIT_FILTER_ATTRIBUTES_FROM_HEAD = (1u << 2),

	/**
	 * Load attributes from `.gitattributes` in a given commit.
	 * This can only be specified in a `git_filter_options`.
	 */
	GIT_FILTER_ATTRIBUTES_FROM_COMMIT = (1u << 3)
} git_filter_flag_t;

/**
 * Filtering options
 */
typedef struct {
	unsigned int version;

	/** See `git_filter_flag_t` above */
	uint32_t flags;

#ifdef GIT_DEPRECATE_HARD
	void *reserved;
#else
	git_oid *commit_id;
#endif

	/**
	 * The commit to load attributes from, when
	 * `GIT_FILTER_ATTRIBUTES_FROM_COMMIT` is specified.
	 */
	git_oid attr_commit_id;
} git_filter_options;

 #define GIT_FILTER_OPTIONS_VERSION 1
 #define GIT_FILTER_OPTIONS_INIT {GIT_FILTER_OPTIONS_VERSION}

/**
 * A filter that can transform file data
 *
 * This represents a filter that can be used to transform or even replace
 * file data.  Libgit2 includes one built in filter and it is possible to
 * write your own (see git2/sys/filter.h for information on that).
 *
 * The two builtin filters are:
 *
 * * "crlf" which uses the complex rules with the "text", "eol", and
 *   "crlf" file attributes to decide how to convert between LF and CRLF
 *   line endings
 * * "ident" which replaces "$Id$" in a blob with "$Id: <blob OID>$" upon
 *   checkout and replaced "$Id: <anything>$" with "$Id$" on checkin.
 */
typedef struct git_filter git_filter;

/**
 * List of filters to be applied
 *
 * This represents a list of filters to be applied to a file / blob.  You
 * can build the list with one call, apply it with another, and dispose it
 * with a third.  In typical usage, there are not many occasions where a
 * git_filter_list is needed directly since the library will generally
 * handle conversions for you, but it can be convenient to be able to
 * build and apply the list sometimes.
 */
typedef struct git_filter_list git_filter_list;

/**
 * Load the filter list for a given path.
 *
 * This will return 0 (success) but set the output git_filter_list to NULL
 * if no filters are requested for the given file.
 *
 * @param filters Output newly created git_filter_list (or NULL)
 * @param repo Repository object that contains `path`
 * @param blob The blob to which the filter will be applied (if known)
 * @param path Relative path of the file to be filtered
 * @param mode Filtering direction (WT->ODB or ODB->WT)
 * @param flags Combination of `git_filter_flag_t` flags
 * @return 0 on success (which could still return NULL if no filters are
 *         needed for the requested file), <0 on error
 */
GIT_EXTERN(int) git_filter_list_load(
	git_filter_list **filters,
	git_repository *repo,
	git_blob *blob, /* can be NULL */
	const char *path,
	git_filter_mode_t mode,
	uint32_t flags);

/**
 * Load the filter list for a given path.
 *
 * This will return 0 (success) but set the output git_filter_list to NULL
 * if no filters are requested for the given file.
 *
 * @param filters Output newly created git_filter_list (or NULL)
 * @param repo Repository object that contains `path`
 * @param blob The blob to which the filter will be applied (if known)
 * @param path Relative path of the file to be filtered
 * @param mode Filtering direction (WT->ODB or ODB->WT)
 * @param opts The `git_filter_options` to use when loading filters
 * @return 0 on success (which could still return NULL if no filters are
 *         needed for the requested file), <0 on error
 */
GIT_EXTERN(int) git_filter_list_load_ext(
	git_filter_list **filters,
	git_repository *repo,
	git_blob *blob,
	const char *path,
	git_filter_mode_t mode,
	git_filter_options *opts);

/**
 * Query the filter list to see if a given filter (by name) will run.
 * The built-in filters "crlf" and "ident" can be queried, otherwise this
 * is the name of the filter specified by the filter attribute.
 *
 * This will return 0 if the given filter is not in the list, or 1 if
 * the filter will be applied.
 *
 * @param filters A loaded git_filter_list (or NULL)
 * @param name The name of the filter to query
 * @return 1 if the filter is in the list, 0 otherwise
 */
GIT_EXTERN(int) git_filter_list_contains(
	git_filter_list *filters,
	const char *name);

/**
 * Apply filter list to a data buffer.
 *
 * @param out Buffer to store the result of the filtering
 * @param filters A loaded git_filter_list (or NULL)
 * @param in Buffer containing the data to filter
 * @param in_len The length of the input buffer
 * @return 0 on success, an error code otherwise
 */
GIT_EXTERN(int) git_filter_list_apply_to_buffer(
	git_buf *out,
	git_filter_list *filters,
	const char *in,
	size_t in_len);

/**
 * Apply a filter list to the contents of a file on disk
 *
 * @param out buffer into which to store the filtered file
 * @param filters the list of filters to apply
 * @param repo the repository in which to perform the filtering
 * @param path the path of the file to filter, a relative path will be
 * taken as relative to the workdir
 * @return 0 or an error code.
 */
GIT_EXTERN(int) git_filter_list_apply_to_file(
	git_buf *out,
	git_filter_list *filters,
	git_repository *repo,
	const char *path);

/**
 * Apply a filter list to the contents of a blob
 *
 * @param out buffer into which to store the filtered file
 * @param filters the list of filters to apply
 * @param blob the blob to filter
 * @return 0 or an error code.
 */
GIT_EXTERN(int) git_filter_list_apply_to_blob(
	git_buf *out,
	git_filter_list *filters,
	git_blob *blob);

/**
 * Apply a filter list to an arbitrary buffer as a stream
 *
 * @param filters the list of filters to apply
 * @param buffer the buffer to filter
 * @param len the size of the buffer
 * @param target the stream into which the data will be written
 * @return 0 or an error code.
 */
GIT_EXTERN(int) git_filter_list_stream_buffer(
	git_filter_list *filters,
	const char *buffer,
	size_t len,
	git_writestream *target);

/**
 * Apply a filter list to a file as a stream
 *
 * @param filters the list of filters to apply
 * @param repo the repository in which to perform the filtering
 * @param path the path of the file to filter, a relative path will be
 * taken as relative to the workdir
 * @param target the stream into which the data will be written
 * @return 0 or an error code.
 */
GIT_EXTERN(int) git_filter_list_stream_file(
	git_filter_list *filters,
	git_repository *repo,
	const char *path,
	git_writestream *target);

/**
 * Apply a filter list to a blob as a stream
 *
 * @param filters the list of filters to apply
 * @param blob the blob to filter
 * @param target the stream into which the data will be written
 * @return 0 or an error code.
 */
GIT_EXTERN(int) git_filter_list_stream_blob(
	git_filter_list *filters,
	git_blob *blob,
	git_writestream *target);

/**
 * Free a git_filter_list
 *
 * @param filters A git_filter_list created by `git_filter_list_load`
 */
GIT_EXTERN(void) git_filter_list_free(git_filter_list *filters);


GIT_END_DECL

/** @} */

#endif
