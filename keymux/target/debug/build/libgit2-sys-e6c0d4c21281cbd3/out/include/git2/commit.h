/*
 * Copyright (C) the libgit2 contributors. All rights reserved.
 *
 * This file is part of libgit2, distributed under the GNU GPL v2 with
 * a Linking Exception. For full terms see the included COPYING file.
 */
#ifndef INCLUDE_git_commit_h__
#define INCLUDE_git_commit_h__

#include "common.h"
#include "types.h"
#include "oid.h"
#include "object.h"

/**
 * @file git2/commit.h
 * @brief Git commit parsing, formatting routines
 * @defgroup git_commit Git commit parsing, formatting routines
 * @ingroup Git
 * @{
 */
GIT_BEGIN_DECL

/**
 * Lookup a commit object from a repository.
 *
 * The returned object should be released with `git_commit_free` when no
 * longer needed.
 *
 * @param commit pointer to the looked up commit
 * @param repo the repo to use when locating the commit.
 * @param id identity of the commit to locate. If the object is
 *		an annotated tag it will be peeled back to the commit.
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_commit_lookup(
	git_commit **commit, git_repository *repo, const git_oid *id);

/**
 * Lookup a commit object from a repository, given a prefix of its
 * identifier (short id).
 *
 * The returned object should be released with `git_commit_free` when no
 * longer needed.
 *
 * @see git_object_lookup_prefix
 *
 * @param commit pointer to the looked up commit
 * @param repo the repo to use when locating the commit.
 * @param id identity of the commit to locate. If the object is
 *		an annotated tag it will be peeled back to the commit.
 * @param len the length of the short identifier
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_commit_lookup_prefix(
	git_commit **commit, git_repository *repo, const git_oid *id, size_t len);

/**
 * Close an open commit
 *
 * This is a wrapper around git_object_free()
 *
 * IMPORTANT:
 * It *is* necessary to call this method when you stop
 * using a commit. Failure to do so will cause a memory leak.
 *
 * @param commit the commit to close
 */

GIT_EXTERN(void) git_commit_free(git_commit *commit);

/**
 * Get the id of a commit.
 *
 * @param commit a previously loaded commit.
 * @return object identity for the commit.
 */
GIT_EXTERN(const git_oid *) git_commit_id(const git_commit *commit);

/**
 * Get the repository that contains the commit.
 *
 * @param commit A previously loaded commit.
 * @return Repository that contains this commit.
 */
GIT_EXTERN(git_repository *) git_commit_owner(const git_commit *commit);

/**
 * Get the encoding for the message of a commit,
 * as a string representing a standard encoding name.
 *
 * The encoding may be NULL if the `encoding` header
 * in the commit is missing; in that case UTF-8 is assumed.
 *
 * @param commit a previously loaded commit.
 * @return NULL, or the encoding
 */
GIT_EXTERN(const char *) git_commit_message_encoding(const git_commit *commit);

/**
 * Get the full message of a commit.
 *
 * The returned message will be slightly prettified by removing any
 * potential leading newlines.
 *
 * @param commit a previously loaded commit.
 * @return the message of a commit
 */
GIT_EXTERN(const char *) git_commit_message(const git_commit *commit);

/**
 * Get the full raw message of a commit.
 *
 * @param commit a previously loaded commit.
 * @return the raw message of a commit
 */
GIT_EXTERN(const char *) git_commit_message_raw(const git_commit *commit);

/**
 * Get the short "summary" of the git commit message.
 *
 * The returned message is the summary of the commit, comprising the
 * first paragraph of the message with whitespace trimmed and squashed.
 *
 * @param commit a previously loaded commit.
 * @return the summary of a commit or NULL on error
 */
GIT_EXTERN(const char *) git_commit_summary(git_commit *commit);

/**
 * Get the long "body" of the git commit message.
 *
 * The returned message is the body of the commit, comprising
 * everything but the first paragraph of the message. Leading and
 * trailing whitespaces are trimmed.
 *
 * @param commit a previously loaded commit.
 * @return the body of a commit or NULL when no the message only
 *   consists of a summary
 */
GIT_EXTERN(const char *) git_commit_body(git_commit *commit);

/**
 * Get the commit time (i.e. committer time) of a commit.
 *
 * @param commit a previously loaded commit.
 * @return the time of a commit
 */
GIT_EXTERN(git_time_t) git_commit_time(const git_commit *commit);

/**
 * Get the commit timezone offset (i.e. committer's preferred timezone) of a commit.
 *
 * @param commit a previously loaded commit.
 * @return positive or negative timezone offset, in minutes from UTC
 */
GIT_EXTERN(int) git_commit_time_offset(const git_commit *commit);

/**
 * Get the committer of a commit.
 *
 * @param commit a previously loaded commit.
 * @return the committer of a commit
 */
GIT_EXTERN(const git_signature *) git_commit_committer(const git_commit *commit);

/**
 * Get the author of a commit.
 *
 * @param commit a previously loaded commit.
 * @return the author of a commit
 */
GIT_EXTERN(const git_signature *) git_commit_author(const git_commit *commit);

/**
 * Get the committer of a commit, using the mailmap to map names and email
 * addresses to canonical real names and email addresses.
 *
 * Call `git_signature_free` to free the signature.
 *
 * @param out a pointer to store the resolved signature.
 * @param commit a previously loaded commit.
 * @param mailmap the mailmap to resolve with. (may be NULL)
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_commit_committer_with_mailmap(
	git_signature **out, const git_commit *commit, const git_mailmap *mailmap);

/**
 * Get the author of a commit, using the mailmap to map names and email
 * addresses to canonical real names and email addresses.
 *
 * Call `git_signature_free` to free the signature.
 *
 * @param out a pointer to store the resolved signature.
 * @param commit a previously loaded commit.
 * @param mailmap the mailmap to resolve with. (may be NULL)
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_commit_author_with_mailmap(
	git_signature **out, const git_commit *commit, const git_mailmap *mailmap);

/**
 * Get the full raw text of the commit header.
 *
 * @param commit a previously loaded commit
 * @return the header text of the commit
 */
GIT_EXTERN(const char *) git_commit_raw_header(const git_commit *commit);

/**
 * Get the tree pointed to by a commit.
 *
 * @param tree_out pointer where to store the tree object
 * @param commit a previously loaded commit.
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_commit_tree(git_tree **tree_out, const git_commit *commit);

/**
 * Get the id of the tree pointed to by a commit. This differs from
 * `git_commit_tree` in that no attempts are made to fetch an object
 * from the ODB.
 *
 * @param commit a previously loaded commit.
 * @return the id of tree pointed to by commit.
 */
GIT_EXTERN(const git_oid *) git_commit_tree_id(const git_commit *commit);

/**
 * Get the number of parents of this commit
 *
 * @param commit a previously loaded commit.
 * @return integer of count of parents
 */
GIT_EXTERN(unsigned int) git_commit_parentcount(const git_commit *commit);

/**
 * Get the specified parent of the commit.
 *
 * @param out Pointer where to store the parent commit
 * @param commit a previously loaded commit.
 * @param n the position of the parent (from 0 to `parentcount`)
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_commit_parent(
	git_commit **out,
	const git_commit *commit,
	unsigned int n);

/**
 * Get the oid of a specified parent for a commit. This is different from
 * `git_commit_parent`, which will attempt to load the parent commit from
 * the ODB.
 *
 * @param commit a previously loaded commit.
 * @param n the position of the parent (from 0 to `parentcount`)
 * @return the id of the parent, NULL on error.
 */
GIT_EXTERN(const git_oid *) git_commit_parent_id(
	const git_commit *commit,
	unsigned int n);

/**
 * Get the commit object that is the <n>th generation ancestor
 * of the named commit object, following only the first parents.
 * The returned commit has to be freed by the caller.
 *
 * Passing `0` as the generation number returns another instance of the
 * base commit itself.
 *
 * @param ancestor Pointer where to store the ancestor commit
 * @param commit a previously loaded commit.
 * @param n the requested generation
 * @return 0 on success; GIT_ENOTFOUND if no matching ancestor exists
 * or an error code
 */
GIT_EXTERN(int) git_commit_nth_gen_ancestor(
	git_commit **ancestor,
	const git_commit *commit,
	unsigned int n);

/**
 * Get an arbitrary header field
 *
 * @param out the buffer to fill; existing content will be
 * overwritten
 * @param commit the commit to look in
 * @param field the header field to return
 * @return 0 on succeess, GIT_ENOTFOUND if the field does not exist,
 * or an error code
 */
GIT_EXTERN(int) git_commit_header_field(git_buf *out, const git_commit *commit, const char *field);

/**
 * Extract the signature from a commit
 *
 * If the id is not for a commit, the error class will be
 * `GIT_ERROR_INVALID`. If the commit does not have a signature, the
 * error class will be `GIT_ERROR_OBJECT`.
 *
 * @param signature the signature block; existing content will be
 * overwritten
 * @param signed_data signed data; this is the commit contents minus the signature block;
 * existing content will be overwritten
 * @param repo the repository in which the commit exists
 * @param commit_id the commit from which to extract the data
 * @param field the name of the header field containing the signature
 * block; pass `NULL` to extract the default 'gpgsig'
 * @return 0 on success, GIT_ENOTFOUND if the id is not for a commit
 * or the commit does not have a signature.
 */
GIT_EXTERN(int) git_commit_extract_signature(git_buf *signature, git_buf *signed_data, git_repository *repo, git_oid *commit_id, const char *field);

/**
 * Create new commit in the repository from a list of `git_object` pointers
 *
 * The message will **not** be cleaned up automatically. You can do that
 * with the `git_message_prettify()` function.
 *
 * @param id Pointer in which to store the OID of the newly created commit
 *
 * @param repo Repository where to store the commit
 *
 * @param update_ref If not NULL, name of the reference that
 *	will be updated to point to this commit. If the reference
 *	is not direct, it will be resolved to a direct reference.
 *	Use "HEAD" to update the HEAD of the current branch and
 *	make it point to this commit. If the reference doesn't
 *	exist yet, it will be created. If it does exist, the first
 *	parent must be the tip of this branch.
 *
 * @param author Signature with author and author time of commit
 *
 * @param committer Signature with committer and * commit time of commit
 *
 * @param message_encoding The encoding for the message in the
 *  commit, represented with a standard encoding name.
 *  E.g. "UTF-8". If NULL, no encoding header is written and
 *  UTF-8 is assumed.
 *
 * @param message Full message for this commit
 *
 * @param tree An instance of a `git_tree` object that will
 *  be used as the tree for the commit. This tree object must
 *  also be owned by the given `repo`.
 *
 * @param parent_count Number of parents for this commit
 *
 * @param parents Array of `parent_count` pointers to `git_commit`
 *  objects that will be used as the parents for this commit. This
 *  array may be NULL if `parent_count` is 0 (root commit). All the
 *  given commits must be owned by the `repo`.
 *
 * @return 0 or an error code
 *	The created commit will be written to the Object Database and
 *	the given reference will be updated to point to it
 */
GIT_EXTERN(int) git_commit_create(
	git_oid *id,
	git_repository *repo,
	const char *update_ref,
	const git_signature *author,
	const git_signature *committer,
	const char *message_encoding,
	const char *message,
	const git_tree *tree,
	size_t parent_count,
	const git_commit *parents[]);

/**
 * Create new commit in the repository using a variable argument list.
 *
 * The message will **not** be cleaned up automatically. You can do that
 * with the `git_message_prettify()` function.
 *
 * The parents for the commit are specified as a variable list of pointers
 * to `const git_commit *`. Note that this is a convenience method which may
 * not be safe to export for certain languages or compilers
 *
 * All other parameters remain the same as `git_commit_create()`.
 *
 * @see git_commit_create
 */
GIT_EXTERN(int) git_commit_create_v(
	git_oid *id,
	git_repository *repo,
	const char *update_ref,
	const git_signature *author,
	const git_signature *committer,
	const char *message_encoding,
	const char *message,
	const git_tree *tree,
	size_t parent_count,
	...);

/**
 * Amend an existing commit by replacing only non-NULL values.
 *
 * This creates a new commit that is exactly the same as the old commit,
 * except that any non-NULL values will be updated.  The new commit has
 * the same parents as the old commit.
 *
 * The `update_ref` value works as in the regular `git_commit_create()`,
 * updating the ref to point to the newly rewritten commit.  If you want
 * to amend a commit that is not currently the tip of the branch and then
 * rewrite the following commits to reach a ref, pass this as NULL and
 * update the rest of the commit chain and ref separately.
 *
 * Unlike `git_commit_create()`, the `author`, `committer`, `message`,
 * `message_encoding`, and `tree` parameters can be NULL in which case this
 * will use the values from the original `commit_to_amend`.
 *
 * All parameters have the same meanings as in `git_commit_create()`.
 *
 * @see git_commit_create
 */
GIT_EXTERN(int) git_commit_amend(
	git_oid *id,
	const git_commit *commit_to_amend,
	const char *update_ref,
	const git_signature *author,
	const git_signature *committer,
	const char *message_encoding,
	const char *message,
	const git_tree *tree);

/**
 * Create a commit and write it into a buffer
 *
 * Create a commit as with `git_commit_create()` but instead of
 * writing it to the objectdb, write the contents of the object into a
 * buffer.
 *
 * @param out the buffer into which to write the commit object content
 *
 * @param repo Repository where the referenced tree and parents live
 *
 * @param author Signature with author and author time of commit
 *
 * @param committer Signature with committer and * commit time of commit
 *
 * @param message_encoding The encoding for the message in the
 *  commit, represented with a standard encoding name.
 *  E.g. "UTF-8". If NULL, no encoding header is written and
 *  UTF-8 is assumed.
 *
 * @param message Full message for this commit
 *
 * @param tree An instance of a `git_tree` object that will
 *  be used as the tree for the commit. This tree object must
 *  also be owned by the given `repo`.
 *
 * @param parent_count Number of parents for this commit
 *
 * @param parents Array of `parent_count` pointers to `git_commit`
 *  objects that will be used as the parents for this commit. This
 *  array may be NULL if `parent_count` is 0 (root commit). All the
 *  given commits must be owned by the `repo`.
 *
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_commit_create_buffer(
	git_buf *out,
	git_repository *repo,
	const git_signature *author,
	const git_signature *committer,
	const char *message_encoding,
	const char *message,
	const git_tree *tree,
	size_t parent_count,
	const git_commit *parents[]);

/**
 * Create a commit object from the given buffer and signature
 *
 * Given the unsigned commit object's contents, its signature and the
 * header field in which to store the signature, attach the signature
 * to the commit and write it into the given repository.
 *
 * @param out the resulting commit id
 * @param repo the repository to create the commit in.
 * @param commit_content the content of the unsigned commit object
 * @param signature the signature to add to the commit. Leave `NULL`
 * to create a commit without adding a signature field.
 * @param signature_field which header field should contain this
 * signature. Leave `NULL` for the default of "gpgsig"
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_commit_create_with_signature(
	git_oid *out,
	git_repository *repo,
	const char *commit_content,
	const char *signature,
	const char *signature_field);

/**
 * Create an in-memory copy of a commit. The copy must be explicitly
 * free'd or it will leak.
 *
 * @param out Pointer to store the copy of the commit
 * @param source Original commit to copy
 * @return 0
 */
GIT_EXTERN(int) git_commit_dup(git_commit **out, git_commit *source);

/**
 * Commit creation callback: used when a function is going to create
 * commits (for example, in `git_rebase_commit`) to allow callers to
 * override the commit creation behavior.  For example, users may
 * wish to sign commits by providing this information to
 * `git_commit_create_buffer`, signing that buffer, then calling
 * `git_commit_create_with_signature`.  The resultant commit id
 * should be set in the `out` object id parameter.
 *
 * @param out pointer that this callback will populate with the object
 *            id of the commit that is created
 * @param author the author name and time of the commit
 * @param committer the committer name and time of the commit
 * @param message_encoding the encoding of the given message, or NULL
 *                         to assume UTF8
 * @param message the commit message
 * @param tree the tree to be committed
 * @param parent_count the number of parents for this commit
 * @param parents the commit parents
 * @param payload the payload pointer in the rebase options
 * @return 0 if this callback has created the commit and populated the out
 *         parameter, GIT_PASSTHROUGH if the callback has not created a
 *         commit and wants the calling function to create the commit as
 *         if no callback had been specified, any other value to stop
 *         and return a failure
 */
typedef int (*git_commit_create_cb)(
	git_oid *out,
	const git_signature *author,
	const git_signature *committer,
	const char *message_encoding,
	const char *message,
	const git_tree *tree,
	size_t parent_count,
	const git_commit *parents[],
	void *payload);

/** @} */
GIT_END_DECL
#endif
