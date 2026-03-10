/*
 * Copyright (C) the libgit2 contributors. All rights reserved.
 *
 * This file is part of libgit2, distributed under the GNU GPL v2 with
 * a Linking Exception. For full terms see the included COPYING file.
 */
#ifndef INCLUDE_git_transaction_h__
#define INCLUDE_git_transaction_h__

#include "common.h"
#include "types.h"

/**
 * @file git2/transaction.h
 * @brief Git transactional reference routines
 * @defgroup git_transaction Git transactional reference routines
 * @ingroup Git
 * @{
 */
GIT_BEGIN_DECL

/**
 * Create a new transaction object
 *
 * This does not lock anything, but sets up the transaction object to
 * know from which repository to lock.
 *
 * @param out the resulting transaction
 * @param repo the repository in which to lock
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_transaction_new(git_transaction **out, git_repository *repo);

/**
 * Lock a reference
 *
 * Lock the specified reference. This is the first step to updating a
 * reference.
 *
 * @param tx the transaction
 * @param refname the reference to lock
 * @return 0 or an error message
 */
GIT_EXTERN(int) git_transaction_lock_ref(git_transaction *tx, const char *refname);

/**
 * Set the target of a reference
 *
 * Set the target of the specified reference. This reference must be
 * locked.
 *
 * @param tx the transaction
 * @param refname reference to update
 * @param target target to set the reference to
 * @param sig signature to use in the reflog; pass NULL to read the identity from the config
 * @param msg message to use in the reflog
 * @return 0, GIT_ENOTFOUND if the reference is not among the locked ones, or an error code
 */
GIT_EXTERN(int) git_transaction_set_target(git_transaction *tx, const char *refname, const git_oid *target, const git_signature *sig, const char *msg);

/**
 * Set the target of a reference
 *
 * Set the target of the specified reference. This reference must be
 * locked.
 *
 * @param tx the transaction
 * @param refname reference to update
 * @param target target to set the reference to
 * @param sig signature to use in the reflog; pass NULL to read the identity from the config
 * @param msg message to use in the reflog
 * @return 0, GIT_ENOTFOUND if the reference is not among the locked ones, or an error code
 */
GIT_EXTERN(int) git_transaction_set_symbolic_target(git_transaction *tx, const char *refname, const char *target, const git_signature *sig, const char *msg);

/**
 * Set the reflog of a reference
 *
 * Set the specified reference's reflog. If this is combined with
 * setting the target, that update won't be written to the reflog.
 *
 * @param tx the transaction
 * @param refname the reference whose reflog to set
 * @param reflog the reflog as it should be written out
 * @return 0, GIT_ENOTFOUND if the reference is not among the locked ones, or an error code
 */
GIT_EXTERN(int) git_transaction_set_reflog(git_transaction *tx, const char *refname, const git_reflog *reflog);

/**
 * Remove a reference
 *
 * @param tx the transaction
 * @param refname the reference to remove
 * @return 0, GIT_ENOTFOUND if the reference is not among the locked ones, or an error code
 */
GIT_EXTERN(int) git_transaction_remove(git_transaction *tx, const char *refname);

/**
 * Commit the changes from the transaction
 *
 * Perform the changes that have been queued. The updates will be made
 * one by one, and the first failure will stop the processing.
 *
 * @param tx the transaction
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_transaction_commit(git_transaction *tx);

/**
 * Free the resources allocated by this transaction
 *
 * If any references remain locked, they will be unlocked without any
 * changes made to them.
 *
 * @param tx the transaction
 */
GIT_EXTERN(void) git_transaction_free(git_transaction *tx);

/** @} */
GIT_END_DECL
#endif
