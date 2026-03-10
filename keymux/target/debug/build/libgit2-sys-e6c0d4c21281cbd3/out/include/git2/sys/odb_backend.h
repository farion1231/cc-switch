/*
 * Copyright (C) the libgit2 contributors. All rights reserved.
 *
 * This file is part of libgit2, distributed under the GNU GPL v2 with
 * a Linking Exception. For full terms see the included COPYING file.
 */
#ifndef INCLUDE_sys_git_odb_backend_h__
#define INCLUDE_sys_git_odb_backend_h__

#include "git2/common.h"
#include "git2/types.h"
#include "git2/oid.h"
#include "git2/odb.h"

/**
 * @file git2/sys/backend.h
 * @brief Git custom backend implementors functions
 * @defgroup git_backend Git custom backend APIs
 * @ingroup Git
 * @{
 */
GIT_BEGIN_DECL

/**
 * An instance for a custom backend
 */
struct git_odb_backend {
	unsigned int version;
	git_odb *odb;

	/* read and read_prefix each return to libgit2 a buffer which
	 * will be freed later. The buffer should be allocated using
	 * the function git_odb_backend_data_alloc to ensure that libgit2
	 * can safely free it later. */
	int GIT_CALLBACK(read)(
		void **, size_t *, git_object_t *, git_odb_backend *, const git_oid *);

	/* To find a unique object given a prefix of its oid.  The oid given
	 * must be so that the remaining (GIT_OID_SHA1_HEXSIZE - len)*4 bits are 0s.
	 */
	int GIT_CALLBACK(read_prefix)(
		git_oid *, void **, size_t *, git_object_t *,
		git_odb_backend *, const git_oid *, size_t);

	int GIT_CALLBACK(read_header)(
		size_t *, git_object_t *, git_odb_backend *, const git_oid *);

	/**
	 * Write an object into the backend. The id of the object has
	 * already been calculated and is passed in.
	 */
	int GIT_CALLBACK(write)(
		git_odb_backend *, const git_oid *, const void *, size_t, git_object_t);

	int GIT_CALLBACK(writestream)(
		git_odb_stream **, git_odb_backend *, git_object_size_t, git_object_t);

	int GIT_CALLBACK(readstream)(
		git_odb_stream **, size_t *, git_object_t *,
		git_odb_backend *, const git_oid *);

	int GIT_CALLBACK(exists)(
		git_odb_backend *, const git_oid *);

	int GIT_CALLBACK(exists_prefix)(
		git_oid *, git_odb_backend *, const git_oid *, size_t);

	/**
	 * If the backend implements a refreshing mechanism, it should be exposed
	 * through this endpoint. Each call to `git_odb_refresh()` will invoke it.
	 *
	 * The odb layer will automatically call this when needed on failed
	 * lookups (ie. `exists()`, `read()`, `read_header()`).
	 */
	int GIT_CALLBACK(refresh)(git_odb_backend *);

	int GIT_CALLBACK(foreach)(
		git_odb_backend *, git_odb_foreach_cb cb, void *payload);

	int GIT_CALLBACK(writepack)(
		git_odb_writepack **, git_odb_backend *, git_odb *odb,
		git_indexer_progress_cb progress_cb, void *progress_payload);

	/**
	 * If the backend supports pack files, this will create a
	 * `multi-pack-index` file which will contain an index of all objects
	 * across all the `.pack` files.
	 */
	int GIT_CALLBACK(writemidx)(git_odb_backend *);

	/**
	 * "Freshens" an already existing object, updating its last-used
	 * time.  This occurs when `git_odb_write` was called, but the
	 * object already existed (and will not be re-written).  The
	 * underlying implementation may want to update last-used timestamps.
	 *
	 * If callers implement this, they should return `0` if the object
	 * exists and was freshened, and non-zero otherwise.
	 */
	int GIT_CALLBACK(freshen)(git_odb_backend *, const git_oid *);

	/**
	 * Frees any resources held by the odb (including the `git_odb_backend`
	 * itself). An odb backend implementation must provide this function.
	 */
	void GIT_CALLBACK(free)(git_odb_backend *);
};

#define GIT_ODB_BACKEND_VERSION 1
#define GIT_ODB_BACKEND_INIT {GIT_ODB_BACKEND_VERSION}

/**
 * Initializes a `git_odb_backend` with default values. Equivalent to
 * creating an instance with GIT_ODB_BACKEND_INIT.
 *
 * @param backend the `git_odb_backend` struct to initialize.
 * @param version Version the struct; pass `GIT_ODB_BACKEND_VERSION`
 * @return Zero on success; -1 on failure.
 */
GIT_EXTERN(int) git_odb_init_backend(
	git_odb_backend *backend,
	unsigned int version);

/**
 * Allocate data for an ODB object.  Custom ODB backends may use this
 * to provide data back to the ODB from their read function.  This
 * memory should not be freed once it is returned to libgit2.  If a
 * custom ODB uses this function but encounters an error and does not
 * return this data to libgit2, then they should use the corresponding
 * git_odb_backend_data_free function.
 *
 * @param backend the ODB backend that is allocating this memory
 * @param len the number of bytes to allocate
 * @return the allocated buffer on success or NULL if out of memory
 */
GIT_EXTERN(void *) git_odb_backend_data_alloc(git_odb_backend *backend, size_t len);

/**
 * Frees custom allocated ODB data.  This should only be called when
 * memory allocated using git_odb_backend_data_alloc is not returned
 * to libgit2 because the backend encountered an error in the read
 * function after allocation and did not return this data to libgit2.
 *
 * @param backend the ODB backend that is freeing this memory
 * @param data the buffer to free
 */
GIT_EXTERN(void) git_odb_backend_data_free(git_odb_backend *backend, void *data);


/*
 * Users can avoid deprecated functions by defining `GIT_DEPRECATE_HARD`.
 */
#ifndef GIT_DEPRECATE_HARD

/**
 * Allocate memory for an ODB object from a custom backend.  This is
 * an alias of `git_odb_backend_data_alloc` and is preserved for
 * backward compatibility.
 *
 * This function is deprecated, but there is no plan to remove this
 * function at this time.
 *
 * @deprecated git_odb_backend_data_alloc
 * @see git_odb_backend_data_alloc
 */
GIT_EXTERN(void *) git_odb_backend_malloc(git_odb_backend *backend, size_t len);

#endif

GIT_END_DECL

#endif
