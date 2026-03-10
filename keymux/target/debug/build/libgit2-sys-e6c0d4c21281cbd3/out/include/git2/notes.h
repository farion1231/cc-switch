/*
 * Copyright (C) the libgit2 contributors. All rights reserved.
 *
 * This file is part of libgit2, distributed under the GNU GPL v2 with
 * a Linking Exception. For full terms see the included COPYING file.
 */
#ifndef INCLUDE_git_note_h__
#define INCLUDE_git_note_h__

#include "oid.h"

/**
 * @file git2/notes.h
 * @brief Git notes management routines
 * @defgroup git_note Git notes management routines
 * @ingroup Git
 * @{
 */
GIT_BEGIN_DECL

/**
 * Callback for git_note_foreach.
 *
 * Receives:
 * - blob_id: Oid of the blob containing the message
 * - annotated_object_id: Oid of the git object being annotated
 * - payload: Payload data passed to `git_note_foreach`
 */
typedef int GIT_CALLBACK(git_note_foreach_cb)(
	const git_oid *blob_id, const git_oid *annotated_object_id, void *payload);

/**
 * note iterator
 */
typedef struct git_iterator git_note_iterator;

/**
 * Creates a new iterator for notes
 *
 * The iterator must be freed manually by the user.
 *
 * @param out pointer to the iterator
 * @param repo repository where to look up the note
 * @param notes_ref canonical name of the reference to use (optional); defaults to
 *                  "refs/notes/commits"
 *
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_note_iterator_new(
	git_note_iterator **out,
	git_repository *repo,
	const char *notes_ref);

/**
 * Creates a new iterator for notes from a commit
 *
 * The iterator must be freed manually by the user.
 *
 * @param out pointer to the iterator
 * @param notes_commit a pointer to the notes commit object
 *
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_note_commit_iterator_new(
	git_note_iterator **out,
	git_commit *notes_commit);

/**
 * Frees an git_note_iterator
 *
 * @param it pointer to the iterator
 */
GIT_EXTERN(void) git_note_iterator_free(git_note_iterator *it);

/**
 * Return the current item (note_id and annotated_id) and advance the iterator
 * internally to the next value
 *
 * @param note_id id of blob containing the message
 * @param annotated_id id of the git object being annotated
 * @param it pointer to the iterator
 *
 * @return 0 (no error), GIT_ITEROVER (iteration is done) or an error code
 *         (negative value)
 */
GIT_EXTERN(int) git_note_next(
	git_oid *note_id,
	git_oid *annotated_id,
	git_note_iterator *it);


/**
 * Read the note for an object
 *
 * The note must be freed manually by the user.
 *
 * @param out pointer to the read note; NULL in case of error
 * @param repo repository where to look up the note
 * @param notes_ref canonical name of the reference to use (optional); defaults to
 *                  "refs/notes/commits"
 * @param oid OID of the git object to read the note from
 *
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_note_read(
	git_note **out,
	git_repository *repo,
	const char *notes_ref,
	const git_oid *oid);


/**
 * Read the note for an object from a note commit
 *
 * The note must be freed manually by the user.
 *
 * @param out pointer to the read note; NULL in case of error
 * @param repo repository where to look up the note
 * @param notes_commit a pointer to the notes commit object
 * @param oid OID of the git object to read the note from
 *
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_note_commit_read(
	git_note **out,
	git_repository *repo,
	git_commit *notes_commit,
	const git_oid *oid);

/**
 * Get the note author
 *
 * @param note the note
 * @return the author
 */
GIT_EXTERN(const git_signature *) git_note_author(const git_note *note);

/**
 * Get the note committer
 *
 * @param note the note
 * @return the committer
 */
GIT_EXTERN(const git_signature *) git_note_committer(const git_note *note);


/**
 * Get the note message
 *
 * @param note the note
 * @return the note message
 */
GIT_EXTERN(const char *) git_note_message(const git_note *note);


/**
 * Get the note object's id
 *
 * @param note the note
 * @return the note object's id
 */
GIT_EXTERN(const git_oid *) git_note_id(const git_note *note);

/**
 * Add a note for an object
 *
 * @param out pointer to store the OID (optional); NULL in case of error
 * @param repo repository where to store the note
 * @param notes_ref canonical name of the reference to use (optional);
 *					defaults to "refs/notes/commits"
 * @param author signature of the notes commit author
 * @param committer signature of the notes commit committer
 * @param oid OID of the git object to decorate
 * @param note Content of the note to add for object oid
 * @param force Overwrite existing note
 *
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_note_create(
	git_oid *out,
	git_repository *repo,
	const char *notes_ref,
	const git_signature *author,
	const git_signature *committer,
	const git_oid *oid,
	const char *note,
	int force);

/**
 * Add a note for an object from a commit
 *
 * This function will create a notes commit for a given object,
 * the commit is a dangling commit, no reference is created.
 *
 * @param notes_commit_out pointer to store the commit (optional);
 *					NULL in case of error
 * @param notes_blob_out a point to the id of a note blob (optional)
 * @param repo repository where the note will live
 * @param parent Pointer to parent note
 *					or NULL if this shall start a new notes tree
 * @param author signature of the notes commit author
 * @param committer signature of the notes commit committer
 * @param oid OID of the git object to decorate
 * @param note Content of the note to add for object oid
 * @param allow_note_overwrite Overwrite existing note
 *
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_note_commit_create(
	git_oid *notes_commit_out,
	git_oid *notes_blob_out,
	git_repository *repo,
	git_commit *parent,
	const git_signature *author,
	const git_signature *committer,
	const git_oid *oid,
	const char *note,
	int allow_note_overwrite);

/**
 * Remove the note for an object
 *
 * @param repo repository where the note lives
 * @param notes_ref canonical name of the reference to use (optional);
 *					defaults to "refs/notes/commits"
 * @param author signature of the notes commit author
 * @param committer signature of the notes commit committer
 * @param oid OID of the git object to remove the note from
 *
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_note_remove(
	git_repository *repo,
	const char *notes_ref,
	const git_signature *author,
	const git_signature *committer,
	const git_oid *oid);

/**
 * Remove the note for an object
 *
 * @param notes_commit_out pointer to store the new notes commit (optional);
 *					NULL in case of error.
 *					When removing a note a new tree containing all notes
 *					sans the note to be removed is created and a new commit
 *					pointing to that tree is also created.
 *					In the case where the resulting tree is an empty tree
 *					a new commit pointing to this empty tree will be returned.
 * @param repo repository where the note lives
 * @param notes_commit a pointer to the notes commit object
 * @param author signature of the notes commit author
 * @param committer signature of the notes commit committer
 * @param oid OID of the git object to remove the note from
 *
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_note_commit_remove(
		git_oid *notes_commit_out,
		git_repository *repo,
		git_commit *notes_commit,
		const git_signature *author,
		const git_signature *committer,
		const git_oid *oid);

/**
 * Free a git_note object
 *
 * @param note git_note object
 */
GIT_EXTERN(void) git_note_free(git_note *note);

/**
 * Get the default notes reference for a repository
 *
 * @param out buffer in which to store the name of the default notes reference
 * @param repo The Git repository
 *
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_note_default_ref(git_buf *out, git_repository *repo);

/**
 * Loop over all the notes within a specified namespace
 * and issue a callback for each one.
 *
 * @param repo Repository where to find the notes.
 *
 * @param notes_ref Reference to read from (optional); defaults to
 *        "refs/notes/commits".
 *
 * @param note_cb Callback to invoke per found annotation.  Return non-zero
 *        to stop looping.
 *
 * @param payload Extra parameter to callback function.
 *
 * @return 0 on success, non-zero callback return value, or error code
 */
GIT_EXTERN(int) git_note_foreach(
	git_repository *repo,
	const char *notes_ref,
	git_note_foreach_cb note_cb,
	void *payload);

/** @} */
GIT_END_DECL
#endif
