/*
 * Copyright (C) the libgit2 contributors. All rights reserved.
 *
 * This file is part of libgit2, distributed under the GNU GPL v2 with
 * a Linking Exception. For full terms see the included COPYING file.
 */
#ifndef INCLUDE_sys_git_merge_h__
#define INCLUDE_sys_git_merge_h__

#include "git2/common.h"
#include "git2/types.h"
#include "git2/index.h"
#include "git2/merge.h"

/**
 * @file git2/sys/merge.h
 * @brief Git merge driver backend and plugin routines
 * @defgroup git_merge Git merge driver APIs
 * @ingroup Git
 * @{
 */
GIT_BEGIN_DECL

typedef struct git_merge_driver git_merge_driver;

/**
 * Look up a merge driver by name
 *
 * @param name The name of the merge driver
 * @return Pointer to the merge driver object or NULL if not found
 */
GIT_EXTERN(git_merge_driver *) git_merge_driver_lookup(const char *name);

#define GIT_MERGE_DRIVER_TEXT   "text"
#define GIT_MERGE_DRIVER_BINARY "binary"
#define GIT_MERGE_DRIVER_UNION  "union"

/**
 * A merge driver source represents the file to be merged
 */
typedef struct git_merge_driver_source git_merge_driver_source;

/** Get the repository that the source data is coming from. */
GIT_EXTERN(git_repository *) git_merge_driver_source_repo(
	const git_merge_driver_source *src);

/** Gets the ancestor of the file to merge. */
GIT_EXTERN(const git_index_entry *) git_merge_driver_source_ancestor(
	const git_merge_driver_source *src);

/** Gets the ours side of the file to merge. */
GIT_EXTERN(const git_index_entry *) git_merge_driver_source_ours(
	const git_merge_driver_source *src);

/** Gets the theirs side of the file to merge. */
GIT_EXTERN(const git_index_entry *) git_merge_driver_source_theirs(
	const git_merge_driver_source *src);

/** Gets the merge file options that the merge was invoked with */
GIT_EXTERN(const git_merge_file_options *) git_merge_driver_source_file_options(
	const git_merge_driver_source *src);


/**
 * Initialize callback on merge driver
 *
 * Specified as `driver.initialize`, this is an optional callback invoked
 * before a merge driver is first used.  It will be called once at most
 * per library lifetime.
 *
 * If non-NULL, the merge driver's `initialize` callback will be invoked
 * right before the first use of the driver, so you can defer expensive
 * initialization operations (in case libgit2 is being used in a way that
 * doesn't need the merge driver).
 */
typedef int GIT_CALLBACK(git_merge_driver_init_fn)(git_merge_driver *self);

/**
 * Shutdown callback on merge driver
 *
 * Specified as `driver.shutdown`, this is an optional callback invoked
 * when the merge driver is unregistered or when libgit2 is shutting down.
 * It will be called once at most and should release resources as needed.
 * This may be called even if the `initialize` callback was not made.
 *
 * Typically this function will free the `git_merge_driver` object itself.
 */
typedef void GIT_CALLBACK(git_merge_driver_shutdown_fn)(git_merge_driver *self);

/**
 * Callback to perform the merge.
 *
 * Specified as `driver.apply`, this is the callback that actually does the
 * merge.  If it can successfully perform a merge, it should populate
 * `path_out` with a pointer to the filename to accept, `mode_out` with
 * the resultant mode, and `merged_out` with the buffer of the merged file
 * and then return 0.  If the driver returns `GIT_PASSTHROUGH`, then the
 * default merge driver should instead be run.  It can also return
 * `GIT_EMERGECONFLICT` if the driver is not able to produce a merge result,
 * and the file will remain conflicted.  Any other errors will fail and
 * return to the caller.
 *
 * The `filter_name` contains the name of the filter that was invoked, as
 * specified by the file's attributes.
 *
 * The `src` contains the data about the file to be merged.
 */
typedef int GIT_CALLBACK(git_merge_driver_apply_fn)(
	git_merge_driver *self,
	const char **path_out,
	uint32_t *mode_out,
	git_buf *merged_out,
	const char *filter_name,
	const git_merge_driver_source *src);

/**
 * Merge driver structure used to register custom merge drivers.
 *
 * To associate extra data with a driver, allocate extra data and put the
 * `git_merge_driver` struct at the start of your data buffer, then cast
 * the `self` pointer to your larger structure when your callback is invoked.
 */
struct git_merge_driver {
	/** The `version` should be set to `GIT_MERGE_DRIVER_VERSION`. */
	unsigned int                 version;

	/** Called when the merge driver is first used for any file. */
	git_merge_driver_init_fn     initialize;

	/** Called when the merge driver is unregistered from the system. */
	git_merge_driver_shutdown_fn shutdown;

	/**
	 * Called to merge the contents of a conflict.  If this function
	 * returns `GIT_PASSTHROUGH` then the default (`text`) merge driver
	 * will instead be invoked.  If this function returns
	 * `GIT_EMERGECONFLICT` then the file will remain conflicted.
	 */
	git_merge_driver_apply_fn    apply;
};

#define GIT_MERGE_DRIVER_VERSION 1

/**
 * Register a merge driver under a given name.
 *
 * As mentioned elsewhere, the initialize callback will not be invoked
 * immediately.  It is deferred until the driver is used in some way.
 *
 * Currently the merge driver registry is not thread safe, so any
 * registering or deregistering of merge drivers must be done outside of
 * any possible usage of the drivers (i.e. during application setup or
 * shutdown).
 *
 * @param name The name of this driver to match an attribute.  Attempting
 * 			to register with an in-use name will return GIT_EEXISTS.
 * @param driver The merge driver definition.  This pointer will be stored
 *			as is by libgit2 so it must be a durable allocation (either
 *			static or on the heap).
 * @return 0 on successful registry, error code <0 on failure
 */
GIT_EXTERN(int) git_merge_driver_register(
	const char *name, git_merge_driver *driver);

/**
 * Remove the merge driver with the given name.
 *
 * Attempting to remove the builtin libgit2 merge drivers is not permitted
 * and will return an error.
 *
 * Currently the merge driver registry is not thread safe, so any
 * registering or deregistering of drivers must be done outside of any
 * possible usage of the drivers (i.e. during application setup or shutdown).
 *
 * @param name The name under which the merge driver was registered
 * @return 0 on success, error code <0 on failure
 */
GIT_EXTERN(int) git_merge_driver_unregister(const char *name);

/** @} */
GIT_END_DECL
#endif
