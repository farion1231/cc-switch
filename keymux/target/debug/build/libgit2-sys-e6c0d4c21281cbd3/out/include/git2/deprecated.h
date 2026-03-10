/*
 * Copyright (C) the libgit2 contributors. All rights reserved.
 *
 * This file is part of libgit2, distributed under the GNU GPL v2 with
 * a Linking Exception. For full terms see the included COPYING file.
 */
#ifndef INCLUDE_git_deprecated_h__
#define INCLUDE_git_deprecated_h__

#include "attr.h"
#include "config.h"
#include "common.h"
#include "blame.h"
#include "buffer.h"
#include "checkout.h"
#include "cherrypick.h"
#include "clone.h"
#include "describe.h"
#include "diff.h"
#include "errors.h"
#include "filter.h"
#include "index.h"
#include "indexer.h"
#include "merge.h"
#include "object.h"
#include "proxy.h"
#include "refs.h"
#include "rebase.h"
#include "remote.h"
#include "trace.h"
#include "repository.h"
#include "revert.h"
#include "revparse.h"
#include "stash.h"
#include "status.h"
#include "submodule.h"
#include "worktree.h"
#include "credential.h"
#include "credential_helpers.h"

/*
 * Users can avoid deprecated functions by defining `GIT_DEPRECATE_HARD`.
 */
#ifndef GIT_DEPRECATE_HARD

/*
 * The credential structures are now opaque by default, and their
 * definition has moved into the `sys/credential.h` header; include
 * them here for backward compatibility.
 */
#include "sys/credential.h"

/**
 * @file git2/deprecated.h
 * @brief libgit2 deprecated functions and values
 * @ingroup Git
 * @{
 */
GIT_BEGIN_DECL

/** @name Deprecated Attribute Constants
 *
 * These enumeration values are retained for backward compatibility.
 * The newer versions of these functions should be preferred in all
 * new code.
 *
 * There is no plan to remove these backward compatibility values at
 * this time.
 */
/**@{*/

#define GIT_ATTR_UNSPECIFIED_T GIT_ATTR_VALUE_UNSPECIFIED
#define GIT_ATTR_TRUE_T GIT_ATTR_VALUE_TRUE
#define GIT_ATTR_FALSE_T GIT_ATTR_VALUE_FALSE
#define GIT_ATTR_VALUE_T GIT_ATTR_VALUE_STRING

#define GIT_ATTR_TRUE(attr) GIT_ATTR_IS_TRUE(attr)
#define GIT_ATTR_FALSE(attr) GIT_ATTR_IS_FALSE(attr)
#define GIT_ATTR_UNSPECIFIED(attr) GIT_ATTR_IS_UNSPECIFIED(attr)

typedef git_attr_value_t git_attr_t;

/**@}*/

/** @name Deprecated Blob Functions and Constants
 *
 * These functions and enumeration values are retained for backward
 * compatibility.  The newer versions of these functions and values
 * should be preferred in all new code.
 *
 * There is no plan to remove these backward compatibility values at
 * this time.
 */
/**@{*/

#define GIT_BLOB_FILTER_ATTTRIBUTES_FROM_HEAD GIT_BLOB_FILTER_ATTRIBUTES_FROM_HEAD

GIT_EXTERN(int) git_blob_create_fromworkdir(git_oid *id, git_repository *repo, const char *relative_path);
GIT_EXTERN(int) git_blob_create_fromdisk(git_oid *id, git_repository *repo, const char *path);
GIT_EXTERN(int) git_blob_create_fromstream(
	git_writestream **out,
	git_repository *repo,
	const char *hintpath);
GIT_EXTERN(int) git_blob_create_fromstream_commit(
	git_oid *out,
	git_writestream *stream);
GIT_EXTERN(int) git_blob_create_frombuffer(
	git_oid *id, git_repository *repo, const void *buffer, size_t len);

/** Deprecated in favor of `git_blob_filter`.
 *
 * @deprecated Use git_blob_filter
 * @see git_blob_filter
 */
GIT_EXTERN(int) git_blob_filtered_content(
	git_buf *out,
	git_blob *blob,
	const char *as_path,
	int check_for_binary_data);

/**@}*/

/** @name Deprecated Filter Functions
 *
 * These functions are retained for backward compatibility.  The
 * newer versions of these functions should be preferred in all
 * new code.
 *
 * There is no plan to remove these backward compatibility values at
 * this time.
 */
/**@{*/

/** Deprecated in favor of `git_filter_list_stream_buffer`.
 *
 * @deprecated Use git_filter_list_stream_buffer
 * @see Use git_filter_list_stream_buffer
 */
GIT_EXTERN(int) git_filter_list_stream_data(
	git_filter_list *filters,
	git_buf *data,
	git_writestream *target);

/** Deprecated in favor of `git_filter_list_apply_to_buffer`.
 *
 * @deprecated Use git_filter_list_apply_to_buffer
 * @see Use git_filter_list_apply_to_buffer
 */
GIT_EXTERN(int) git_filter_list_apply_to_data(
	git_buf *out,
	git_filter_list *filters,
	git_buf *in);

/**@}*/

/** @name Deprecated Tree Functions
 *
 * These functions are retained for backward compatibility.  The
 * newer versions of these functions and values should be preferred
 * in all new code.
 *
 * There is no plan to remove these backward compatibility values at
 * this time.
 */
/**@{*/

/**
 * Write the contents of the tree builder as a tree object.
 * This is an alias of `git_treebuilder_write` and is preserved
 * for backward compatibility.
 *
 * This function is deprecated, but there is no plan to remove this
 * function at this time.
 *
 * @deprecated Use git_treebuilder_write
 * @see git_treebuilder_write
 */
GIT_EXTERN(int) git_treebuilder_write_with_buffer(
	git_oid *oid, git_treebuilder *bld, git_buf *tree);

/**@}*/

/** @name Deprecated Buffer Functions
 *
 * These functions and enumeration values are retained for backward
 * compatibility.  The newer versions of these functions should be
 * preferred in all new code.
 *
 * There is no plan to remove these backward compatibility values at
 * this time.
 */
/**@{*/

/**
 * Static initializer for git_buf from static buffer
 */
#define GIT_BUF_INIT_CONST(STR,LEN) { (char *)(STR), 0, (size_t)(LEN) }

/**
 * Resize the buffer allocation to make more space.
 *
 * This will attempt to grow the buffer to accommodate the target size.
 *
 * If the buffer refers to memory that was not allocated by libgit2 (i.e.
 * the `asize` field is zero), then `ptr` will be replaced with a newly
 * allocated block of data.  Be careful so that memory allocated by the
 * caller is not lost.  As a special variant, if you pass `target_size` as
 * 0 and the memory is not allocated by libgit2, this will allocate a new
 * buffer of size `size` and copy the external data into it.
 *
 * Currently, this will never shrink a buffer, only expand it.
 *
 * If the allocation fails, this will return an error and the buffer will be
 * marked as invalid for future operations, invaliding the contents.
 *
 * @param buffer The buffer to be resized; may or may not be allocated yet
 * @param target_size The desired available size
 * @return 0 on success, -1 on allocation failure
 */
GIT_EXTERN(int) git_buf_grow(git_buf *buffer, size_t target_size);

/**
 * Set buffer to a copy of some raw data.
 *
 * @param buffer The buffer to set
 * @param data The data to copy into the buffer
 * @param datalen The length of the data to copy into the buffer
 * @return 0 on success, -1 on allocation failure
 */
GIT_EXTERN(int) git_buf_set(
	git_buf *buffer, const void *data, size_t datalen);

/**
* Check quickly if buffer looks like it contains binary data
*
* @param buf Buffer to check
* @return 1 if buffer looks like non-text data
*/
GIT_EXTERN(int) git_buf_is_binary(const git_buf *buf);

/**
* Check quickly if buffer contains a NUL byte
*
* @param buf Buffer to check
* @return 1 if buffer contains a NUL byte
*/
GIT_EXTERN(int) git_buf_contains_nul(const git_buf *buf);

/**
 * Free the memory referred to by the git_buf.  This is an alias of
 * `git_buf_dispose` and is preserved for backward compatibility.
 *
 * This function is deprecated, but there is no plan to remove this
 * function at this time.
 *
 * @deprecated Use git_buf_dispose
 * @see git_buf_dispose
 */
GIT_EXTERN(void) git_buf_free(git_buf *buffer);

/**@}*/

/** @name Deprecated Commit Definitions
 */
/**@{*/

/**
 * Provide a commit signature during commit creation.
 *
 * Callers should instead define a `git_commit_create_cb` that
 * generates a commit buffer using `git_commit_create_buffer`, sign
 * that buffer and call `git_commit_create_with_signature`.
 *
 * @deprecated use a `git_commit_create_cb` instead
 */
typedef int (*git_commit_signing_cb)(
	git_buf *signature,
	git_buf *signature_field,
	const char *commit_content,
	void *payload);

/**@}*/

/** @name Deprecated Config Functions and Constants
 */
/**@{*/

#define GIT_CVAR_FALSE  GIT_CONFIGMAP_FALSE
#define GIT_CVAR_TRUE   GIT_CONFIGMAP_TRUE
#define GIT_CVAR_INT32  GIT_CONFIGMAP_INT32
#define GIT_CVAR_STRING GIT_CONFIGMAP_STRING

typedef git_configmap git_cvar_map;

/**@}*/

/** @name Deprecated Diff Functions and Constants
 *
 * These functions and enumeration values are retained for backward
 * compatibility.  The newer versions of these functions and values
 * should be preferred in all new code.
 *
 * There is no plan to remove these backward compatibility values at
 * this time.
 */
/**@{*/

/**
 * Formatting options for diff e-mail generation
 */
typedef enum {
	/** Normal patch, the default */
	GIT_DIFF_FORMAT_EMAIL_NONE = 0,

	/** Don't insert "[PATCH]" in the subject header*/
	GIT_DIFF_FORMAT_EMAIL_EXCLUDE_SUBJECT_PATCH_MARKER = (1 << 0)

} git_diff_format_email_flags_t;

/**
 * Options for controlling the formatting of the generated e-mail.
 */
typedef struct {
	unsigned int version;

	/** see `git_diff_format_email_flags_t` above */
	uint32_t flags;

	/** This patch number */
	size_t patch_no;

	/** Total number of patches in this series */
	size_t total_patches;

	/** id to use for the commit */
	const git_oid *id;

	/** Summary of the change */
	const char *summary;

	/** Commit message's body */
	const char *body;

	/** Author of the change */
	const git_signature *author;
} git_diff_format_email_options;

#define GIT_DIFF_FORMAT_EMAIL_OPTIONS_VERSION 1
#define GIT_DIFF_FORMAT_EMAIL_OPTIONS_INIT {GIT_DIFF_FORMAT_EMAIL_OPTIONS_VERSION, 0, 1, 1, NULL, NULL, NULL, NULL}

/**
 * Create an e-mail ready patch from a diff.
 *
 * @deprecated git_email_create_from_diff
 * @see git_email_create_from_diff
 */
GIT_EXTERN(int) git_diff_format_email(
	git_buf *out,
	git_diff *diff,
	const git_diff_format_email_options *opts);

/**
 * Create an e-mail ready patch for a commit.
 *
 * @deprecated git_email_create_from_commit
 * @see git_email_create_from_commit
 */
GIT_EXTERN(int) git_diff_commit_as_email(
	git_buf *out,
	git_repository *repo,
	git_commit *commit,
	size_t patch_no,
	size_t total_patches,
	uint32_t flags,
	const git_diff_options *diff_opts);

/**
 * Initialize git_diff_format_email_options structure
 *
 * Initializes a `git_diff_format_email_options` with default values. Equivalent
 * to creating an instance with GIT_DIFF_FORMAT_EMAIL_OPTIONS_INIT.
 *
 * @param opts The `git_blame_options` struct to initialize.
 * @param version The struct version; pass `GIT_DIFF_FORMAT_EMAIL_OPTIONS_VERSION`.
 * @return Zero on success; -1 on failure.
 */
GIT_EXTERN(int) git_diff_format_email_options_init(
	git_diff_format_email_options *opts,
	unsigned int version);

/**@}*/

/** @name Deprecated Error Functions and Constants
 *
 * These functions and enumeration values are retained for backward
 * compatibility.  The newer versions of these functions and values
 * should be preferred in all new code.
 *
 * There is no plan to remove these backward compatibility values at
 * this time.
 */
/**@{*/

#define GITERR_NONE GIT_ERROR_NONE
#define GITERR_NOMEMORY GIT_ERROR_NOMEMORY
#define GITERR_OS GIT_ERROR_OS
#define GITERR_INVALID GIT_ERROR_INVALID
#define GITERR_REFERENCE GIT_ERROR_REFERENCE
#define GITERR_ZLIB GIT_ERROR_ZLIB
#define GITERR_REPOSITORY GIT_ERROR_REPOSITORY
#define GITERR_CONFIG GIT_ERROR_CONFIG
#define GITERR_REGEX GIT_ERROR_REGEX
#define GITERR_ODB GIT_ERROR_ODB
#define GITERR_INDEX GIT_ERROR_INDEX
#define GITERR_OBJECT GIT_ERROR_OBJECT
#define GITERR_NET GIT_ERROR_NET
#define GITERR_TAG GIT_ERROR_TAG
#define GITERR_TREE GIT_ERROR_TREE
#define GITERR_INDEXER GIT_ERROR_INDEXER
#define GITERR_SSL GIT_ERROR_SSL
#define GITERR_SUBMODULE GIT_ERROR_SUBMODULE
#define GITERR_THREAD GIT_ERROR_THREAD
#define GITERR_STASH GIT_ERROR_STASH
#define GITERR_CHECKOUT GIT_ERROR_CHECKOUT
#define GITERR_FETCHHEAD GIT_ERROR_FETCHHEAD
#define GITERR_MERGE GIT_ERROR_MERGE
#define GITERR_SSH GIT_ERROR_SSH
#define GITERR_FILTER GIT_ERROR_FILTER
#define GITERR_REVERT GIT_ERROR_REVERT
#define GITERR_CALLBACK GIT_ERROR_CALLBACK
#define GITERR_CHERRYPICK GIT_ERROR_CHERRYPICK
#define GITERR_DESCRIBE GIT_ERROR_DESCRIBE
#define GITERR_REBASE GIT_ERROR_REBASE
#define GITERR_FILESYSTEM GIT_ERROR_FILESYSTEM
#define GITERR_PATCH GIT_ERROR_PATCH
#define GITERR_WORKTREE GIT_ERROR_WORKTREE
#define GITERR_SHA1 GIT_ERROR_SHA1

#define GIT_ERROR_SHA1 GIT_ERROR_SHA

/**
 * Return the last `git_error` object that was generated for the
 * current thread.  This is an alias of `git_error_last` and is
 * preserved for backward compatibility.
 *
 * This function is deprecated, but there is no plan to remove this
 * function at this time.
 *
 * @deprecated Use git_error_last
 * @see git_error_last
 */
GIT_EXTERN(const git_error *) giterr_last(void);

/**
 * Clear the last error.  This is an alias of `git_error_last` and is
 * preserved for backward compatibility.
 *
 * This function is deprecated, but there is no plan to remove this
 * function at this time.
 *
 * @deprecated Use git_error_clear
 * @see git_error_clear
 */
GIT_EXTERN(void) giterr_clear(void);

/**
 * Sets the error message to the given string.  This is an alias of
 * `git_error_set_str` and is preserved for backward compatibility.
 *
 * This function is deprecated, but there is no plan to remove this
 * function at this time.
 *
 * @deprecated Use git_error_set_str
 * @see git_error_set_str
 */
GIT_EXTERN(void) giterr_set_str(int error_class, const char *string);

/**
 * Indicates that an out-of-memory situation occurred.  This is an alias
 * of `git_error_set_oom` and is preserved for backward compatibility.
 *
 * This function is deprecated, but there is no plan to remove this
 * function at this time.
 *
 * @deprecated Use git_error_set_oom
 * @see git_error_set_oom
 */
GIT_EXTERN(void) giterr_set_oom(void);

/**@}*/

/** @name Deprecated Index Functions and Constants
 *
 * These functions and enumeration values are retained for backward
 * compatibility.  The newer versions of these values should be
 * preferred in all new code.
 *
 * There is no plan to remove these backward compatibility values at
 * this time.
 */
/**@{*/

#define GIT_IDXENTRY_NAMEMASK          GIT_INDEX_ENTRY_NAMEMASK
#define GIT_IDXENTRY_STAGEMASK         GIT_INDEX_ENTRY_STAGEMASK
#define GIT_IDXENTRY_STAGESHIFT        GIT_INDEX_ENTRY_STAGESHIFT

/* The git_indxentry_flag_t enum */
#define GIT_IDXENTRY_EXTENDED          GIT_INDEX_ENTRY_EXTENDED
#define GIT_IDXENTRY_VALID             GIT_INDEX_ENTRY_VALID

#define GIT_IDXENTRY_STAGE(E)          GIT_INDEX_ENTRY_STAGE(E)
#define GIT_IDXENTRY_STAGE_SET(E,S)    GIT_INDEX_ENTRY_STAGE_SET(E,S)

/* The git_idxentry_extended_flag_t enum */
#define GIT_IDXENTRY_INTENT_TO_ADD     GIT_INDEX_ENTRY_INTENT_TO_ADD
#define GIT_IDXENTRY_SKIP_WORKTREE     GIT_INDEX_ENTRY_SKIP_WORKTREE
#define GIT_IDXENTRY_EXTENDED_FLAGS    (GIT_INDEX_ENTRY_INTENT_TO_ADD | GIT_INDEX_ENTRY_SKIP_WORKTREE)
#define GIT_IDXENTRY_EXTENDED2         (1 << 15)
#define GIT_IDXENTRY_UPDATE            (1 << 0)
#define GIT_IDXENTRY_REMOVE            (1 << 1)
#define GIT_IDXENTRY_UPTODATE          (1 << 2)
#define GIT_IDXENTRY_ADDED             (1 << 3)
#define GIT_IDXENTRY_HASHED            (1 << 4)
#define GIT_IDXENTRY_UNHASHED          (1 << 5)
#define GIT_IDXENTRY_WT_REMOVE         (1 << 6)
#define GIT_IDXENTRY_CONFLICTED        (1 << 7)
#define GIT_IDXENTRY_UNPACKED          (1 << 8)
#define GIT_IDXENTRY_NEW_SKIP_WORKTREE (1 << 9)

/* The git_index_capability_t enum */
#define GIT_INDEXCAP_IGNORE_CASE       GIT_INDEX_CAPABILITY_IGNORE_CASE
#define GIT_INDEXCAP_NO_FILEMODE       GIT_INDEX_CAPABILITY_NO_FILEMODE
#define GIT_INDEXCAP_NO_SYMLINKS       GIT_INDEX_CAPABILITY_NO_SYMLINKS
#define GIT_INDEXCAP_FROM_OWNER        GIT_INDEX_CAPABILITY_FROM_OWNER

GIT_EXTERN(int) git_index_add_frombuffer(
	git_index *index,
	const git_index_entry *entry,
	const void *buffer, size_t len);

/**@}*/

/** @name Deprecated Object Constants
 *
 * These enumeration values are retained for backward compatibility.  The
 * newer versions of these values should be preferred in all new code.
 *
 * There is no plan to remove these backward compatibility values at
 * this time.
 */
/**@{*/

#define git_otype git_object_t

#define GIT_OBJ_ANY GIT_OBJECT_ANY
#define GIT_OBJ_BAD GIT_OBJECT_INVALID
#define GIT_OBJ__EXT1 0
#define GIT_OBJ_COMMIT GIT_OBJECT_COMMIT
#define GIT_OBJ_TREE GIT_OBJECT_TREE
#define GIT_OBJ_BLOB GIT_OBJECT_BLOB
#define GIT_OBJ_TAG GIT_OBJECT_TAG
#define GIT_OBJ__EXT2 5
#define GIT_OBJ_OFS_DELTA GIT_OBJECT_OFS_DELTA
#define GIT_OBJ_REF_DELTA GIT_OBJECT_REF_DELTA

/**
 * Get the size in bytes for the structure which
 * acts as an in-memory representation of any given
 * object type.
 *
 * For all the core types, this would the equivalent
 * of calling `sizeof(git_commit)` if the core types
 * were not opaque on the external API.
 *
 * @param type object type to get its size
 * @return size in bytes of the object
 */
GIT_EXTERN(size_t) git_object__size(git_object_t type);

/**@}*/

/** @name Deprecated Remote Functions
 *
 * These functions are retained for backward compatibility.  The newer
 * versions of these functions should be preferred in all new code.
 *
 * There is no plan to remove these backward compatibility functions at
 * this time.
 */
/**@{*/

/**
 * Ensure the remote name is well-formed.
 *
 * @deprecated Use git_remote_name_is_valid
 * @param remote_name name to be checked.
 * @return 1 if the reference name is acceptable; 0 if it isn't
 */
GIT_EXTERN(int) git_remote_is_valid_name(const char *remote_name);

/**@}*/

/** @name Deprecated Reference Functions and Constants
 *
 * These functions and enumeration values are retained for backward
 * compatibility.  The newer versions of these values should be
 * preferred in all new code.
 *
 * There is no plan to remove these backward compatibility values at
 * this time.
 */
/**@{*/

 /** Basic type of any Git reference. */
#define git_ref_t git_reference_t
#define git_reference_normalize_t git_reference_format_t

#define GIT_REF_INVALID GIT_REFERENCE_INVALID
#define GIT_REF_OID GIT_REFERENCE_DIRECT
#define GIT_REF_SYMBOLIC GIT_REFERENCE_SYMBOLIC
#define GIT_REF_LISTALL GIT_REFERENCE_ALL

#define GIT_REF_FORMAT_NORMAL GIT_REFERENCE_FORMAT_NORMAL
#define GIT_REF_FORMAT_ALLOW_ONELEVEL GIT_REFERENCE_FORMAT_ALLOW_ONELEVEL
#define GIT_REF_FORMAT_REFSPEC_PATTERN GIT_REFERENCE_FORMAT_REFSPEC_PATTERN
#define GIT_REF_FORMAT_REFSPEC_SHORTHAND GIT_REFERENCE_FORMAT_REFSPEC_SHORTHAND

/**
 * Ensure the reference name is well-formed.
 *
 * Valid reference names must follow one of two patterns:
 *
 * 1. Top-level names must contain only capital letters and underscores,
 *    and must begin and end with a letter. (e.g. "HEAD", "ORIG_HEAD").
 * 2. Names prefixed with "refs/" can be almost anything.  You must avoid
 *    the characters '~', '^', ':', '\\', '?', '[', and '*', and the
 *    sequences ".." and "@{" which have special meaning to revparse.
 *
 * @deprecated Use git_reference_name_is_valid
 * @param refname name to be checked.
 * @return 1 if the reference name is acceptable; 0 if it isn't
 */
GIT_EXTERN(int) git_reference_is_valid_name(const char *refname);

GIT_EXTERN(int) git_tag_create_frombuffer(
	git_oid *oid,
	git_repository *repo,
	const char *buffer,
	int force);

/**@}*/

/** @name Deprecated Revspec Constants
 *
 * These enumeration values are retained for backward compatibility.
 * The newer versions of these values should be preferred in all new
 * code.
 *
 * There is no plan to remove these backward compatibility values at
 * this time.
 */
/**@{*/

typedef git_revspec_t git_revparse_mode_t;

#define GIT_REVPARSE_SINGLE GIT_REVSPEC_SINGLE
#define GIT_REVPARSE_RANGE GIT_REVSPEC_RANGE
#define GIT_REVPARSE_MERGE_BASE GIT_REVSPEC_MERGE_BASE

/**@}*/

/** @name Deprecated Credential Types
 *
 * These types are retained for backward compatibility.  The newer
 * versions of these values should be preferred in all new code.
 *
 * There is no plan to remove these backward compatibility values at
 * this time.
 */
/**@{*/

typedef git_credential git_cred;
typedef git_credential_userpass_plaintext git_cred_userpass_plaintext;
typedef git_credential_username git_cred_username;
typedef git_credential_default git_cred_default;
typedef git_credential_ssh_key git_cred_ssh_key;
typedef git_credential_ssh_interactive git_cred_ssh_interactive;
typedef git_credential_ssh_custom git_cred_ssh_custom;

typedef git_credential_acquire_cb git_cred_acquire_cb;
typedef git_credential_sign_cb git_cred_sign_callback;
typedef git_credential_sign_cb git_cred_sign_cb;
typedef git_credential_ssh_interactive_cb git_cred_ssh_interactive_callback;
typedef git_credential_ssh_interactive_cb git_cred_ssh_interactive_cb;

#define git_credtype_t git_credential_t

#define GIT_CREDTYPE_USERPASS_PLAINTEXT GIT_CREDENTIAL_USERPASS_PLAINTEXT
#define GIT_CREDTYPE_SSH_KEY GIT_CREDENTIAL_SSH_KEY
#define GIT_CREDTYPE_SSH_CUSTOM GIT_CREDENTIAL_SSH_CUSTOM
#define GIT_CREDTYPE_DEFAULT GIT_CREDENTIAL_DEFAULT
#define GIT_CREDTYPE_SSH_INTERACTIVE GIT_CREDENTIAL_SSH_INTERACTIVE
#define GIT_CREDTYPE_USERNAME GIT_CREDENTIAL_USERNAME
#define GIT_CREDTYPE_SSH_MEMORY GIT_CREDENTIAL_SSH_MEMORY

GIT_EXTERN(void) git_cred_free(git_credential *cred);
GIT_EXTERN(int) git_cred_has_username(git_credential *cred);
GIT_EXTERN(const char *) git_cred_get_username(git_credential *cred);
GIT_EXTERN(int) git_cred_userpass_plaintext_new(
	git_credential **out,
	const char *username,
	const char *password);
GIT_EXTERN(int) git_cred_default_new(git_credential **out);
GIT_EXTERN(int) git_cred_username_new(git_credential **out, const char *username);
GIT_EXTERN(int) git_cred_ssh_key_new(
	git_credential **out,
	const char *username,
	const char *publickey,
	const char *privatekey,
	const char *passphrase);
GIT_EXTERN(int) git_cred_ssh_key_memory_new(
	git_credential **out,
	const char *username,
	const char *publickey,
	const char *privatekey,
	const char *passphrase);
GIT_EXTERN(int) git_cred_ssh_interactive_new(
	git_credential **out,
	const char *username,
	git_credential_ssh_interactive_cb prompt_callback,
	void *payload);
GIT_EXTERN(int) git_cred_ssh_key_from_agent(
	git_credential **out,
	const char *username);
GIT_EXTERN(int) git_cred_ssh_custom_new(
	git_credential **out,
	const char *username,
	const char *publickey,
	size_t publickey_len,
	git_credential_sign_cb sign_callback,
	void *payload);

/* Deprecated Credential Helper Types */

typedef git_credential_userpass_payload git_cred_userpass_payload;

GIT_EXTERN(int) git_cred_userpass(
	git_credential **out,
	const char *url,
	const char *user_from_url,
	unsigned int allowed_types,
	void *payload);

/**@}*/

/** @name Deprecated Trace Callback Types
 *
 * These types are retained for backward compatibility.  The newer
 * versions of these values should be preferred in all new code.
 *
 * There is no plan to remove these backward compatibility values at
 * this time.
 */
/**@{*/

typedef git_trace_cb git_trace_callback;

/**@}*/

/** @name Deprecated Object ID Types
 *
 * These types are retained for backward compatibility.  The newer
 * versions of these values should be preferred in all new code.
 *
 * There is no plan to remove these backward compatibility values at
 * this time.
 */
/**@{*/

#ifndef GIT_EXPERIMENTAL_SHA256
# define GIT_OID_RAWSZ    GIT_OID_SHA1_SIZE
# define GIT_OID_HEXSZ    GIT_OID_SHA1_HEXSIZE
# define GIT_OID_HEX_ZERO GIT_OID_SHA1_HEXZERO
#endif

GIT_EXTERN(int) git_oid_iszero(const git_oid *id);

/**@}*/

/** @name Deprecated OID Array Functions
 *
 * These types are retained for backward compatibility.  The newer
 * versions of these values should be preferred in all new code.
 *
 * There is no plan to remove these backward compatibility values at
 * this time.
 */
/**@{*/

/**
 * Free the memory referred to by the git_oidarray.  This is an alias of
 * `git_oidarray_dispose` and is preserved for backward compatibility.
 *
 * This function is deprecated, but there is no plan to remove this
 * function at this time.
 *
 * @deprecated Use git_oidarray_dispose
 * @see git_oidarray_dispose
 */
GIT_EXTERN(void) git_oidarray_free(git_oidarray *array);

/**@}*/

/** @name Deprecated Transfer Progress Types
 *
 * These types are retained for backward compatibility.  The newer
 * versions of these values should be preferred in all new code.
 *
 * There is no plan to remove these backward compatibility values at
 * this time.
 */
/**@{*/

/**
 * This structure is used to provide callers information about the
 * progress of indexing a packfile.
 *
 * This type is deprecated, but there is no plan to remove this
 * type definition at this time.
 */
typedef git_indexer_progress git_transfer_progress;

/**
 * Type definition for progress callbacks during indexing.
 *
 * This type is deprecated, but there is no plan to remove this
 * type definition at this time.
 */
typedef git_indexer_progress_cb git_transfer_progress_cb;

/**
 * Type definition for push transfer progress callbacks.
 *
 * This type is deprecated, but there is no plan to remove this
 * type definition at this time.
 */
typedef git_push_transfer_progress_cb git_push_transfer_progress;

 /** The type of a remote completion event */
#define git_remote_completion_type git_remote_completion_t

/**
 * Callback for listing the remote heads
 */
typedef int GIT_CALLBACK(git_headlist_cb)(git_remote_head *rhead, void *payload);

/**@}*/

/** @name Deprecated String Array Functions
 *
 * These types are retained for backward compatibility.  The newer
 * versions of these values should be preferred in all new code.
 *
 * There is no plan to remove these backward compatibility values at
 * this time.
 */
/**@{*/

/**
 * Copy a string array object from source to target.
 *
 * This function is deprecated, but there is no plan to remove this
 * function at this time.
 *
 * @param tgt target
 * @param src source
 * @return 0 on success, < 0 on allocation failure
 */
GIT_EXTERN(int) git_strarray_copy(git_strarray *tgt, const git_strarray *src);

/**
 * Free the memory referred to by the git_strarray.  This is an alias of
 * `git_strarray_dispose` and is preserved for backward compatibility.
 *
 * This function is deprecated, but there is no plan to remove this
 * function at this time.
 *
 * @deprecated Use git_strarray_dispose
 * @see git_strarray_dispose
 */
GIT_EXTERN(void) git_strarray_free(git_strarray *array);

/**@}*/

/** @name Deprecated Options Initialization Functions
 *
 * These functions are retained for backward compatibility.  The newer
 * versions of these functions should be preferred in all new code.
 *
 * There is no plan to remove these backward compatibility functions at
 * this time.
 */
/**@{*/

GIT_EXTERN(int) git_blame_init_options(git_blame_options *opts, unsigned int version);
GIT_EXTERN(int) git_checkout_init_options(git_checkout_options *opts, unsigned int version);
GIT_EXTERN(int) git_cherrypick_init_options(git_cherrypick_options *opts, unsigned int version);
GIT_EXTERN(int) git_clone_init_options(git_clone_options *opts, unsigned int version);
GIT_EXTERN(int) git_describe_init_options(git_describe_options *opts, unsigned int version);
GIT_EXTERN(int) git_describe_init_format_options(git_describe_format_options *opts, unsigned int version);
GIT_EXTERN(int) git_diff_init_options(git_diff_options *opts, unsigned int version);
GIT_EXTERN(int) git_diff_find_init_options(git_diff_find_options *opts, unsigned int version);
GIT_EXTERN(int) git_diff_format_email_init_options(git_diff_format_email_options *opts, unsigned int version);
GIT_EXTERN(int) git_diff_patchid_init_options(git_diff_patchid_options *opts, unsigned int version);
GIT_EXTERN(int) git_fetch_init_options(git_fetch_options *opts, unsigned int version);
GIT_EXTERN(int) git_indexer_init_options(git_indexer_options *opts, unsigned int version);
GIT_EXTERN(int) git_merge_init_options(git_merge_options *opts, unsigned int version);
GIT_EXTERN(int) git_merge_file_init_input(git_merge_file_input *input, unsigned int version);
GIT_EXTERN(int) git_merge_file_init_options(git_merge_file_options *opts, unsigned int version);
GIT_EXTERN(int) git_proxy_init_options(git_proxy_options *opts, unsigned int version);
GIT_EXTERN(int) git_push_init_options(git_push_options *opts, unsigned int version);
GIT_EXTERN(int) git_rebase_init_options(git_rebase_options *opts, unsigned int version);
GIT_EXTERN(int) git_remote_create_init_options(git_remote_create_options *opts, unsigned int version);
GIT_EXTERN(int) git_repository_init_init_options(git_repository_init_options *opts, unsigned int version);
GIT_EXTERN(int) git_revert_init_options(git_revert_options *opts, unsigned int version);
GIT_EXTERN(int) git_stash_apply_init_options(git_stash_apply_options *opts, unsigned int version);
GIT_EXTERN(int) git_status_init_options(git_status_options *opts, unsigned int version);
GIT_EXTERN(int) git_submodule_update_init_options(git_submodule_update_options *opts, unsigned int version);
GIT_EXTERN(int) git_worktree_add_init_options(git_worktree_add_options *opts, unsigned int version);
GIT_EXTERN(int) git_worktree_prune_init_options(git_worktree_prune_options *opts, unsigned int version);

/**@}*/

/** @} */
GIT_END_DECL

#endif

#endif
