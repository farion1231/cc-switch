/*
 * Copyright (C) the libgit2 contributors. All rights reserved.
 *
 * This file is part of libgit2, distributed under the GNU GPL v2 with
 * a Linking Exception. For full terms see the included COPYING file.
 */
#ifndef INCLUDE_git_tag_h__
#define INCLUDE_git_tag_h__

#include "common.h"
#include "types.h"
#include "oid.h"
#include "object.h"
#include "strarray.h"

/**
 * @file git2/tag.h
 * @brief Git tag parsing routines
 * @defgroup git_tag Git tag management
 * @ingroup Git
 * @{
 */
GIT_BEGIN_DECL

/**
 * Lookup a tag object from the repository.
 *
 * @param out pointer to the looked up tag
 * @param repo the repo to use when locating the tag.
 * @param id identity of the tag to locate.
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_tag_lookup(
	git_tag **out, git_repository *repo, const git_oid *id);

/**
 * Lookup a tag object from the repository,
 * given a prefix of its identifier (short id).
 *
 * @see git_object_lookup_prefix
 *
 * @param out pointer to the looked up tag
 * @param repo the repo to use when locating the tag.
 * @param id identity of the tag to locate.
 * @param len the length of the short identifier
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_tag_lookup_prefix(
	git_tag **out, git_repository *repo, const git_oid *id, size_t len);

/**
 * Close an open tag
 *
 * You can no longer use the git_tag pointer after this call.
 *
 * IMPORTANT: You MUST call this method when you are through with a tag to
 * release memory. Failure to do so will cause a memory leak.
 *
 * @param tag the tag to close
 */
GIT_EXTERN(void) git_tag_free(git_tag *tag);

/**
 * Get the id of a tag.
 *
 * @param tag a previously loaded tag.
 * @return object identity for the tag.
 */
GIT_EXTERN(const git_oid *) git_tag_id(const git_tag *tag);

/**
 * Get the repository that contains the tag.
 *
 * @param tag A previously loaded tag.
 * @return Repository that contains this tag.
 */
GIT_EXTERN(git_repository *) git_tag_owner(const git_tag *tag);

/**
 * Get the tagged object of a tag
 *
 * This method performs a repository lookup for the
 * given object and returns it
 *
 * @param target_out pointer where to store the target
 * @param tag a previously loaded tag.
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_tag_target(git_object **target_out, const git_tag *tag);

/**
 * Get the OID of the tagged object of a tag
 *
 * @param tag a previously loaded tag.
 * @return pointer to the OID
 */
GIT_EXTERN(const git_oid *) git_tag_target_id(const git_tag *tag);

/**
 * Get the type of a tag's tagged object
 *
 * @param tag a previously loaded tag.
 * @return type of the tagged object
 */
GIT_EXTERN(git_object_t) git_tag_target_type(const git_tag *tag);

/**
 * Get the name of a tag
 *
 * @param tag a previously loaded tag.
 * @return name of the tag
 */
GIT_EXTERN(const char *) git_tag_name(const git_tag *tag);

/**
 * Get the tagger (author) of a tag
 *
 * @param tag a previously loaded tag.
 * @return reference to the tag's author or NULL when unspecified
 */
GIT_EXTERN(const git_signature *) git_tag_tagger(const git_tag *tag);

/**
 * Get the message of a tag
 *
 * @param tag a previously loaded tag.
 * @return message of the tag or NULL when unspecified
 */
GIT_EXTERN(const char *) git_tag_message(const git_tag *tag);


/**
 * Create a new tag in the repository from an object
 *
 * A new reference will also be created pointing to
 * this tag object. If `force` is true and a reference
 * already exists with the given name, it'll be replaced.
 *
 * The message will not be cleaned up. This can be achieved
 * through `git_message_prettify()`.
 *
 * The tag name will be checked for validity. You must avoid
 * the characters '~', '^', ':', '\\', '?', '[', and '*', and the
 * sequences ".." and "@{" which have special meaning to revparse.
 *
 * @param oid Pointer where to store the OID of the
 * newly created tag. If the tag already exists, this parameter
 * will be the oid of the existing tag, and the function will
 * return a GIT_EEXISTS error code.
 *
 * @param repo Repository where to store the tag
 *
 * @param tag_name Name for the tag; this name is validated
 * for consistency. It should also not conflict with an
 * already existing tag name
 *
 * @param target Object to which this tag points. This object
 * must belong to the given `repo`.
 *
 * @param tagger Signature of the tagger for this tag, and
 * of the tagging time
 *
 * @param message Full message for this tag
 *
 * @param force Overwrite existing references
 *
 * @return 0 on success, GIT_EINVALIDSPEC or an error code
 *	A tag object is written to the ODB, and a proper reference
 *	is written in the /refs/tags folder, pointing to it
 */
GIT_EXTERN(int) git_tag_create(
	git_oid *oid,
	git_repository *repo,
	const char *tag_name,
	const git_object *target,
	const git_signature *tagger,
	const char *message,
	int force);

/**
 * Create a new tag in the object database pointing to a git_object
 *
 * The message will not be cleaned up. This can be achieved
 * through `git_message_prettify()`.
 *
 * @param oid Pointer where to store the OID of the
 * newly created tag
 *
 * @param repo Repository where to store the tag
 *
 * @param tag_name Name for the tag
 *
 * @param target Object to which this tag points. This object
 * must belong to the given `repo`.
 *
 * @param tagger Signature of the tagger for this tag, and
 * of the tagging time
 *
 * @param message Full message for this tag
 *
 * @return 0 on success or an error code
 */
GIT_EXTERN(int) git_tag_annotation_create(
	git_oid *oid,
	git_repository *repo,
	const char *tag_name,
	const git_object *target,
	const git_signature *tagger,
	const char *message);

/**
 * Create a new tag in the repository from a buffer
 *
 * @param oid Pointer where to store the OID of the newly created tag
 * @param repo Repository where to store the tag
 * @param buffer Raw tag data
 * @param force Overwrite existing tags
 * @return 0 on success; error code otherwise
 */
GIT_EXTERN(int) git_tag_create_from_buffer(
	git_oid *oid,
	git_repository *repo,
	const char *buffer,
	int force);

/**
 * Create a new lightweight tag pointing at a target object
 *
 * A new direct reference will be created pointing to
 * this target object. If `force` is true and a reference
 * already exists with the given name, it'll be replaced.
 *
 * The tag name will be checked for validity.
 * See `git_tag_create()` for rules about valid names.
 *
 * @param oid Pointer where to store the OID of the provided
 * target object. If the tag already exists, this parameter
 * will be filled with the oid of the existing pointed object
 * and the function will return a GIT_EEXISTS error code.
 *
 * @param repo Repository where to store the lightweight tag
 *
 * @param tag_name Name for the tag; this name is validated
 * for consistency. It should also not conflict with an
 * already existing tag name
 *
 * @param target Object to which this tag points. This object
 * must belong to the given `repo`.
 *
 * @param force Overwrite existing references
 *
 * @return 0 on success, GIT_EINVALIDSPEC or an error code
 *	A proper reference is written in the /refs/tags folder,
 * pointing to the provided target object
 */
GIT_EXTERN(int) git_tag_create_lightweight(
	git_oid *oid,
	git_repository *repo,
	const char *tag_name,
	const git_object *target,
	int force);

/**
 * Delete an existing tag reference.
 *
 * The tag name will be checked for validity.
 * See `git_tag_create()` for rules about valid names.
 *
 * @param repo Repository where lives the tag
 *
 * @param tag_name Name of the tag to be deleted;
 * this name is validated for consistency.
 *
 * @return 0 on success, GIT_EINVALIDSPEC or an error code
 */
GIT_EXTERN(int) git_tag_delete(
	git_repository *repo,
	const char *tag_name);

/**
 * Fill a list with all the tags in the Repository
 *
 * The string array will be filled with the names of the
 * matching tags; these values are owned by the user and
 * should be free'd manually when no longer needed, using
 * `git_strarray_free`.
 *
 * @param tag_names Pointer to a git_strarray structure where
 *		the tag names will be stored
 * @param repo Repository where to find the tags
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_tag_list(
	git_strarray *tag_names,
	git_repository *repo);

/**
 * Fill a list with all the tags in the Repository
 * which name match a defined pattern
 *
 * If an empty pattern is provided, all the tags
 * will be returned.
 *
 * The string array will be filled with the names of the
 * matching tags; these values are owned by the user and
 * should be free'd manually when no longer needed, using
 * `git_strarray_free`.
 *
 * @param tag_names Pointer to a git_strarray structure where
 *		the tag names will be stored
 * @param pattern Standard fnmatch pattern
 * @param repo Repository where to find the tags
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_tag_list_match(
	git_strarray *tag_names,
	const char *pattern,
	git_repository *repo);

/**
 * Callback used to iterate over tag names
 *
 * @see git_tag_foreach
 *
 * @param name The tag name
 * @param oid The tag's OID
 * @param payload Payload passed to git_tag_foreach
 * @return non-zero to terminate the iteration
 */
typedef int GIT_CALLBACK(git_tag_foreach_cb)(const char *name, git_oid *oid, void *payload);

/**
 * Call callback `cb' for each tag in the repository
 *
 * @param repo Repository
 * @param callback Callback function
 * @param payload Pointer to callback data (optional)
 */
GIT_EXTERN(int) git_tag_foreach(
	git_repository *repo,
	git_tag_foreach_cb callback,
	void *payload);


/**
 * Recursively peel a tag until a non tag git_object is found
 *
 * The retrieved `tag_target` object is owned by the repository
 * and should be closed with the `git_object_free` method.
 *
 * @param tag_target_out Pointer to the peeled git_object
 * @param tag The tag to be processed
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_tag_peel(
	git_object **tag_target_out,
	const git_tag *tag);

/**
 * Create an in-memory copy of a tag. The copy must be explicitly
 * free'd or it will leak.
 *
 * @param out Pointer to store the copy of the tag
 * @param source Original tag to copy
 * @return 0
 */
GIT_EXTERN(int) git_tag_dup(git_tag **out, git_tag *source);

/**
 * Determine whether a tag name is valid, meaning that (when prefixed
 * with `refs/tags/`) that it is a valid reference name, and that any
 * additional tag name restrictions are imposed (eg, it cannot start
 * with a `-`).
 *
 * @param valid output pointer to set with validity of given tag name
 * @param name a tag name to test
 * @return 0 on success or an error code
 */
GIT_EXTERN(int) git_tag_name_is_valid(int *valid, const char *name);

/** @} */
GIT_END_DECL
#endif
