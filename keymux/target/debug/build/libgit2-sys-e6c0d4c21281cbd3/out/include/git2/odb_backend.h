/*
 * Copyright (C) the libgit2 contributors. All rights reserved.
 *
 * This file is part of libgit2, distributed under the GNU GPL v2 with
 * a Linking Exception. For full terms see the included COPYING file.
 */
#ifndef INCLUDE_git_odb_backend_h__
#define INCLUDE_git_odb_backend_h__

#include "common.h"
#include "types.h"
#include "indexer.h"

/**
 * @file git2/backend.h
 * @brief Git custom backend functions
 * @defgroup git_odb Git object database routines
 * @ingroup Git
 * @{
 */
GIT_BEGIN_DECL

/*
 * Constructors for in-box ODB backends.
 */

/** Options for configuring a packfile object backend. */
typedef struct {
	unsigned int version; /**< version for the struct */

	/**
	 * Type of object IDs to use for this object database, or
	 * 0 for default (currently SHA1).
	 */
	git_oid_t oid_type;
} git_odb_backend_pack_options;

/* The current version of the diff options structure */
#define GIT_ODB_BACKEND_PACK_OPTIONS_VERSION 1

/* Stack initializer for odb pack backend options.  Alternatively use
 * `git_odb_backend_pack_options_init` programmatic initialization.
 */
#define GIT_ODB_BACKEND_PACK_OPTIONS_INIT \
	{ GIT_ODB_BACKEND_PACK_OPTIONS_VERSION }

/**
 * Create a backend for the packfiles.
 *
 * @param out location to store the odb backend pointer
 * @param objects_dir the Git repository's objects directory
 *
 * @return 0 or an error code
 */
#ifdef GIT_EXPERIMENTAL_SHA256
GIT_EXTERN(int) git_odb_backend_pack(
	git_odb_backend **out,
	const char *objects_dir,
	const git_odb_backend_pack_options *opts);
#else
GIT_EXTERN(int) git_odb_backend_pack(
	git_odb_backend **out,
	const char *objects_dir);
#endif

/**
 * Create a backend out of a single packfile
 *
 * This can be useful for inspecting the contents of a single
 * packfile.
 *
 * @param out location to store the odb backend pointer
 * @param index_file path to the packfile's .idx file
 *
 * @return 0 or an error code
 */
#ifdef GIT_EXPERIMENTAL_SHA256
GIT_EXTERN(int) git_odb_backend_one_pack(
	git_odb_backend **out,
	const char *index_file,
	const git_odb_backend_pack_options *opts);
#else
GIT_EXTERN(int) git_odb_backend_one_pack(
	git_odb_backend **out,
	const char *index_file);
#endif

typedef enum {
	GIT_ODB_BACKEND_LOOSE_FSYNC = (1 << 0)
} git_odb_backend_loose_flag_t;

/** Options for configuring a loose object backend. */
typedef struct {
	unsigned int version; /**< version for the struct */

	/** A combination of the `git_odb_backend_loose_flag_t` types. */
	uint32_t flags;

	/**
	 * zlib compression level to use (0-9), where 1 is the fastest
	 * at the expense of larger files, and 9 produces the best
	 * compression at the expense of speed.  0 indicates that no
	 * compression should be performed.  -1 is the default (currently
	 * optimizing for speed).
	 */
	int compression_level;

	/** Permissions to use creating a directory or 0 for defaults */
	unsigned int dir_mode;

	/** Permissions to use creating a file or 0 for defaults */
	unsigned int file_mode;

	/**
	 * Type of object IDs to use for this object database, or
	 * 0 for default (currently SHA1).
	 */
	git_oid_t oid_type;
} git_odb_backend_loose_options;

/* The current version of the diff options structure */
#define GIT_ODB_BACKEND_LOOSE_OPTIONS_VERSION 1

/* Stack initializer for odb loose backend options.  Alternatively use
 * `git_odb_backend_loose_options_init` programmatic initialization.
 */
#define GIT_ODB_BACKEND_LOOSE_OPTIONS_INIT \
	{ GIT_ODB_BACKEND_LOOSE_OPTIONS_VERSION, 0, -1 }

/**
 * Create a backend for loose objects
 *
 * @param out location to store the odb backend pointer
 * @param objects_dir the Git repository's objects directory
 * @param opts options for the loose object backend or NULL
 *
 * @return 0 or an error code
 */
#ifdef GIT_EXPERIMENTAL_SHA256
GIT_EXTERN(int) git_odb_backend_loose(
	git_odb_backend **out,
	const char *objects_dir,
	git_odb_backend_loose_options *opts);
#else
GIT_EXTERN(int) git_odb_backend_loose(
	git_odb_backend **out,
	const char *objects_dir,
	int compression_level,
	int do_fsync,
	unsigned int dir_mode,
	unsigned int file_mode);
#endif

/** Streaming mode */
typedef enum {
	GIT_STREAM_RDONLY = (1 << 1),
	GIT_STREAM_WRONLY = (1 << 2),
	GIT_STREAM_RW = (GIT_STREAM_RDONLY | GIT_STREAM_WRONLY)
} git_odb_stream_t;

/**
 * A stream to read/write from a backend.
 *
 * This represents a stream of data being written to or read from a
 * backend. When writing, the frontend functions take care of
 * calculating the object's id and all `finalize_write` needs to do is
 * store the object with the id it is passed.
 */
struct git_odb_stream {
	git_odb_backend *backend;
	unsigned int mode;
	void *hash_ctx;

#ifdef GIT_EXPERIMENTAL_SHA256
	git_oid_t oid_type;
#endif

	git_object_size_t declared_size;
	git_object_size_t received_bytes;

	/**
	 * Write at most `len` bytes into `buffer` and advance the stream.
	 */
	int GIT_CALLBACK(read)(git_odb_stream *stream, char *buffer, size_t len);

	/**
	 * Write `len` bytes from `buffer` into the stream.
	 */
	int GIT_CALLBACK(write)(git_odb_stream *stream, const char *buffer, size_t len);

	/**
	 * Store the contents of the stream as an object with the id
	 * specified in `oid`.
	 *
	 * This method might not be invoked if:
	 * - an error occurs earlier with the `write` callback,
	 * - the object referred to by `oid` already exists in any backend, or
	 * - the final number of received bytes differs from the size declared
	 *   with `git_odb_open_wstream()`
	 */
	int GIT_CALLBACK(finalize_write)(git_odb_stream *stream, const git_oid *oid);

	/**
	 * Free the stream's memory.
	 *
	 * This method might be called without a call to `finalize_write` if
	 * an error occurs or if the object is already present in the ODB.
	 */
	void GIT_CALLBACK(free)(git_odb_stream *stream);
};

/** A stream to write a pack file to the ODB */
struct git_odb_writepack {
	git_odb_backend *backend;

	int GIT_CALLBACK(append)(git_odb_writepack *writepack, const void *data, size_t size, git_indexer_progress *stats);
	int GIT_CALLBACK(commit)(git_odb_writepack *writepack, git_indexer_progress *stats);
	void GIT_CALLBACK(free)(git_odb_writepack *writepack);
};

GIT_END_DECL

#endif
