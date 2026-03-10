/*
 * Copyright (C) the libgit2 contributors. All rights reserved.
 *
 * This file is part of libgit2, distributed under the GNU GPL v2 with
 * a Linking Exception. For full terms see the included COPYING file.
 */
#ifndef INCLUDE_git_remote_h__
#define INCLUDE_git_remote_h__

#include "common.h"
#include "repository.h"
#include "refspec.h"
#include "net.h"
#include "indexer.h"
#include "strarray.h"
#include "transport.h"
#include "pack.h"
#include "proxy.h"

/**
 * @file git2/remote.h
 * @brief Git remote management functions
 * @defgroup git_remote remote management functions
 * @ingroup Git
 * @{
 */
GIT_BEGIN_DECL

/**
 * Add a remote with the default fetch refspec to the repository's configuration.
 *
 * @param out the resulting remote
 * @param repo the repository in which to create the remote
 * @param name the remote's name
 * @param url the remote's url
 * @return 0, GIT_EINVALIDSPEC, GIT_EEXISTS or an error code
 */
GIT_EXTERN(int) git_remote_create(
		git_remote **out,
		git_repository *repo,
		const char *name,
		const char *url);

/**
 * Remote redirection settings; whether redirects to another host
 * are permitted.  By default, git will follow a redirect on the
 * initial request (`/info/refs`), but not subsequent requests.
 */
typedef enum {
	/**
	 * Do not follow any off-site redirects at any stage of
	 * the fetch or push.
	 */
	GIT_REMOTE_REDIRECT_NONE = (1 << 0),

	/**
	 * Allow off-site redirects only upon the initial request.
	 * This is the default.
	 */
	GIT_REMOTE_REDIRECT_INITIAL = (1 << 1),

	/**
	 * Allow redirects at any stage in the fetch or push.
	 */
	GIT_REMOTE_REDIRECT_ALL = (1 << 2)
} git_remote_redirect_t;

/**
 * Remote creation options flags
 */
typedef enum {
	/** Ignore the repository apply.insteadOf configuration */
	GIT_REMOTE_CREATE_SKIP_INSTEADOF = (1 << 0),

	/** Don't build a fetchspec from the name if none is set */
	GIT_REMOTE_CREATE_SKIP_DEFAULT_FETCHSPEC = (1 << 1)
} git_remote_create_flags;

/**
 * Remote creation options structure
 *
 * Initialize with `GIT_REMOTE_CREATE_OPTIONS_INIT`. Alternatively, you can
 * use `git_remote_create_options_init`.
 *
 */
typedef struct git_remote_create_options {
	unsigned int version;

	/**
	 * The repository that should own the remote.
	 * Setting this to NULL results in a detached remote.
	 */
	git_repository *repository;

	/**
	 * The remote's name.
	 * Setting this to NULL results in an in-memory/anonymous remote.
	 */
	const char *name;

	/** The fetchspec the remote should use. */
	const char *fetchspec;

	/** Additional flags for the remote. See git_remote_create_flags. */
	unsigned int flags;
} git_remote_create_options;

#define GIT_REMOTE_CREATE_OPTIONS_VERSION 1
#define GIT_REMOTE_CREATE_OPTIONS_INIT {GIT_REMOTE_CREATE_OPTIONS_VERSION}

/**
 * Initialize git_remote_create_options structure
 *
 * Initializes a `git_remote_create_options` with default values. Equivalent to
 * creating an instance with `GIT_REMOTE_CREATE_OPTIONS_INIT`.
 *
 * @param opts The `git_remote_create_options` struct to initialize.
 * @param version The struct version; pass `GIT_REMOTE_CREATE_OPTIONS_VERSION`.
 * @return Zero on success; -1 on failure.
 */
GIT_EXTERN(int) git_remote_create_options_init(
		git_remote_create_options *opts,
		unsigned int version);

/**
 * Create a remote, with options.
 *
 * This function allows more fine-grained control over the remote creation.
 *
 * Passing NULL as the opts argument will result in a detached remote.
 *
 * @param out the resulting remote
 * @param url the remote's url
 * @param opts the remote creation options
 * @return 0, GIT_EINVALIDSPEC, GIT_EEXISTS or an error code
 */
GIT_EXTERN(int) git_remote_create_with_opts(
		git_remote **out,
		const char *url,
		const git_remote_create_options *opts);

/**
 * Add a remote with the provided fetch refspec (or default if NULL) to the repository's
 * configuration.
 *
 * @param out the resulting remote
 * @param repo the repository in which to create the remote
 * @param name the remote's name
 * @param url the remote's url
 * @param fetch the remote fetch value
 * @return 0, GIT_EINVALIDSPEC, GIT_EEXISTS or an error code
 */
GIT_EXTERN(int) git_remote_create_with_fetchspec(
		git_remote **out,
		git_repository *repo,
		const char *name,
		const char *url,
		const char *fetch);

/**
 * Create an anonymous remote
 *
 * Create a remote with the given url in-memory. You can use this when
 * you have a URL instead of a remote's name.
 *
 * @param out pointer to the new remote objects
 * @param repo the associated repository
 * @param url the remote repository's URL
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_remote_create_anonymous(
		git_remote **out,
		git_repository *repo,
		const char *url);

/**
 * Create a remote without a connected local repo
 *
 * Create a remote with the given url in-memory. You can use this when
 * you have a URL instead of a remote's name.
 *
 * Contrasted with git_remote_create_anonymous, a detached remote
 * will not consider any repo configuration values (such as insteadof url
 * substitutions).
 *
 * @param out pointer to the new remote objects
 * @param url the remote repository's URL
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_remote_create_detached(
		git_remote **out,
		const char *url);

/**
 * Get the information for a particular remote
 *
 * The name will be checked for validity.
 * See `git_tag_create()` for rules about valid names.
 *
 * @param out pointer to the new remote object
 * @param repo the associated repository
 * @param name the remote's name
 * @return 0, GIT_ENOTFOUND, GIT_EINVALIDSPEC or an error code
 */
GIT_EXTERN(int) git_remote_lookup(git_remote **out, git_repository *repo, const char *name);

/**
 * Create a copy of an existing remote.  All internal strings are also
 * duplicated. Callbacks are not duplicated.
 *
 * Call `git_remote_free` to free the data.
 *
 * @param dest pointer where to store the copy
 * @param source object to copy
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_remote_dup(git_remote **dest, git_remote *source);

/**
 * Get the remote's repository
 *
 * @param remote the remote
 * @return a pointer to the repository
 */
GIT_EXTERN(git_repository *) git_remote_owner(const git_remote *remote);

/**
 * Get the remote's name
 *
 * @param remote the remote
 * @return a pointer to the name or NULL for in-memory remotes
 */
GIT_EXTERN(const char *) git_remote_name(const git_remote *remote);

/**
 * Get the remote's url
 *
 * If url.*.insteadOf has been configured for this URL, it will
 * return the modified URL.  If `git_remote_set_instance_pushurl`
 * has been called for this remote, then that URL will be returned.
 *
 * @param remote the remote
 * @return a pointer to the url
 */
GIT_EXTERN(const char *) git_remote_url(const git_remote *remote);

/**
 * Get the remote's url for pushing.
 *
 * If url.*.pushInsteadOf has been configured for this URL, it
 * will return the modified URL.  If `git_remote_set_instance_pushurl`
 * has been called for this remote, then that URL will be returned.
 *
 * @param remote the remote
 * @return a pointer to the url or NULL if no special url for pushing is set
 */
GIT_EXTERN(const char *) git_remote_pushurl(const git_remote *remote);

/**
 * Set the remote's url in the configuration
 *
 * Remote objects already in memory will not be affected. This assumes
 * the common case of a single-url remote and will otherwise return an error.
 *
 * @param repo the repository in which to perform the change
 * @param remote the remote's name
 * @param url the url to set
 * @return 0 or an error value
 */
GIT_EXTERN(int) git_remote_set_url(git_repository *repo, const char *remote, const char *url);

/**
 * Set the remote's url for pushing in the configuration.
 *
 * Remote objects already in memory will not be affected. This assumes
 * the common case of a single-url remote and will otherwise return an error.
 *
 *
 * @param repo the repository in which to perform the change
 * @param remote the remote's name
 * @param url the url to set
 * @return 0, or an error code
 */
GIT_EXTERN(int) git_remote_set_pushurl(git_repository *repo, const char *remote, const char *url);

/**
 * Set the url for this particular url instance.  The URL in the
 * configuration will be ignored, and will not be changed.
 *
 * @param remote the remote's name
 * @param url the url to set
 * @return 0 or an error value
 */
GIT_EXTERN(int) git_remote_set_instance_url(git_remote *remote, const char *url);

/**
 * Set the push url for this particular url instance.  The URL in the
 * configuration will be ignored, and will not be changed.
 *
 * @param remote the remote's name
 * @param url the url to set
 * @return 0 or an error value
 */
GIT_EXTERN(int) git_remote_set_instance_pushurl(git_remote *remote, const char *url);

/**
 * Add a fetch refspec to the remote's configuration
 *
 * Add the given refspec to the fetch list in the configuration. No
 * loaded remote instances will be affected.
 *
 * @param repo the repository in which to change the configuration
 * @param remote the name of the remote to change
 * @param refspec the new fetch refspec
 * @return 0, GIT_EINVALIDSPEC if refspec is invalid or an error value
 */
GIT_EXTERN(int) git_remote_add_fetch(git_repository *repo, const char *remote, const char *refspec);

/**
 * Get the remote's list of fetch refspecs
 *
 * The memory is owned by the user and should be freed with
 * `git_strarray_free`.
 *
 * @param array pointer to the array in which to store the strings
 * @param remote the remote to query
 * @return 0 or an error code.
 */
GIT_EXTERN(int) git_remote_get_fetch_refspecs(git_strarray *array, const git_remote *remote);

/**
 * Add a push refspec to the remote's configuration
 *
 * Add the given refspec to the push list in the configuration. No
 * loaded remote instances will be affected.
 *
 * @param repo the repository in which to change the configuration
 * @param remote the name of the remote to change
 * @param refspec the new push refspec
 * @return 0, GIT_EINVALIDSPEC if refspec is invalid or an error value
 */
GIT_EXTERN(int) git_remote_add_push(git_repository *repo, const char *remote, const char *refspec);

/**
 * Get the remote's list of push refspecs
 *
 * The memory is owned by the user and should be freed with
 * `git_strarray_free`.
 *
 * @param array pointer to the array in which to store the strings
 * @param remote the remote to query
 * @return 0 or an error code.
 */
GIT_EXTERN(int) git_remote_get_push_refspecs(git_strarray *array, const git_remote *remote);

/**
 * Get the number of refspecs for a remote
 *
 * @param remote the remote
 * @return the amount of refspecs configured in this remote
 */
GIT_EXTERN(size_t) git_remote_refspec_count(const git_remote *remote);

/**
 * Get a refspec from the remote
 *
 * @param remote the remote to query
 * @param n the refspec to get
 * @return the nth refspec
 */
GIT_EXTERN(const git_refspec *)git_remote_get_refspec(const git_remote *remote, size_t n);

/**
 * Get the remote repository's reference advertisement list
 *
 * Get the list of references with which the server responds to a new
 * connection.
 *
 * The remote (or more exactly its transport) must have connected to
 * the remote repository. This list is available as soon as the
 * connection to the remote is initiated and it remains available
 * after disconnecting.
 *
 * The memory belongs to the remote. The pointer will be valid as long
 * as a new connection is not initiated, but it is recommended that
 * you make a copy in order to make use of the data.
 *
 * @param out pointer to the array
 * @param size the number of remote heads
 * @param remote the remote
 * @return 0 on success, or an error code
 */
GIT_EXTERN(int) git_remote_ls(const git_remote_head ***out,  size_t *size, git_remote *remote);

/**
 * Check whether the remote is connected
 *
 * Check whether the remote's underlying transport is connected to the
 * remote host.
 *
 * @param remote the remote
 * @return 1 if it's connected, 0 otherwise.
 */
GIT_EXTERN(int) git_remote_connected(const git_remote *remote);

/**
 * Cancel the operation
 *
 * At certain points in its operation, the network code checks whether
 * the operation has been cancelled and if so stops the operation.
 *
 * @param remote the remote
 * @return 0 on success, or an error code
 */
GIT_EXTERN(int) git_remote_stop(git_remote *remote);

/**
 * Disconnect from the remote
 *
 * Close the connection to the remote.
 *
 * @param remote the remote to disconnect from
 * @return 0 on success, or an error code
 */
GIT_EXTERN(int) git_remote_disconnect(git_remote *remote);

/**
 * Free the memory associated with a remote
 *
 * This also disconnects from the remote, if the connection
 * has not been closed yet (using git_remote_disconnect).
 *
 * @param remote the remote to free
 */
GIT_EXTERN(void) git_remote_free(git_remote *remote);

/**
 * Get a list of the configured remotes for a repo
 *
 * The string array must be freed by the user.
 *
 * @param out a string array which receives the names of the remotes
 * @param repo the repository to query
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_remote_list(git_strarray *out, git_repository *repo);

/**
 * Argument to the completion callback which tells it which operation
 * finished.
 */
typedef enum git_remote_completion_t {
	GIT_REMOTE_COMPLETION_DOWNLOAD,
	GIT_REMOTE_COMPLETION_INDEXING,
	GIT_REMOTE_COMPLETION_ERROR
} git_remote_completion_t;

/** Push network progress notification function */
typedef int GIT_CALLBACK(git_push_transfer_progress_cb)(
	unsigned int current,
	unsigned int total,
	size_t bytes,
	void *payload);

/**
 * Represents an update which will be performed on the remote during push
 */
typedef struct {
	/**
	 * The source name of the reference
	 */
	char *src_refname;
	/**
	 * The name of the reference to update on the server
	 */
	char *dst_refname;
	/**
	 * The current target of the reference
	 */
	git_oid src;
	/**
	 * The new target for the reference
	 */
	git_oid dst;
} git_push_update;

/**
 * Callback used to inform of upcoming updates.
 *
 * @param updates an array containing the updates which will be sent
 * as commands to the destination.
 * @param len number of elements in `updates`
 * @param payload Payload provided by the caller
 */
typedef int GIT_CALLBACK(git_push_negotiation)(const git_push_update **updates, size_t len, void *payload);

/**
 * Callback used to inform of the update status from the remote.
 *
 * Called for each updated reference on push. If `status` is
 * not `NULL`, the update was rejected by the remote server
 * and `status` contains the reason given.
 *
 * @param refname refname specifying to the remote ref
 * @param status status message sent from the remote
 * @param data data provided by the caller
 * @return 0 on success, otherwise an error
 */
typedef int GIT_CALLBACK(git_push_update_reference_cb)(const char *refname, const char *status, void *data);

#ifndef GIT_DEPRECATE_HARD
/**
 * Callback to resolve URLs before connecting to remote
 *
 * If you return GIT_PASSTHROUGH, you don't need to write anything to
 * url_resolved.
 *
 * @param url_resolved The buffer to write the resolved URL to
 * @param url The URL to resolve
 * @param direction GIT_DIRECTION_FETCH or GIT_DIRECTION_PUSH
 * @param payload Payload provided by the caller
 * @return 0 on success, GIT_PASSTHROUGH or an error
 * @deprecated Use `git_remote_set_instance_url`
 */
typedef int GIT_CALLBACK(git_url_resolve_cb)(git_buf *url_resolved, const char *url, int direction, void *payload);
#endif

/**
 * Callback invoked immediately before we attempt to connect to the
 * given url.  Callers may change the URL before the connection by
 * calling `git_remote_set_instance_url` in the callback.
 *
 * @param remote The remote to be connected
 * @param direction GIT_DIRECTION_FETCH or GIT_DIRECTION_PUSH
 * @param payload Payload provided by the caller
 * @return 0 on success, or an error
 */
typedef int GIT_CALLBACK(git_remote_ready_cb)(git_remote *remote, int direction, void *payload);

/**
 * The callback settings structure
 *
 * Set the callbacks to be called by the remote when informing the user
 * about the progress of the network operations.
 */
struct git_remote_callbacks {
	unsigned int version; /**< The version */

	/**
	 * Textual progress from the remote. Text send over the
	 * progress side-band will be passed to this function (this is
	 * the 'counting objects' output).
	 */
	git_transport_message_cb sideband_progress;

	/**
	 * Completion is called when different parts of the download
	 * process are done (currently unused).
	 */
	int GIT_CALLBACK(completion)(git_remote_completion_t type, void *data);

	/**
	 * This will be called if the remote host requires
	 * authentication in order to connect to it.
	 *
	 * Returning GIT_PASSTHROUGH will make libgit2 behave as
	 * though this field isn't set.
	 */
	git_credential_acquire_cb credentials;

	/**
	 * If cert verification fails, this will be called to let the
	 * user make the final decision of whether to allow the
	 * connection to proceed. Returns 0 to allow the connection
	 * or a negative value to indicate an error.
	 */
	git_transport_certificate_check_cb certificate_check;

	/**
	 * During the download of new data, this will be regularly
	 * called with the current count of progress done by the
	 * indexer.
	 */
	git_indexer_progress_cb transfer_progress;

	/**
	 * Each time a reference is updated locally, this function
	 * will be called with information about it.
	 */
	int GIT_CALLBACK(update_tips)(const char *refname, const git_oid *a, const git_oid *b, void *data);

	/**
	 * Function to call with progress information during pack
	 * building. Be aware that this is called inline with pack
	 * building operations, so performance may be affected.
	 */
	git_packbuilder_progress pack_progress;

	/**
	 * Function to call with progress information during the
	 * upload portion of a push. Be aware that this is called
	 * inline with pack building operations, so performance may be
	 * affected.
	 */
	git_push_transfer_progress_cb push_transfer_progress;

	/**
	 * See documentation of git_push_update_reference_cb
	 */
	git_push_update_reference_cb push_update_reference;

	/**
	 * Called once between the negotiation step and the upload. It
	 * provides information about what updates will be performed.
	 */
	git_push_negotiation push_negotiation;

	/**
	 * Create the transport to use for this operation. Leave NULL
	 * to auto-detect.
	 */
	git_transport_cb transport;

	/**
	 * Callback when the remote is ready to connect.
	 */
	git_remote_ready_cb remote_ready;

	/**
	 * This will be passed to each of the callbacks in this struct
	 * as the last parameter.
	 */
	void *payload;

#ifdef GIT_DEPRECATE_HARD
	void *reserved;
#else
	/**
	 * Resolve URL before connecting to remote.
	 * The returned URL will be used to connect to the remote instead.
	 *
	 * This callback is deprecated; users should use
	 * git_remote_ready_cb and configure the instance URL instead.
	 */
	git_url_resolve_cb resolve_url;
#endif
};

#define GIT_REMOTE_CALLBACKS_VERSION 1
#define GIT_REMOTE_CALLBACKS_INIT {GIT_REMOTE_CALLBACKS_VERSION}

/**
 * Initializes a `git_remote_callbacks` with default values. Equivalent to
 * creating an instance with GIT_REMOTE_CALLBACKS_INIT.
 *
 * @param opts the `git_remote_callbacks` struct to initialize
 * @param version Version of struct; pass `GIT_REMOTE_CALLBACKS_VERSION`
 * @return Zero on success; -1 on failure.
 */
GIT_EXTERN(int) git_remote_init_callbacks(
	git_remote_callbacks *opts,
	unsigned int version);

/** Acceptable prune settings when fetching */
typedef enum {
	/**
	 * Use the setting from the configuration
	 */
	GIT_FETCH_PRUNE_UNSPECIFIED,
	/**
	 * Force pruning on
	 */
	GIT_FETCH_PRUNE,
	/**
	 * Force pruning off
	 */
	GIT_FETCH_NO_PRUNE
} git_fetch_prune_t;

/**
 * Automatic tag following option
 *
 * Lets us select the --tags option to use.
 */
typedef enum {
	/**
	 * Use the setting from the configuration.
	 */
	GIT_REMOTE_DOWNLOAD_TAGS_UNSPECIFIED = 0,
	/**
	 * Ask the server for tags pointing to objects we're already
	 * downloading.
	 */
	GIT_REMOTE_DOWNLOAD_TAGS_AUTO,
	/**
	 * Don't ask for any tags beyond the refspecs.
	 */
	GIT_REMOTE_DOWNLOAD_TAGS_NONE,
	/**
	 * Ask for the all the tags.
	 */
	GIT_REMOTE_DOWNLOAD_TAGS_ALL
} git_remote_autotag_option_t;

/** Constants for fetch depth (shallowness of fetch). */
typedef enum {
	/** The fetch is "full" (not shallow). This is the default. */
	GIT_FETCH_DEPTH_FULL = 0,

	/** The fetch should "unshallow" and fetch missing data. */
	GIT_FETCH_DEPTH_UNSHALLOW = 2147483647
} git_fetch_depth_t;

/**
 * Fetch options structure.
 *
 * Zero out for defaults.  Initialize with `GIT_FETCH_OPTIONS_INIT` macro to
 * correctly set the `version` field.  E.g.
 *
 *		git_fetch_options opts = GIT_FETCH_OPTIONS_INIT;
 */
typedef struct {
	int version;

	/**
	 * Callbacks to use for this fetch operation
	 */
	git_remote_callbacks callbacks;

	/**
	 * Whether to perform a prune after the fetch
	 */
	git_fetch_prune_t prune;

	/**
	 * Whether to write the results to FETCH_HEAD. Defaults to
	 * on. Leave this default in order to behave like git.
	 */
	int update_fetchhead;

	/**
	 * Determines how to behave regarding tags on the remote, such
	 * as auto-downloading tags for objects we're downloading or
	 * downloading all of them.
	 *
	 * The default is to auto-follow tags.
	 */
	git_remote_autotag_option_t download_tags;

	/**
	 * Proxy options to use, by default no proxy is used.
	 */
	git_proxy_options proxy_opts;

	/**
	 * Depth of the fetch to perform, or `GIT_FETCH_DEPTH_FULL`
	 * (or `0`) for full history, or `GIT_FETCH_DEPTH_UNSHALLOW`
	 * to "unshallow" a shallow repository.
	 *
	 * The default is full (`GIT_FETCH_DEPTH_FULL` or `0`).
	 */
	int depth;

	/**
	 * Whether to allow off-site redirects.  If this is not
	 * specified, the `http.followRedirects` configuration setting
	 * will be consulted.
	 */
	git_remote_redirect_t follow_redirects;

	/**
	 * Extra headers for this fetch operation
	 */
	git_strarray custom_headers;
} git_fetch_options;

#define GIT_FETCH_OPTIONS_VERSION 1
#define GIT_FETCH_OPTIONS_INIT { GIT_FETCH_OPTIONS_VERSION, GIT_REMOTE_CALLBACKS_INIT, GIT_FETCH_PRUNE_UNSPECIFIED, 1, \
				 GIT_REMOTE_DOWNLOAD_TAGS_UNSPECIFIED, GIT_PROXY_OPTIONS_INIT }

/**
 * Initialize git_fetch_options structure
 *
 * Initializes a `git_fetch_options` with default values. Equivalent to
 * creating an instance with `GIT_FETCH_OPTIONS_INIT`.
 *
 * @param opts The `git_fetch_options` struct to initialize.
 * @param version The struct version; pass `GIT_FETCH_OPTIONS_VERSION`.
 * @return Zero on success; -1 on failure.
 */
GIT_EXTERN(int) git_fetch_options_init(
	git_fetch_options *opts,
	unsigned int version);


/**
 * Controls the behavior of a git_push object.
 */
typedef struct {
	unsigned int version;

	/**
	 * If the transport being used to push to the remote requires the creation
	 * of a pack file, this controls the number of worker threads used by
	 * the packbuilder when creating that pack file to be sent to the remote.
	 *
	 * If set to 0, the packbuilder will auto-detect the number of threads
	 * to create. The default value is 1.
	 */
	unsigned int pb_parallelism;

	/**
	 * Callbacks to use for this push operation
	 */
	git_remote_callbacks callbacks;

	/**
	* Proxy options to use, by default no proxy is used.
	*/
	git_proxy_options proxy_opts;

	/**
	 * Whether to allow off-site redirects.  If this is not
	 * specified, the `http.followRedirects` configuration setting
	 * will be consulted.
	 */
	git_remote_redirect_t follow_redirects;

	/**
	 * Extra headers for this push operation
	 */
	git_strarray custom_headers;
} git_push_options;

#define GIT_PUSH_OPTIONS_VERSION 1
#define GIT_PUSH_OPTIONS_INIT { GIT_PUSH_OPTIONS_VERSION, 1, GIT_REMOTE_CALLBACKS_INIT, GIT_PROXY_OPTIONS_INIT }

/**
 * Initialize git_push_options structure
 *
 * Initializes a `git_push_options` with default values. Equivalent to
 * creating an instance with `GIT_PUSH_OPTIONS_INIT`.
 *
 * @param opts The `git_push_options` struct to initialize.
 * @param version The struct version; pass `GIT_PUSH_OPTIONS_VERSION`.
 * @return Zero on success; -1 on failure.
 */
GIT_EXTERN(int) git_push_options_init(
	git_push_options *opts,
	unsigned int version);

/**
 * Remote creation options structure
 *
 * Initialize with `GIT_REMOTE_CREATE_OPTIONS_INIT`. Alternatively, you can
 * use `git_remote_create_options_init`.
 *
 */
typedef struct {
	unsigned int version;

	/** Callbacks to use for this connection */
	git_remote_callbacks callbacks;

	/** HTTP Proxy settings */
	git_proxy_options proxy_opts;

	/**
	 * Whether to allow off-site redirects.  If this is not
	 * specified, the `http.followRedirects` configuration setting
	 * will be consulted.
	 */
	git_remote_redirect_t follow_redirects;

	/** Extra HTTP headers to use in this connection */
	git_strarray custom_headers;
} git_remote_connect_options;

#define GIT_REMOTE_CONNECT_OPTIONS_VERSION 1
#define GIT_REMOTE_CONNECT_OPTIONS_INIT { \
	GIT_REMOTE_CONNECT_OPTIONS_VERSION, \
	GIT_REMOTE_CALLBACKS_INIT, \
	GIT_PROXY_OPTIONS_INIT }

/**
 * Initialize git_remote_connect_options structure.
 *
 * Initializes a `git_remote_connect_options` with default values.
 * Equivalent to creating an instance with
 * `GIT_REMOTE_CONNECT_OPTIONS_INIT`.
 *
 * @param opts The `git_remote_connect_options` struct to initialize.
 * @param version The struct version; pass `GIT_REMOTE_CONNECT_OPTIONS_VERSION`.
 * @return Zero on success; -1 on failure.
 */
GIT_EXTERN(int) git_remote_connect_options_init(
		git_remote_connect_options *opts,
		unsigned int version);

/**
 * Open a connection to a remote.
 *
 * The transport is selected based on the URL; the direction argument
 * is due to a limitation of the git protocol which starts up a
 * specific binary which can only do the one or the other.
 *
 * @param remote the remote to connect to
 * @param direction GIT_DIRECTION_FETCH if you want to fetch or
 * GIT_DIRECTION_PUSH if you want to push
 * @param callbacks the callbacks to use for this connection
 * @param proxy_opts proxy settings
 * @param custom_headers extra HTTP headers to use in this connection
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_remote_connect(
	git_remote *remote,
	git_direction direction,
	const git_remote_callbacks *callbacks,
	const git_proxy_options *proxy_opts,
	const git_strarray *custom_headers);

/**
 * Open a connection to a remote with extended options.
 *
 * The transport is selected based on the URL; the direction argument
 * is due to a limitation of the git protocol which starts up a
 * specific binary which can only do the one or the other.
 *
 * The given options structure will form the defaults for connection
 * options and callback setup.  Callers may override these defaults
 * by specifying `git_fetch_options` or `git_push_options` in
 * subsequent calls.
 *
 * @param remote the remote to connect to
 * @param direction GIT_DIRECTION_FETCH if you want to fetch or
 * GIT_DIRECTION_PUSH if you want to push
 * @param opts the remote connection options
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_remote_connect_ext(
	git_remote *remote,
	git_direction direction,
	const git_remote_connect_options *opts);

/**
 * Download and index the packfile.
 *
 * Connect to the remote if it hasn't been done yet, negotiate with
 * the remote git which objects are missing, download and index the
 * packfile.
 *
 * The .idx file will be created and both it and the packfile with be
 * renamed to their final name.
 *
 * If options are specified and this remote is already connected then
 * the existing remote connection options will be discarded and the
 * remote will now use the new options.
 *
 * @param remote the remote
 * @param refspecs the refspecs to use for this negotiation and
 * download. Use NULL or an empty array to use the base refspecs
 * @param opts the options to use for this fetch or NULL
 * @return 0 or an error code
 */
 GIT_EXTERN(int) git_remote_download(
	git_remote *remote,
	const git_strarray *refspecs,
	const git_fetch_options *opts);

/**
 * Create a packfile and send it to the server
 *
 * Connect to the remote if it hasn't been done yet, negotiate with
 * the remote git which objects are missing, create a packfile with
 * the missing objects and send it.
 *
 * If options are specified and this remote is already connected then
 * the existing remote connection options will be discarded and the
 * remote will now use the new options.
 *
 * @param remote the remote
 * @param refspecs the refspecs to use for this negotiation and
 * upload. Use NULL or an empty array to use the base refspecs
 * @param opts the options to use for this push
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_remote_upload(
	git_remote *remote,
	const git_strarray *refspecs,
	const git_push_options *opts);

/**
 * Update the tips to the new state.
 *
 * If callbacks are not specified then the callbacks specified to
 * `git_remote_connect` will be used (if it was called).
 *
 * @param remote the remote to update
 * @param reflog_message The message to insert into the reflogs. If
 * NULL and fetching, the default is "fetch <name>", where <name> is
 * the name of the remote (or its url, for in-memory remotes). This
 * parameter is ignored when pushing.
 * @param callbacks  pointer to the callback structure to use or NULL
 * @param update_fetchhead whether to write to FETCH_HEAD. Pass 1 to behave like git.
 * @param download_tags what the behaviour for downloading tags is for this fetch. This is
 * ignored for push. This must be the same value passed to `git_remote_download()`.
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_remote_update_tips(
		git_remote *remote,
		const git_remote_callbacks *callbacks,
		int update_fetchhead,
		git_remote_autotag_option_t download_tags,
		const char *reflog_message);

/**
 * Download new data and update tips.
 *
 * Convenience function to connect to a remote, download the data,
 * disconnect and update the remote-tracking branches.
 *
 * If options are specified and this remote is already connected then
 * the existing remote connection options will be discarded and the
 * remote will now use the new options.
 *
 * @param remote the remote to fetch from
 * @param refspecs the refspecs to use for this fetch. Pass NULL or an
 *                 empty array to use the base refspecs.
 * @param opts options to use for this fetch or NULL
 * @param reflog_message The message to insert into the reflogs. If NULL, the
 *								 default is "fetch"
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_remote_fetch(
		git_remote *remote,
		const git_strarray *refspecs,
		const git_fetch_options *opts,
		const char *reflog_message);

/**
 * Prune tracking refs that are no longer present on remote.
 *
 * If callbacks are not specified then the callbacks specified to
 * `git_remote_connect` will be used (if it was called).
 *
 * @param remote the remote to prune
 * @param callbacks callbacks to use for this prune
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_remote_prune(
	git_remote *remote,
	const git_remote_callbacks *callbacks);

/**
 * Perform a push.
 *
 * If options are specified and this remote is already connected then
 * the existing remote connection options will be discarded and the
 * remote will now use the new options.
 *
 * @param remote the remote to push to
 * @param refspecs the refspecs to use for pushing. If NULL or an empty
 *                 array, the configured refspecs will be used
 * @param opts options to use for this push
 * @return 0 or an error code.
 */
GIT_EXTERN(int) git_remote_push(
	git_remote *remote,
	const git_strarray *refspecs,
	const git_push_options *opts);

/**
 * Get the statistics structure that is filled in by the fetch operation.
 */
GIT_EXTERN(const git_indexer_progress *) git_remote_stats(git_remote *remote);

/**
 * Retrieve the tag auto-follow setting
 *
 * @param remote the remote to query
 * @return the auto-follow setting
 */
GIT_EXTERN(git_remote_autotag_option_t) git_remote_autotag(const git_remote *remote);

/**
 * Set the remote's tag following setting.
 *
 * The change will be made in the configuration. No loaded remotes
 * will be affected.
 *
 * @param repo the repository in which to make the change
 * @param remote the name of the remote
 * @param value the new value to take.
 * @return 0, or an error code.
 */
GIT_EXTERN(int) git_remote_set_autotag(git_repository *repo, const char *remote, git_remote_autotag_option_t value);

/**
 * Retrieve the ref-prune setting
 *
 * @param remote the remote to query
 * @return the ref-prune setting
 */
GIT_EXTERN(int) git_remote_prune_refs(const git_remote *remote);

/**
 * Give the remote a new name
 *
 * All remote-tracking branches and configuration settings
 * for the remote are updated.
 *
 * The new name will be checked for validity.
 * See `git_tag_create()` for rules about valid names.
 *
 * No loaded instances of a the remote with the old name will change
 * their name or their list of refspecs.
 *
 * @param problems non-default refspecs cannot be renamed and will be
 * stored here for further processing by the caller. Always free this
 * strarray on successful return.
 * @param repo the repository in which to rename
 * @param name the current name of the remote
 * @param new_name the new name the remote should bear
 * @return 0, GIT_EINVALIDSPEC, GIT_EEXISTS or an error code
 */
GIT_EXTERN(int) git_remote_rename(
	git_strarray *problems,
	git_repository *repo,
	const char *name,
	const char *new_name);

/**
 * Ensure the remote name is well-formed.
 *
 * @param valid output pointer to set with validity of given remote name
 * @param remote_name name to be checked.
 * @return 0 on success or an error code
 */
GIT_EXTERN(int) git_remote_name_is_valid(int *valid, const char *remote_name);

/**
* Delete an existing persisted remote.
*
* All remote-tracking branches and configuration settings
* for the remote will be removed.
*
* @param repo the repository in which to act
* @param name the name of the remote to delete
* @return 0 on success, or an error code.
*/
GIT_EXTERN(int) git_remote_delete(git_repository *repo, const char *name);

/**
 * Retrieve the name of the remote's default branch
 *
 * The default branch of a repository is the branch which HEAD points
 * to. If the remote does not support reporting this information
 * directly, it performs the guess as git does; that is, if there are
 * multiple branches which point to the same commit, the first one is
 * chosen. If the master branch is a candidate, it wins.
 *
 * This function must only be called after connecting.
 *
 * @param out the buffer in which to store the reference name
 * @param remote the remote
 * @return 0, GIT_ENOTFOUND if the remote does not have any references
 * or none of them point to HEAD's commit, or an error message.
 */
GIT_EXTERN(int) git_remote_default_branch(git_buf *out, git_remote *remote);

/** @} */
GIT_END_DECL
#endif
