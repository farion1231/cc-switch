/*
 * Copyright (C) the libgit2 contributors. All rights reserved.
 *
 * This file is part of libgit2, distributed under the GNU GPL v2 with
 * a Linking Exception. For full terms see the included COPYING file.
 */
#ifndef INCLUDE_git_credential_helpers_h__
#define INCLUDE_git_credential_helpers_h__

#include "transport.h"

/**
 * @file git2/credential_helpers.h
 * @brief Utility functions for credential management
 * @defgroup git_credential_helpers credential management helpers
 * @ingroup Git
 * @{
 */
GIT_BEGIN_DECL

/**
 * Payload for git_credential_userpass_plaintext.
 */
typedef struct git_credential_userpass_payload {
	const char *username;
	const char *password;
} git_credential_userpass_payload;


/**
 * Stock callback usable as a git_credential_acquire_cb.  This calls
 * git_cred_userpass_plaintext_new unless the protocol has not specified
 * `GIT_CREDENTIAL_USERPASS_PLAINTEXT` as an allowed type.
 *
 * @param out The newly created credential object.
 * @param url The resource for which we are demanding a credential.
 * @param user_from_url The username that was embedded in a "user\@host"
 *                          remote url, or NULL if not included.
 * @param allowed_types A bitmask stating which credential types are OK to return.
 * @param payload The payload provided when specifying this callback.  (This is
 *        interpreted as a `git_credential_userpass_payload*`.)
 * @return 0 or an error code.
 */
GIT_EXTERN(int) git_credential_userpass(
		git_credential **out,
		const char *url,
		const char *user_from_url,
		unsigned int allowed_types,
		void *payload);

/** @} */
GIT_END_DECL
#endif
