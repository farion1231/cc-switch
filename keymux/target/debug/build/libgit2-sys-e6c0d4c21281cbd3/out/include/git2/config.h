/*
 * Copyright (C) the libgit2 contributors. All rights reserved.
 *
 * This file is part of libgit2, distributed under the GNU GPL v2 with
 * a Linking Exception. For full terms see the included COPYING file.
 */
#ifndef INCLUDE_git_config_h__
#define INCLUDE_git_config_h__

#include "common.h"
#include "types.h"
#include "buffer.h"

/**
 * @file git2/config.h
 * @brief Git config management routines
 * @defgroup git_config Git config management routines
 * @ingroup Git
 * @{
 */
GIT_BEGIN_DECL

/**
 * Priority level of a config file.
 * These priority levels correspond to the natural escalation logic
 * (from higher to lower) when searching for config entries in git.git.
 *
 * git_config_open_default() and git_repository_config() honor those
 * priority levels as well.
 */
typedef enum {
	/** System-wide on Windows, for compatibility with portable git */
	GIT_CONFIG_LEVEL_PROGRAMDATA = 1,

	/** System-wide configuration file; /etc/gitconfig on Linux systems */
	GIT_CONFIG_LEVEL_SYSTEM = 2,

	/** XDG compatible configuration file; typically ~/.config/git/config */
	GIT_CONFIG_LEVEL_XDG = 3,

	/** User-specific configuration file (also called Global configuration
	 * file); typically ~/.gitconfig
	 */
	GIT_CONFIG_LEVEL_GLOBAL = 4,

	/** Repository specific configuration file; $WORK_DIR/.git/config on
	 * non-bare repos
	 */
	GIT_CONFIG_LEVEL_LOCAL = 5,

	/** Application specific configuration file; freely defined by applications
	 */
	GIT_CONFIG_LEVEL_APP = 6,

	/** Represents the highest level available config file (i.e. the most
	 * specific config file available that actually is loaded)
	 */
	GIT_CONFIG_HIGHEST_LEVEL = -1
} git_config_level_t;

/**
 * An entry in a configuration file
 */
typedef struct git_config_entry {
	const char *name; /**< Name of the entry (normalised) */
	const char *value; /**< String value of the entry */
	unsigned int include_depth; /**< Depth of includes where this variable was found */
	git_config_level_t level; /**< Which config file this was found in */
	void GIT_CALLBACK(free)(struct git_config_entry *entry); /**< Free function for this entry */
	void *payload; /**< Opaque value for the free function. Do not read or write */
} git_config_entry;

/**
 * Free a config entry
 *
 * @param entry The entry to free.
 */
GIT_EXTERN(void) git_config_entry_free(git_config_entry *entry);

/**
 * A config enumeration callback
 *
 * @param entry the entry currently being enumerated
 * @param payload a user-specified pointer
 * @return non-zero to terminate the iteration.
 */
typedef int GIT_CALLBACK(git_config_foreach_cb)(const git_config_entry *entry, void *payload);

/**
 * An opaque structure for a configuration iterator
 */
typedef struct git_config_iterator git_config_iterator;

/**
 * Config var type
 */
typedef enum {
	GIT_CONFIGMAP_FALSE = 0,
	GIT_CONFIGMAP_TRUE = 1,
	GIT_CONFIGMAP_INT32,
	GIT_CONFIGMAP_STRING
} git_configmap_t;

/**
 * Mapping from config variables to values.
 */
typedef struct {
	git_configmap_t type;
	const char *str_match;
	int map_value;
} git_configmap;

/**
 * Locate the path to the global configuration file
 *
 * The user or global configuration file is usually
 * located in `$HOME/.gitconfig`.
 *
 * This method will try to guess the full path to that
 * file, if the file exists. The returned path
 * may be used on any `git_config` call to load the
 * global configuration file.
 *
 * This method will not guess the path to the xdg compatible
 * config file (`.config/git/config`).
 *
 * @param out Pointer to a user-allocated git_buf in which to store the path
 * @return 0 if a global configuration file has been found. Its path will be stored in `out`.
 */
GIT_EXTERN(int) git_config_find_global(git_buf *out);

/**
 * Locate the path to the global xdg compatible configuration file
 *
 * The xdg compatible configuration file is usually
 * located in `$HOME/.config/git/config`.
 *
 * This method will try to guess the full path to that
 * file, if the file exists. The returned path
 * may be used on any `git_config` call to load the
 * xdg compatible configuration file.
 *
 * @param out Pointer to a user-allocated git_buf in which to store the path
 * @return 0 if a xdg compatible configuration file has been
 *	found. Its path will be stored in `out`.
 */
GIT_EXTERN(int) git_config_find_xdg(git_buf *out);

/**
 * Locate the path to the system configuration file
 *
 * If `/etc/gitconfig` doesn't exist, it will look for
 * `%PROGRAMFILES%\Git\etc\gitconfig`.
 *
 * @param out Pointer to a user-allocated git_buf in which to store the path
 * @return 0 if a system configuration file has been
 *	found. Its path will be stored in `out`.
 */
GIT_EXTERN(int) git_config_find_system(git_buf *out);

/**
 * Locate the path to the configuration file in ProgramData
 *
 * Look for the file in `%PROGRAMDATA%\Git\config` used by portable git.
 *
 * @param out Pointer to a user-allocated git_buf in which to store the path
 * @return 0 if a ProgramData configuration file has been
 *	found. Its path will be stored in `out`.
 */
GIT_EXTERN(int) git_config_find_programdata(git_buf *out);

/**
 * Open the global, XDG and system configuration files
 *
 * Utility wrapper that finds the global, XDG and system configuration files
 * and opens them into a single prioritized config object that can be
 * used when accessing default config data outside a repository.
 *
 * @param out Pointer to store the config instance
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_config_open_default(git_config **out);

/**
 * Allocate a new configuration object
 *
 * This object is empty, so you have to add a file to it before you
 * can do anything with it.
 *
 * @param out pointer to the new configuration
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_config_new(git_config **out);

/**
 * Add an on-disk config file instance to an existing config
 *
 * The on-disk file pointed at by `path` will be opened and
 * parsed; it's expected to be a native Git config file following
 * the default Git config syntax (see man git-config).
 *
 * If the file does not exist, the file will still be added and it
 * will be created the first time we write to it.
 *
 * Note that the configuration object will free the file
 * automatically.
 *
 * Further queries on this config object will access each
 * of the config file instances in order (instances with
 * a higher priority level will be accessed first).
 *
 * @param cfg the configuration to add the file to
 * @param path path to the configuration file to add
 * @param level the priority level of the backend
 * @param force replace config file at the given priority level
 * @param repo optional repository to allow parsing of
 *  conditional includes
 * @return 0 on success, GIT_EEXISTS when adding more than one file
 *  for a given priority level (and force_replace set to 0),
 *  GIT_ENOTFOUND when the file doesn't exist or error code
 */
GIT_EXTERN(int) git_config_add_file_ondisk(
	git_config *cfg,
	const char *path,
	git_config_level_t level,
	const git_repository *repo,
	int force);

/**
 * Create a new config instance containing a single on-disk file
 *
 * This method is a simple utility wrapper for the following sequence
 * of calls:
 *	- git_config_new
 *	- git_config_add_file_ondisk
 *
 * @param out The configuration instance to create
 * @param path Path to the on-disk file to open
 * @return 0 on success, or an error code
 */
GIT_EXTERN(int) git_config_open_ondisk(git_config **out, const char *path);

/**
 * Build a single-level focused config object from a multi-level one.
 *
 * The returned config object can be used to perform get/set/delete operations
 * on a single specific level.
 *
 * Getting several times the same level from the same parent multi-level config
 * will return different config instances, but containing the same config_file
 * instance.
 *
 * @param out The configuration instance to create
 * @param parent Multi-level config to search for the given level
 * @param level Configuration level to search for
 * @return 0, GIT_ENOTFOUND if the passed level cannot be found in the
 * multi-level parent config, or an error code
 */
GIT_EXTERN(int) git_config_open_level(
	git_config **out,
	const git_config *parent,
	git_config_level_t level);

/**
 * Open the global/XDG configuration file according to git's rules
 *
 * Git allows you to store your global configuration at
 * `$HOME/.gitconfig` or `$XDG_CONFIG_HOME/git/config`. For backwards
 * compatibility, the XDG file shouldn't be used unless the use has
 * created it explicitly. With this function you'll open the correct
 * one to write to.
 *
 * @param out pointer in which to store the config object
 * @param config the config object in which to look
 * @return 0 or an error code.
 */
GIT_EXTERN(int) git_config_open_global(git_config **out, git_config *config);

/**
 * Create a snapshot of the configuration
 *
 * Create a snapshot of the current state of a configuration, which
 * allows you to look into a consistent view of the configuration for
 * looking up complex values (e.g. a remote, submodule).
 *
 * The string returned when querying such a config object is valid
 * until it is freed.
 *
 * @param out pointer in which to store the snapshot config object
 * @param config configuration to snapshot
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_config_snapshot(git_config **out, git_config *config);

/**
 * Free the configuration and its associated memory and files
 *
 * @param cfg the configuration to free
 */
GIT_EXTERN(void) git_config_free(git_config *cfg);

/**
 * Get the git_config_entry of a config variable.
 *
 * Free the git_config_entry after use with `git_config_entry_free()`.
 *
 * @param out pointer to the variable git_config_entry
 * @param cfg where to look for the variable
 * @param name the variable's name
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_config_get_entry(
	git_config_entry **out,
	const git_config *cfg,
	const char *name);

/**
 * Get the value of an integer config variable.
 *
 * All config files will be looked into, in the order of their
 * defined level. A higher level means a higher priority. The
 * first occurrence of the variable will be returned here.
 *
 * @param out pointer to the variable where the value should be stored
 * @param cfg where to look for the variable
 * @param name the variable's name
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_config_get_int32(int32_t *out, const git_config *cfg, const char *name);

/**
 * Get the value of a long integer config variable.
 *
 * All config files will be looked into, in the order of their
 * defined level. A higher level means a higher priority. The
 * first occurrence of the variable will be returned here.
 *
 * @param out pointer to the variable where the value should be stored
 * @param cfg where to look for the variable
 * @param name the variable's name
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_config_get_int64(int64_t *out, const git_config *cfg, const char *name);

/**
 * Get the value of a boolean config variable.
 *
 * This function uses the usual C convention of 0 being false and
 * anything else true.
 *
 * All config files will be looked into, in the order of their
 * defined level. A higher level means a higher priority. The
 * first occurrence of the variable will be returned here.
 *
 * @param out pointer to the variable where the value should be stored
 * @param cfg where to look for the variable
 * @param name the variable's name
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_config_get_bool(int *out, const git_config *cfg, const char *name);

/**
 * Get the value of a path config variable.
 *
 * A leading '~' will be expanded to the global search path (which
 * defaults to the user's home directory but can be overridden via
 * `git_libgit2_opts()`.
 *
 * All config files will be looked into, in the order of their
 * defined level. A higher level means a higher priority. The
 * first occurrence of the variable will be returned here.
 *
 * @param out the buffer in which to store the result
 * @param cfg where to look for the variable
 * @param name the variable's name
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_config_get_path(git_buf *out, const git_config *cfg, const char *name);

/**
 * Get the value of a string config variable.
 *
 * This function can only be used on snapshot config objects. The
 * string is owned by the config and should not be freed by the
 * user. The pointer will be valid until the config is freed.
 *
 * All config files will be looked into, in the order of their
 * defined level. A higher level means a higher priority. The
 * first occurrence of the variable will be returned here.
 *
 * @param out pointer to the string
 * @param cfg where to look for the variable
 * @param name the variable's name
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_config_get_string(const char **out, const git_config *cfg, const char *name);

/**
 * Get the value of a string config variable.
 *
 * The value of the config will be copied into the buffer.
 *
 * All config files will be looked into, in the order of their
 * defined level. A higher level means a higher priority. The
 * first occurrence of the variable will be returned here.
 *
 * @param out buffer in which to store the string
 * @param cfg where to look for the variable
 * @param name the variable's name
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_config_get_string_buf(git_buf *out, const git_config *cfg, const char *name);

/**
 * Get each value of a multivar in a foreach callback
 *
 * The callback will be called on each variable found
 *
 * The regular expression is applied case-sensitively on the normalized form of
 * the variable name: the section and variable parts are lower-cased. The
 * subsection is left unchanged.
 *
 * @param cfg where to look for the variable
 * @param name the variable's name
 * @param regexp regular expression to filter which variables we're
 * interested in. Use NULL to indicate all
 * @param callback the function to be called on each value of the variable
 * @param payload opaque pointer to pass to the callback
 * @return 0 or an error code.
 */
GIT_EXTERN(int) git_config_get_multivar_foreach(const git_config *cfg, const char *name, const char *regexp, git_config_foreach_cb callback, void *payload);

/**
 * Get each value of a multivar
 *
 * The regular expression is applied case-sensitively on the normalized form of
 * the variable name: the section and variable parts are lower-cased. The
 * subsection is left unchanged.
 *
 * @param out pointer to store the iterator
 * @param cfg where to look for the variable
 * @param name the variable's name
 * @param regexp regular expression to filter which variables we're
 * interested in. Use NULL to indicate all
 * @return 0 or an error code.
 */
GIT_EXTERN(int) git_config_multivar_iterator_new(git_config_iterator **out, const git_config *cfg, const char *name, const char *regexp);

/**
 * Return the current entry and advance the iterator
 *
 * The pointers returned by this function are valid until the next call
 * to `git_config_next` or until the iterator is freed.
 *
 * @param entry pointer to store the entry
 * @param iter the iterator
 * @return 0 or an error code. GIT_ITEROVER if the iteration has completed
 */
GIT_EXTERN(int) git_config_next(git_config_entry **entry, git_config_iterator *iter);

/**
 * Free a config iterator
 *
 * @param iter the iterator to free
 */
GIT_EXTERN(void) git_config_iterator_free(git_config_iterator *iter);

/**
 * Set the value of an integer config variable in the config file
 * with the highest level (usually the local one).
 *
 * @param cfg where to look for the variable
 * @param name the variable's name
 * @param value Integer value for the variable
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_config_set_int32(git_config *cfg, const char *name, int32_t value);

/**
 * Set the value of a long integer config variable in the config file
 * with the highest level (usually the local one).
 *
 * @param cfg where to look for the variable
 * @param name the variable's name
 * @param value Long integer value for the variable
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_config_set_int64(git_config *cfg, const char *name, int64_t value);

/**
 * Set the value of a boolean config variable in the config file
 * with the highest level (usually the local one).
 *
 * @param cfg where to look for the variable
 * @param name the variable's name
 * @param value the value to store
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_config_set_bool(git_config *cfg, const char *name, int value);

/**
 * Set the value of a string config variable in the config file
 * with the highest level (usually the local one).
 *
 * A copy of the string is made and the user is free to use it
 * afterwards.
 *
 * @param cfg where to look for the variable
 * @param name the variable's name
 * @param value the string to store.
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_config_set_string(git_config *cfg, const char *name, const char *value);

/**
 * Set a multivar in the local config file.
 *
 * The regular expression is applied case-sensitively on the value.
 *
 * @param cfg where to look for the variable
 * @param name the variable's name
 * @param regexp a regular expression to indicate which values to replace
 * @param value the new value.
 * @return 0 or an error code.
 */
GIT_EXTERN(int) git_config_set_multivar(git_config *cfg, const char *name, const char *regexp, const char *value);

/**
 * Delete a config variable from the config file
 * with the highest level (usually the local one).
 *
 * @param cfg the configuration
 * @param name the variable to delete
 * @return 0 or an error code.
 */
GIT_EXTERN(int) git_config_delete_entry(git_config *cfg, const char *name);

/**
 * Deletes one or several entries from a multivar in the local config file.
 *
 * The regular expression is applied case-sensitively on the value.
 *
 * @param cfg where to look for the variables
 * @param name the variable's name
 * @param regexp a regular expression to indicate which values to delete
 *
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_config_delete_multivar(git_config *cfg, const char *name, const char *regexp);

/**
 * Perform an operation on each config variable.
 *
 * The callback receives the normalized name and value of each variable
 * in the config backend, and the data pointer passed to this function.
 * If the callback returns a non-zero value, the function stops iterating
 * and returns that value to the caller.
 *
 * The pointers passed to the callback are only valid as long as the
 * iteration is ongoing.
 *
 * @param cfg where to get the variables from
 * @param callback the function to call on each variable
 * @param payload the data to pass to the callback
 * @return 0 on success, non-zero callback return value, or error code
 */
GIT_EXTERN(int) git_config_foreach(
	const git_config *cfg,
	git_config_foreach_cb callback,
	void *payload);

/**
 * Iterate over all the config variables
 *
 * Use `git_config_next` to advance the iteration and
 * `git_config_iterator_free` when done.
 *
 * @param out pointer to store the iterator
 * @param cfg where to get the variables from
 * @return 0 or an error code.
 */
GIT_EXTERN(int) git_config_iterator_new(git_config_iterator **out, const git_config *cfg);

/**
 * Iterate over all the config variables whose name matches a pattern
 *
 * Use `git_config_next` to advance the iteration and
 * `git_config_iterator_free` when done.
 *
 * The regular expression is applied case-sensitively on the normalized form of
 * the variable name: the section and variable parts are lower-cased. The
 * subsection is left unchanged.
 *
 * @param out pointer to store the iterator
 * @param cfg where to ge the variables from
 * @param regexp regular expression to match the names
 * @return 0 or an error code.
 */
GIT_EXTERN(int) git_config_iterator_glob_new(git_config_iterator **out, const git_config *cfg, const char *regexp);

/**
 * Perform an operation on each config variable matching a regular expression.
 *
 * This behaves like `git_config_foreach` with an additional filter of a
 * regular expression that filters which config keys are passed to the
 * callback.
 *
 * The regular expression is applied case-sensitively on the normalized form of
 * the variable name: the section and variable parts are lower-cased. The
 * subsection is left unchanged.
 *
 * The regular expression is applied case-sensitively on the normalized form of
 * the variable name: the case-insensitive parts are lower-case.
 *
 * @param cfg where to get the variables from
 * @param regexp regular expression to match against config names
 * @param callback the function to call on each variable
 * @param payload the data to pass to the callback
 * @return 0 or the return value of the callback which didn't return 0
 */
GIT_EXTERN(int) git_config_foreach_match(
	const git_config *cfg,
	const char *regexp,
	git_config_foreach_cb callback,
	void *payload);

/**
 * Query the value of a config variable and return it mapped to
 * an integer constant.
 *
 * This is a helper method to easily map different possible values
 * to a variable to integer constants that easily identify them.
 *
 * A mapping array looks as follows:
 *
 *	git_configmap autocrlf_mapping[] = {
 *		{GIT_CVAR_FALSE, NULL, GIT_AUTO_CRLF_FALSE},
 *		{GIT_CVAR_TRUE, NULL, GIT_AUTO_CRLF_TRUE},
 *		{GIT_CVAR_STRING, "input", GIT_AUTO_CRLF_INPUT},
 *		{GIT_CVAR_STRING, "default", GIT_AUTO_CRLF_DEFAULT}};
 *
 * On any "false" value for the variable (e.g. "false", "FALSE", "no"), the
 * mapping will store `GIT_AUTO_CRLF_FALSE` in the `out` parameter.
 *
 * The same thing applies for any "true" value such as "true", "yes" or "1", storing
 * the `GIT_AUTO_CRLF_TRUE` variable.
 *
 * Otherwise, if the value matches the string "input" (with case insensitive comparison),
 * the given constant will be stored in `out`, and likewise for "default".
 *
 * If not a single match can be made to store in `out`, an error code will be
 * returned.
 *
 * @param out place to store the result of the mapping
 * @param cfg config file to get the variables from
 * @param name name of the config variable to lookup
 * @param maps array of `git_configmap` objects specifying the possible mappings
 * @param map_n number of mapping objects in `maps`
 * @return 0 on success, error code otherwise
 */
GIT_EXTERN(int) git_config_get_mapped(
	int *out,
	const git_config *cfg,
	const char *name,
	const git_configmap *maps,
	size_t map_n);

/**
 * Maps a string value to an integer constant
 *
 * @param out place to store the result of the parsing
 * @param maps array of `git_configmap` objects specifying the possible mappings
 * @param map_n number of mapping objects in `maps`
 * @param value value to parse
 * @return 0 or an error code.
 */
GIT_EXTERN(int) git_config_lookup_map_value(
	int *out,
	const git_configmap *maps,
	size_t map_n,
	const char *value);

/**
 * Parse a string value as a bool.
 *
 * Valid values for true are: 'true', 'yes', 'on', 1 or any
 *  number different from 0
 * Valid values for false are: 'false', 'no', 'off', 0
 *
 * @param out place to store the result of the parsing
 * @param value value to parse
 * @return 0 or an error code.
 */
GIT_EXTERN(int) git_config_parse_bool(int *out, const char *value);

/**
 * Parse a string value as an int32.
 *
 * An optional value suffix of 'k', 'm', or 'g' will
 * cause the value to be multiplied by 1024, 1048576,
 * or 1073741824 prior to output.
 *
 * @param out place to store the result of the parsing
 * @param value value to parse
 * @return 0 or an error code.
 */
GIT_EXTERN(int) git_config_parse_int32(int32_t *out, const char *value);

/**
 * Parse a string value as an int64.
 *
 * An optional value suffix of 'k', 'm', or 'g' will
 * cause the value to be multiplied by 1024, 1048576,
 * or 1073741824 prior to output.
 *
 * @param out place to store the result of the parsing
 * @param value value to parse
 * @return 0 or an error code.
 */
GIT_EXTERN(int) git_config_parse_int64(int64_t *out, const char *value);

/**
 * Parse a string value as a path.
 *
 * A leading '~' will be expanded to the global search path (which
 * defaults to the user's home directory but can be overridden via
 * `git_libgit2_opts()`.
 *
 * If the value does not begin with a tilde, the input will be
 * returned.
 *
 * @param out placae to store the result of parsing
 * @param value the path to evaluate
 * @return 0 or an error code.
 */
GIT_EXTERN(int) git_config_parse_path(git_buf *out, const char *value);

/**
 * Perform an operation on each config variable in a given config backend,
 * matching a regular expression.
 *
 * This behaves like `git_config_foreach_match` except that only config
 * entries from the given backend entry are enumerated.
 *
 * The regular expression is applied case-sensitively on the normalized form of
 * the variable name: the section and variable parts are lower-cased. The
 * subsection is left unchanged.
 *
 * @param backend where to get the variables from
 * @param regexp regular expression to match against config names (can be NULL)
 * @param callback the function to call on each variable
 * @param payload the data to pass to the callback
 * @return 0 or an error code.
 */
GIT_EXTERN(int) git_config_backend_foreach_match(
	git_config_backend *backend,
	const char *regexp,
	git_config_foreach_cb callback,
	void *payload);


/**
 * Lock the backend with the highest priority
 *
 * Locking disallows anybody else from writing to that backend. Any
 * updates made after locking will not be visible to a reader until
 * the file is unlocked.
 *
 * You can apply the changes by calling `git_transaction_commit()`
 * before freeing the transaction. Either of these actions will unlock
 * the config.
 *
 * @param tx the resulting transaction, use this to commit or undo the
 * changes
 * @param cfg the configuration in which to lock
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_config_lock(git_transaction **tx, git_config *cfg);

/** @} */
GIT_END_DECL
#endif
