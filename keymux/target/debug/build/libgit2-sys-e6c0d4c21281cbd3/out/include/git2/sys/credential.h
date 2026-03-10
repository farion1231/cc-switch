/*
 * Copyright (C) the libgit2 contributors. All rights reserved.
 *
 * This file is part of libgit2, distributed under the GNU GPL v2 with
 * a Linking Exception. For full terms see the included COPYING file.
 */
#ifndef INCLUDE_sys_git_credential_h__
#define INCLUDE_sys_git_credential_h__

#include "git2/common.h"
#include "git2/credential.h"

/**
 * @file git2/sys/cred.h
 * @brief Git credentials low-level implementation
 * @defgroup git_credential Git credentials low-level implementation
 * @ingroup Git
 * @{
 */
GIT_BEGIN_DECL

/**
 * The base structure for all credential types
 */
struct git_credential {
	git_credential_t credtype; /**< A type of credential */

	/** The deallocator for this type of credentials */
	void GIT_CALLBACK(free)(git_credential *cred);
};

/** A plaintext username and password */
struct git_credential_userpass_plaintext {
	git_credential parent; /**< The parent credential */
	char *username;        /**< The username to authenticate as */
	char *password;        /**< The password to use */
};

/** Username-only credential information */
struct git_credential_username {
	git_credential parent; /**< The parent credential */
	char username[1];      /**< The username to authenticate as */
};

/**
 * A ssh key from disk
 */
struct git_credential_ssh_key {
	git_credential parent; /**< The parent credential */
	char *username;        /**< The username to authenticate as */
	char *publickey;       /**< The path to a public key */
	char *privatekey;      /**< The path to a private key */
	char *passphrase;      /**< Passphrase to decrypt the private key */
};

/**
 * Keyboard-interactive based ssh authentication
 */
struct git_credential_ssh_interactive {
	git_credential parent; /**< The parent credential */
	char *username;        /**< The username to authenticate as */

	/**
	 * Callback used for authentication.
	 */
	git_credential_ssh_interactive_cb prompt_callback;

	void *payload;         /**< Payload passed to prompt_callback */
};

/**
 * A key with a custom signature function
 */
struct git_credential_ssh_custom {
	git_credential parent; /**< The parent credential */
	char *username;        /**< The username to authenticate as */
	char *publickey;       /**< The public key data */
	size_t publickey_len;  /**< Length of the public key */

	/**
	 * Callback used to sign the data.
	 */
	git_credential_sign_cb sign_callback;

	void *payload;         /**< Payload passed to prompt_callback */
};

GIT_END_DECL

#endif
