/*
 * Copyright (C) the libgit2 contributors. All rights reserved.
 *
 * This file is part of libgit2, distributed under the GNU GPL v2 with
 * a Linking Exception. For full terms see the included COPYING file.
 */

#ifndef INCLUDE_sys_git_transport_h
#define INCLUDE_sys_git_transport_h

#include "git2/net.h"
#include "git2/oidarray.h"
#include "git2/proxy.h"
#include "git2/remote.h"
#include "git2/strarray.h"
#include "git2/transport.h"
#include "git2/types.h"

/**
 * @file git2/sys/transport.h
 * @brief Git custom transport registration interfaces and functions
 * @defgroup git_transport Git custom transport registration
 * @ingroup Git
 * @{
 */

GIT_BEGIN_DECL

typedef struct {
	const git_remote_head * const *refs;
	size_t refs_len;
	git_oid *shallow_roots;
	size_t shallow_roots_len;
	int depth;
} git_fetch_negotiation;

struct git_transport {
	unsigned int version; /**< The struct version */

	/**
	 * Connect the transport to the remote repository, using the given
	 * direction.
	 */
	int GIT_CALLBACK(connect)(
		git_transport *transport,
		const char *url,
		int direction,
		const git_remote_connect_options *connect_opts);

	/**
	 * Resets the connect options for the given transport.  This
	 * is useful for updating settings or callbacks for an already
	 * connected transport.
	 */
	int GIT_CALLBACK(set_connect_opts)(
		git_transport *transport,
		const git_remote_connect_options *connect_opts);

	/**
	 * Gets the capabilities for this remote repository.
	 *
	 * This function may be called after a successful call to
	 * `connect()`.
	 */
	int GIT_CALLBACK(capabilities)(
		unsigned int *capabilities,
		git_transport *transport);

#ifdef GIT_EXPERIMENTAL_SHA256
	/**
	 * Gets the object type for the remote repository.
	 *
	 * This function may be called after a successful call to
	 * `connect()`.
	 */
	int GIT_CALLBACK(oid_type)(
		git_oid_t *object_type,
		git_transport *transport);
#endif

	/**
	 * Get the list of available references in the remote repository.
	 *
	 * This function may be called after a successful call to
	 * `connect()`. The array returned is owned by the transport and
	 * must be kept valid until the next call to one of its functions.
	 */
	int GIT_CALLBACK(ls)(
		const git_remote_head ***out,
		size_t *size,
		git_transport *transport);

	/** Executes the push whose context is in the git_push object. */
	int GIT_CALLBACK(push)(
		git_transport *transport,
		git_push *push);

	/**
	 * Negotiate a fetch with the remote repository.
	 *
	 * This function may be called after a successful call to `connect()`,
	 * when the direction is GIT_DIRECTION_FETCH. The function performs a
	 * negotiation to calculate the `wants` list for the fetch.
	 */
	int GIT_CALLBACK(negotiate_fetch)(
		git_transport *transport,
		git_repository *repo,
		const git_fetch_negotiation *fetch_data);

	/**
	 * Return the shallow roots of the remote.
	 *
	 * This function may be called after a successful call to
	 * `negotiate_fetch`.
	 */
	int GIT_CALLBACK(shallow_roots)(
		git_oidarray *out,
		git_transport *transport);

	/**
	 * Start downloading the packfile from the remote repository.
	 *
	 * This function may be called after a successful call to
	 * negotiate_fetch(), when the direction is GIT_DIRECTION_FETCH.
	 */
	int GIT_CALLBACK(download_pack)(
		git_transport *transport,
		git_repository *repo,
		git_indexer_progress *stats);

	/** Checks to see if the transport is connected */
	int GIT_CALLBACK(is_connected)(git_transport *transport);

	/** Cancels any outstanding transport operation */
	void GIT_CALLBACK(cancel)(git_transport *transport);

	/**
	 * Close the connection to the remote repository.
	 *
	 * This function is the reverse of connect() -- it terminates the
	 * connection to the remote end.
	 */
	int GIT_CALLBACK(close)(git_transport *transport);

	/** Frees/destructs the git_transport object. */
	void GIT_CALLBACK(free)(git_transport *transport);
};

#define GIT_TRANSPORT_VERSION 1
#define GIT_TRANSPORT_INIT {GIT_TRANSPORT_VERSION}

/**
 * Initializes a `git_transport` with default values. Equivalent to
 * creating an instance with GIT_TRANSPORT_INIT.
 *
 * @param opts the `git_transport` struct to initialize
 * @param version Version of struct; pass `GIT_TRANSPORT_VERSION`
 * @return Zero on success; -1 on failure.
 */
GIT_EXTERN(int) git_transport_init(
	git_transport *opts,
	unsigned int version);

/**
 * Function to use to create a transport from a URL. The transport database
 * is scanned to find a transport that implements the scheme of the URI (i.e.
 * git:// or http://) and a transport object is returned to the caller.
 *
 * @param out The newly created transport (out)
 * @param owner The git_remote which will own this transport
 * @param url The URL to connect to
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_transport_new(git_transport **out, git_remote *owner, const char *url);

/**
 * Create an ssh transport with custom git command paths
 *
 * This is a factory function suitable for setting as the transport
 * callback in a remote (or for a clone in the options).
 *
 * The payload argument must be a strarray pointer with the paths for
 * the `git-upload-pack` and `git-receive-pack` at index 0 and 1.
 *
 * @param out the resulting transport
 * @param owner the owning remote
 * @param payload a strarray with the paths
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_transport_ssh_with_paths(git_transport **out, git_remote *owner, void *payload);

/**
 * Add a custom transport definition, to be used in addition to the built-in
 * set of transports that come with libgit2.
 *
 * The caller is responsible for synchronizing calls to git_transport_register
 * and git_transport_unregister with other calls to the library that
 * instantiate transports.
 *
 * @param prefix The scheme (ending in "://") to match, i.e. "git://"
 * @param cb The callback used to create an instance of the transport
 * @param param A fixed parameter to pass to cb at creation time
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_transport_register(
	const char *prefix,
	git_transport_cb cb,
	void *param);

/**
 * Unregister a custom transport definition which was previously registered
 * with git_transport_register.
 *
 * The caller is responsible for synchronizing calls to git_transport_register
 * and git_transport_unregister with other calls to the library that
 * instantiate transports.
 *
 * @param prefix From the previous call to git_transport_register
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_transport_unregister(
	const char *prefix);

/* Transports which come with libgit2 (match git_transport_cb). The expected
 * value for "param" is listed in-line below. */

/**
 * Create an instance of the dummy transport.
 *
 * @param out The newly created transport (out)
 * @param owner The git_remote which will own this transport
 * @param payload You must pass NULL for this parameter.
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_transport_dummy(
	git_transport **out,
	git_remote *owner,
	/* NULL */ void *payload);

/**
 * Create an instance of the local transport.
 *
 * @param out The newly created transport (out)
 * @param owner The git_remote which will own this transport
 * @param payload You must pass NULL for this parameter.
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_transport_local(
	git_transport **out,
	git_remote *owner,
	/* NULL */ void *payload);

/**
 * Create an instance of the smart transport.
 *
 * @param out The newly created transport (out)
 * @param owner The git_remote which will own this transport
 * @param payload A pointer to a git_smart_subtransport_definition
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_transport_smart(
	git_transport **out,
	git_remote *owner,
	/* (git_smart_subtransport_definition *) */ void *payload);

/**
 * Call the certificate check for this transport.
 *
 * @param transport a smart transport
 * @param cert the certificate to pass to the caller
 * @param valid whether we believe the certificate is valid
 * @param hostname the hostname we connected to
 * @return the return value of the callback: 0 for no error, GIT_PASSTHROUGH
 *         to indicate that there is no callback registered (or the callback
 *         refused to validate the certificate and callers should behave as
 *         if no callback was set), or < 0 for an error
 */
GIT_EXTERN(int) git_transport_smart_certificate_check(git_transport *transport, git_cert *cert, int valid, const char *hostname);

/**
 * Call the credentials callback for this transport
 *
 * @param out the pointer where the creds are to be stored
 * @param transport a smart transport
 * @param user the user we saw on the url (if any)
 * @param methods available methods for authentication
 * @return the return value of the callback: 0 for no error, GIT_PASSTHROUGH
 *         to indicate that there is no callback registered (or the callback
 *         refused to provide credentials and callers should behave as if no
 *         callback was set), or < 0 for an error
 */
GIT_EXTERN(int) git_transport_smart_credentials(git_credential **out, git_transport *transport, const char *user, int methods);

/**
 * Get a copy of the remote connect options
 *
 * All data is copied and must be freed by the caller by calling
 * `git_remote_connect_options_dispose`.
 *
 * @param out options struct to fill
 * @param transport the transport to extract the data from.
 */
GIT_EXTERN(int) git_transport_remote_connect_options(
		git_remote_connect_options *out,
		git_transport *transport);

/*
 *** End of base transport interface ***
 *** Begin interface for subtransports for the smart transport ***
 */

/** Actions that the smart transport can ask a subtransport to perform */
typedef enum {
	GIT_SERVICE_UPLOADPACK_LS = 1,
	GIT_SERVICE_UPLOADPACK = 2,
	GIT_SERVICE_RECEIVEPACK_LS = 3,
	GIT_SERVICE_RECEIVEPACK = 4
} git_smart_service_t;

typedef struct git_smart_subtransport git_smart_subtransport;
typedef struct git_smart_subtransport_stream git_smart_subtransport_stream;

/**
 * A stream used by the smart transport to read and write data
 * from a subtransport.
 *
 * This provides a customization point in case you need to
 * support some other communication method.
 */
struct git_smart_subtransport_stream {
	git_smart_subtransport *subtransport; /**< The owning subtransport */

	/**
	 * Read available data from the stream.
	 *
	 * The implementation may read less than requested.
	 */
	int GIT_CALLBACK(read)(
		git_smart_subtransport_stream *stream,
		char *buffer,
		size_t buf_size,
		size_t *bytes_read);

	/**
	 * Write data to the stream
	 *
	 * The implementation must write all data or return an error.
	 */
	int GIT_CALLBACK(write)(
		git_smart_subtransport_stream *stream,
		const char *buffer,
		size_t len);

	/** Free the stream */
	void GIT_CALLBACK(free)(
		git_smart_subtransport_stream *stream);
};

/**
 * An implementation of a subtransport which carries data for the
 * smart transport
 */
struct git_smart_subtransport {
	/**
	 * Setup a subtransport stream for the requested action.
	 */
	int GIT_CALLBACK(action)(
			git_smart_subtransport_stream **out,
			git_smart_subtransport *transport,
			const char *url,
			git_smart_service_t action);

	/**
	 * Close the subtransport.
	 *
	 * Subtransports are guaranteed a call to close() between
	 * calls to action(), except for the following two "natural" progressions
	 * of actions against a constant URL:
	 *
	 * - UPLOADPACK_LS -> UPLOADPACK
	 * - RECEIVEPACK_LS -> RECEIVEPACK
	 */
	int GIT_CALLBACK(close)(git_smart_subtransport *transport);

	/** Free the subtransport */
	void GIT_CALLBACK(free)(git_smart_subtransport *transport);
};

/** A function which creates a new subtransport for the smart transport */
typedef int GIT_CALLBACK(git_smart_subtransport_cb)(
	git_smart_subtransport **out,
	git_transport *owner,
	void *param);

/**
 * Definition for a "subtransport"
 *
 * The smart transport knows how to speak the git protocol, but it has no
 * knowledge of how to establish a connection between it and another endpoint,
 * or how to move data back and forth. For this, a subtransport interface is
 * declared, and the smart transport delegates this work to the subtransports.
 *
 * Three subtransports are provided by libgit2: ssh, git, http(s).
 *
 * Subtransports can either be RPC = 0 (persistent connection) or RPC = 1
 * (request/response). The smart transport handles the differences in its own
 * logic. The git subtransport is RPC = 0, while http is RPC = 1.
 */
typedef struct git_smart_subtransport_definition {
	/** The function to use to create the git_smart_subtransport */
	git_smart_subtransport_cb callback;

	/**
	 * True if the protocol is stateless; false otherwise. For example,
	 * http:// is stateless, but git:// is not.
	 */
	unsigned rpc;

	/** User-specified parameter passed to the callback */
	void *param;
} git_smart_subtransport_definition;

/* Smart transport subtransports that come with libgit2 */

/**
 * Create an instance of the http subtransport.
 *
 * This subtransport also supports https.
 *
 * @param out The newly created subtransport
 * @param owner The smart transport to own this subtransport
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_smart_subtransport_http(
	git_smart_subtransport **out,
	git_transport *owner,
	void *param);

/**
 * Create an instance of the git subtransport.
 *
 * @param out The newly created subtransport
 * @param owner The smart transport to own this subtransport
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_smart_subtransport_git(
	git_smart_subtransport **out,
	git_transport *owner,
	void *param);

/**
 * Create an instance of the ssh subtransport.
 *
 * @param out The newly created subtransport
 * @param owner The smart transport to own this subtransport
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_smart_subtransport_ssh(
	git_smart_subtransport **out,
	git_transport *owner,
	void *param);

/** @} */
GIT_END_DECL
#endif
