/*
 * Copyright (C) the libgit2 contributors. All rights reserved.
 *
 * This file is part of libgit2, distributed under the GNU GPL v2 with
 * a Linking Exception. For full terms see the included COPYING file.
 */
#ifndef INCLUDE_git_pack_h__
#define INCLUDE_git_pack_h__

#include "common.h"
#include "oid.h"
#include "indexer.h"

/**
 * @file git2/pack.h
 * @brief Git pack management routines
 *
 * Packing objects
 * ---------------
 *
 * Creation of packfiles requires two steps:
 *
 * - First, insert all the objects you want to put into the packfile
 *   using `git_packbuilder_insert` and `git_packbuilder_insert_tree`.
 *   It's important to add the objects in recency order ("in the order
 *   that they are 'reachable' from head").
 *
 *   "ANY order will give you a working pack, ... [but it is] the thing
 *   that gives packs good locality. It keeps the objects close to the
 *   head (whether they are old or new, but they are _reachable_ from the
 *   head) at the head of the pack. So packs actually have absolutely
 *   _wonderful_ IO patterns." - Linus Torvalds
 *   git.git/Documentation/technical/pack-heuristics.txt
 *
 * - Second, use `git_packbuilder_write` or `git_packbuilder_foreach` to
 *   write the resulting packfile.
 *
 *   libgit2 will take care of the delta ordering and generation.
 *   `git_packbuilder_set_threads` can be used to adjust the number of
 *   threads used for the process.
 *
 * See tests/pack/packbuilder.c for an example.
 *
 * @ingroup Git
 * @{
 */
GIT_BEGIN_DECL

/**
 * Stages that are reported by the packbuilder progress callback.
 */
typedef enum {
	GIT_PACKBUILDER_ADDING_OBJECTS = 0,
	GIT_PACKBUILDER_DELTAFICATION = 1
} git_packbuilder_stage_t;

/**
 * Initialize a new packbuilder
 *
 * @param out The new packbuilder object
 * @param repo The repository
 *
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_packbuilder_new(git_packbuilder **out, git_repository *repo);

/**
 * Set number of threads to spawn
 *
 * By default, libgit2 won't spawn any threads at all;
 * when set to 0, libgit2 will autodetect the number of
 * CPUs.
 *
 * @param pb The packbuilder
 * @param n Number of threads to spawn
 * @return number of actual threads to be used
 */
GIT_EXTERN(unsigned int) git_packbuilder_set_threads(git_packbuilder *pb, unsigned int n);

/**
 * Insert a single object
 *
 * For an optimal pack it's mandatory to insert objects in recency order,
 * commits followed by trees and blobs.
 *
 * @param pb The packbuilder
 * @param id The oid of the commit
 * @param name The name; might be NULL
 *
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_packbuilder_insert(git_packbuilder *pb, const git_oid *id, const char *name);

/**
 * Insert a root tree object
 *
 * This will add the tree as well as all referenced trees and blobs.
 *
 * @param pb The packbuilder
 * @param id The oid of the root tree
 *
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_packbuilder_insert_tree(git_packbuilder *pb, const git_oid *id);

/**
 * Insert a commit object
 *
 * This will add a commit as well as the completed referenced tree.
 *
 * @param pb The packbuilder
 * @param id The oid of the commit
 *
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_packbuilder_insert_commit(git_packbuilder *pb, const git_oid *id);

/**
 * Insert objects as given by the walk
 *
 * Those commits and all objects they reference will be inserted into
 * the packbuilder.
 *
 * @param pb the packbuilder
 * @param walk the revwalk to use to fill the packbuilder
 *
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_packbuilder_insert_walk(git_packbuilder *pb, git_revwalk *walk);

/**
 * Recursively insert an object and its referenced objects
 *
 * Insert the object as well as any object it references.
 *
 * @param pb the packbuilder
 * @param id the id of the root object to insert
 * @param name optional name for the object
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_packbuilder_insert_recur(git_packbuilder *pb, const git_oid *id, const char *name);

/**
 * Write the contents of the packfile to an in-memory buffer
 *
 * The contents of the buffer will become a valid packfile, even though there
 * will be no attached index
 *
 * @param buf Buffer where to write the packfile
 * @param pb The packbuilder
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_packbuilder_write_buf(git_buf *buf, git_packbuilder *pb);

/**
 * Write the new pack and corresponding index file to path.
 *
 * @param pb The packbuilder
 * @param path Path to the directory where the packfile and index should be stored, or NULL for default location
 * @param mode permissions to use creating a packfile or 0 for defaults
 * @param progress_cb function to call with progress information from the indexer (optional)
 * @param progress_cb_payload payload for the progress callback (optional)
 *
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_packbuilder_write(
	git_packbuilder *pb,
	const char *path,
	unsigned int mode,
	git_indexer_progress_cb progress_cb,
	void *progress_cb_payload);

#ifndef GIT_DEPRECATE_HARD
/**
 * Get the packfile's hash
 *
 * A packfile's name is derived from the sorted hashing of all object
 * names. This is only correct after the packfile has been written.
 *
 * @deprecated use git_packbuilder_name
 * @param pb The packbuilder object
 * @return 0 or an error code
 */
GIT_EXTERN(const git_oid *) git_packbuilder_hash(git_packbuilder *pb);
#endif

/**
 * Get the unique name for the resulting packfile.
 *
 * The packfile's name is derived from the packfile's content.
 * This is only correct after the packfile has been written.
 *
 * @param pb the packbuilder instance
 * @return a NUL terminated string for the packfile name
 */
GIT_EXTERN(const char *) git_packbuilder_name(git_packbuilder *pb);

/**
 * Callback used to iterate over packed objects
 *
 * @see git_packbuilder_foreach
 *
 * @param buf A pointer to the object's data
 * @param size The size of the underlying object
 * @param payload Payload passed to git_packbuilder_foreach
 * @return non-zero to terminate the iteration
 */
typedef int GIT_CALLBACK(git_packbuilder_foreach_cb)(void *buf, size_t size, void *payload);

/**
 * Create the new pack and pass each object to the callback
 *
 * @param pb the packbuilder
 * @param cb the callback to call with each packed object's buffer
 * @param payload the callback's data
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_packbuilder_foreach(git_packbuilder *pb, git_packbuilder_foreach_cb cb, void *payload);

/**
 * Get the total number of objects the packbuilder will write out
 *
 * @param pb the packbuilder
 * @return the number of objects in the packfile
 */
GIT_EXTERN(size_t) git_packbuilder_object_count(git_packbuilder *pb);

/**
 * Get the number of objects the packbuilder has already written out
 *
 * @param pb the packbuilder
 * @return the number of objects which have already been written
 */
GIT_EXTERN(size_t) git_packbuilder_written(git_packbuilder *pb);

/** Packbuilder progress notification function */
typedef int GIT_CALLBACK(git_packbuilder_progress)(
	int stage,
	uint32_t current,
	uint32_t total,
	void *payload);

/**
 * Set the callbacks for a packbuilder
 *
 * @param pb The packbuilder object
 * @param progress_cb Function to call with progress information during
 * pack building. Be aware that this is called inline with pack building
 * operations, so performance may be affected.
 * @param progress_cb_payload Payload for progress callback.
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_packbuilder_set_callbacks(
	git_packbuilder *pb,
	git_packbuilder_progress progress_cb,
	void *progress_cb_payload);

/**
 * Free the packbuilder and all associated data
 *
 * @param pb The packbuilder
 */
GIT_EXTERN(void) git_packbuilder_free(git_packbuilder *pb);

/** @} */
GIT_END_DECL
#endif
