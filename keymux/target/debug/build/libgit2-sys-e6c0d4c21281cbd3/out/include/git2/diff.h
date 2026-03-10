/*
 * Copyright (C) the libgit2 contributors. All rights reserved.
 *
 * This file is part of libgit2, distributed under the GNU GPL v2 with
 * a Linking Exception. For full terms see the included COPYING file.
 */
#ifndef INCLUDE_git_diff_h__
#define INCLUDE_git_diff_h__

#include "common.h"
#include "types.h"
#include "oid.h"
#include "tree.h"
#include "refs.h"

/**
 * @file git2/diff.h
 * @brief Git tree and file differencing routines.
 * @ingroup Git
 * @{
 */
GIT_BEGIN_DECL

/**
 * Flags for diff options.  A combination of these flags can be passed
 * in via the `flags` value in the `git_diff_options`.
 */
typedef enum {
	/** Normal diff, the default */
	GIT_DIFF_NORMAL = 0,

	/*
	 * Options controlling which files will be in the diff
	 */

	/** Reverse the sides of the diff */
	GIT_DIFF_REVERSE = (1u << 0),

	/** Include ignored files in the diff */
	GIT_DIFF_INCLUDE_IGNORED = (1u << 1),

	/** Even with GIT_DIFF_INCLUDE_IGNORED, an entire ignored directory
	 *  will be marked with only a single entry in the diff; this flag
	 *  adds all files under the directory as IGNORED entries, too.
	 */
	GIT_DIFF_RECURSE_IGNORED_DIRS = (1u << 2),

	/** Include untracked files in the diff */
	GIT_DIFF_INCLUDE_UNTRACKED = (1u << 3),

	/** Even with GIT_DIFF_INCLUDE_UNTRACKED, an entire untracked
	 *  directory will be marked with only a single entry in the diff
	 *  (a la what core Git does in `git status`); this flag adds *all*
	 *  files under untracked directories as UNTRACKED entries, too.
	 */
	GIT_DIFF_RECURSE_UNTRACKED_DIRS = (1u << 4),

	/** Include unmodified files in the diff */
	GIT_DIFF_INCLUDE_UNMODIFIED = (1u << 5),

	/** Normally, a type change between files will be converted into a
	 *  DELETED record for the old and an ADDED record for the new; this
	 *  options enabled the generation of TYPECHANGE delta records.
	 */
	GIT_DIFF_INCLUDE_TYPECHANGE = (1u << 6),

	/** Even with GIT_DIFF_INCLUDE_TYPECHANGE, blob->tree changes still
	 *  generally show as a DELETED blob.  This flag tries to correctly
	 *  label blob->tree transitions as TYPECHANGE records with new_file's
	 *  mode set to tree.  Note: the tree SHA will not be available.
	 */
	GIT_DIFF_INCLUDE_TYPECHANGE_TREES = (1u << 7),

	/** Ignore file mode changes */
	GIT_DIFF_IGNORE_FILEMODE = (1u << 8),

	/** Treat all submodules as unmodified */
	GIT_DIFF_IGNORE_SUBMODULES = (1u << 9),

	/** Use case insensitive filename comparisons */
	GIT_DIFF_IGNORE_CASE = (1u << 10),

	/** May be combined with `GIT_DIFF_IGNORE_CASE` to specify that a file
	 *  that has changed case will be returned as an add/delete pair.
	 */
	GIT_DIFF_INCLUDE_CASECHANGE = (1u << 11),

	/** If the pathspec is set in the diff options, this flags indicates
	 *  that the paths will be treated as literal paths instead of
	 *  fnmatch patterns.  Each path in the list must either be a full
	 *  path to a file or a directory.  (A trailing slash indicates that
	 *  the path will _only_ match a directory).  If a directory is
	 *  specified, all children will be included.
	 */
	GIT_DIFF_DISABLE_PATHSPEC_MATCH = (1u << 12),

	/** Disable updating of the `binary` flag in delta records.  This is
	 *  useful when iterating over a diff if you don't need hunk and data
	 *  callbacks and want to avoid having to load file completely.
	 */
	GIT_DIFF_SKIP_BINARY_CHECK = (1u << 13),

	/** When diff finds an untracked directory, to match the behavior of
	 *  core Git, it scans the contents for IGNORED and UNTRACKED files.
	 *  If *all* contents are IGNORED, then the directory is IGNORED; if
	 *  any contents are not IGNORED, then the directory is UNTRACKED.
	 *  This is extra work that may not matter in many cases.  This flag
	 *  turns off that scan and immediately labels an untracked directory
	 *  as UNTRACKED (changing the behavior to not match core Git).
	 */
	GIT_DIFF_ENABLE_FAST_UNTRACKED_DIRS = (1u << 14),

	/** When diff finds a file in the working directory with stat
	 * information different from the index, but the OID ends up being the
	 * same, write the correct stat information into the index.  Note:
	 * without this flag, diff will always leave the index untouched.
	 */
	GIT_DIFF_UPDATE_INDEX = (1u << 15),

	/** Include unreadable files in the diff */
	GIT_DIFF_INCLUDE_UNREADABLE = (1u << 16),

	/** Include unreadable files in the diff */
	GIT_DIFF_INCLUDE_UNREADABLE_AS_UNTRACKED = (1u << 17),

	/*
	 * Options controlling how output will be generated
	 */

	/** Use a heuristic that takes indentation and whitespace into account
	 * which generally can produce better diffs when dealing with ambiguous
	 * diff hunks.
	 */
	GIT_DIFF_INDENT_HEURISTIC = (1u << 18),

	/** Ignore blank lines */
	GIT_DIFF_IGNORE_BLANK_LINES = (1u << 19),

	/** Treat all files as text, disabling binary attributes & detection */
	GIT_DIFF_FORCE_TEXT = (1u << 20),
	/** Treat all files as binary, disabling text diffs */
	GIT_DIFF_FORCE_BINARY = (1u << 21),

	/** Ignore all whitespace */
	GIT_DIFF_IGNORE_WHITESPACE = (1u << 22),
	/** Ignore changes in amount of whitespace */
	GIT_DIFF_IGNORE_WHITESPACE_CHANGE = (1u << 23),
	/** Ignore whitespace at end of line */
	GIT_DIFF_IGNORE_WHITESPACE_EOL = (1u << 24),

	/** When generating patch text, include the content of untracked
	 *  files.  This automatically turns on GIT_DIFF_INCLUDE_UNTRACKED but
	 *  it does not turn on GIT_DIFF_RECURSE_UNTRACKED_DIRS.  Add that
	 *  flag if you want the content of every single UNTRACKED file.
	 */
	GIT_DIFF_SHOW_UNTRACKED_CONTENT = (1u << 25),

	/** When generating output, include the names of unmodified files if
	 *  they are included in the git_diff.  Normally these are skipped in
	 *  the formats that list files (e.g. name-only, name-status, raw).
	 *  Even with this, these will not be included in patch format.
	 */
	GIT_DIFF_SHOW_UNMODIFIED = (1u << 26),

	/** Use the "patience diff" algorithm */
	GIT_DIFF_PATIENCE = (1u << 28),
	/** Take extra time to find minimal diff */
	GIT_DIFF_MINIMAL = (1u << 29),

	/** Include the necessary deflate / delta information so that `git-apply`
	 *  can apply given diff information to binary files.
	 */
	GIT_DIFF_SHOW_BINARY = (1u << 30)
} git_diff_option_t;

/**
 * The diff object that contains all individual file deltas.
 *
 * A `diff` represents the cumulative list of differences between two
 * snapshots of a repository (possibly filtered by a set of file name
 * patterns).
 *
 * Calculating diffs is generally done in two phases: building a list of
 * diffs then traversing it. This makes is easier to share logic across
 * the various types of diffs (tree vs tree, workdir vs index, etc.), and
 * also allows you to insert optional diff post-processing phases,
 * such as rename detection, in between the steps. When you are done with
 * a diff object, it must be freed.
 *
 * This is an opaque structure which will be allocated by one of the diff
 * generator functions below (such as `git_diff_tree_to_tree`). You are
 * responsible for releasing the object memory when done, using the
 * `git_diff_free()` function.
 *
 */
typedef struct git_diff git_diff;

/**
 * Flags for the delta object and the file objects on each side.
 *
 * These flags are used for both the `flags` value of the `git_diff_delta`
 * and the flags for the `git_diff_file` objects representing the old and
 * new sides of the delta.  Values outside of this public range should be
 * considered reserved for internal or future use.
 */
typedef enum {
	GIT_DIFF_FLAG_BINARY     = (1u << 0), /**< file(s) treated as binary data */
	GIT_DIFF_FLAG_NOT_BINARY = (1u << 1), /**< file(s) treated as text data */
	GIT_DIFF_FLAG_VALID_ID   = (1u << 2), /**< `id` value is known correct */
	GIT_DIFF_FLAG_EXISTS     = (1u << 3), /**< file exists at this side of the delta */
	GIT_DIFF_FLAG_VALID_SIZE = (1u << 4)  /**< file size value is known correct */
} git_diff_flag_t;

/**
 * What type of change is described by a git_diff_delta?
 *
 * `GIT_DELTA_RENAMED` and `GIT_DELTA_COPIED` will only show up if you run
 * `git_diff_find_similar()` on the diff object.
 *
 * `GIT_DELTA_TYPECHANGE` only shows up given `GIT_DIFF_INCLUDE_TYPECHANGE`
 * in the option flags (otherwise type changes will be split into ADDED /
 * DELETED pairs).
 */
typedef enum {
	GIT_DELTA_UNMODIFIED = 0,  /**< no changes */
	GIT_DELTA_ADDED = 1,	   /**< entry does not exist in old version */
	GIT_DELTA_DELETED = 2,	   /**< entry does not exist in new version */
	GIT_DELTA_MODIFIED = 3,    /**< entry content changed between old and new */
	GIT_DELTA_RENAMED = 4,     /**< entry was renamed between old and new */
	GIT_DELTA_COPIED = 5,      /**< entry was copied from another old entry */
	GIT_DELTA_IGNORED = 6,     /**< entry is ignored item in workdir */
	GIT_DELTA_UNTRACKED = 7,   /**< entry is untracked item in workdir */
	GIT_DELTA_TYPECHANGE = 8,  /**< type of entry changed between old and new */
	GIT_DELTA_UNREADABLE = 9,  /**< entry is unreadable */
	GIT_DELTA_CONFLICTED = 10  /**< entry in the index is conflicted */
} git_delta_t;

/**
 * Description of one side of a delta.
 *
 * Although this is called a "file", it could represent a file, a symbolic
 * link, a submodule commit id, or even a tree (although that only if you
 * are tracking type changes or ignored/untracked directories).
 */
typedef struct {
	/**
	 * The `git_oid` of the item.  If the entry represents an
	 * absent side of a diff (e.g. the `old_file` of a `GIT_DELTA_ADDED` delta),
	 * then the oid will be zeroes.
	 */
	git_oid            id;

	/**
	 * The NUL-terminated path to the entry relative to the working
	 * directory of the repository.
	 */
	const char        *path;

	/**
	 * The size of the entry in bytes.
	 */
	git_object_size_t  size;

	/**
	 * A combination of the `git_diff_flag_t` types
	 */
	uint32_t           flags;

	/**
	 * Roughly, the stat() `st_mode` value for the item.  This will
	 * be restricted to one of the `git_filemode_t` values.
	 */
	uint16_t           mode;

	/**
	 * Represents the known length of the `id` field, when
	 * converted to a hex string.  It is generally `GIT_OID_SHA1_HEXSIZE`, unless this
	 * delta was created from reading a patch file, in which case it may be
	 * abbreviated to something reasonable, like 7 characters.
	 */
	uint16_t           id_abbrev;
} git_diff_file;

/**
 * Description of changes to one entry.
 *
 * A `delta` is a file pair with an old and new revision.  The old version
 * may be absent if the file was just created and the new version may be
 * absent if the file was deleted.  A diff is mostly just a list of deltas.
 *
 * When iterating over a diff, this will be passed to most callbacks and
 * you can use the contents to understand exactly what has changed.
 *
 * The `old_file` represents the "from" side of the diff and the `new_file`
 * represents to "to" side of the diff.  What those means depend on the
 * function that was used to generate the diff and will be documented below.
 * You can also use the `GIT_DIFF_REVERSE` flag to flip it around.
 *
 * Although the two sides of the delta are named "old_file" and "new_file",
 * they actually may correspond to entries that represent a file, a symbolic
 * link, a submodule commit id, or even a tree (if you are tracking type
 * changes or ignored/untracked directories).
 *
 * Under some circumstances, in the name of efficiency, not all fields will
 * be filled in, but we generally try to fill in as much as possible.  One
 * example is that the "flags" field may not have either the `BINARY` or the
 * `NOT_BINARY` flag set to avoid examining file contents if you do not pass
 * in hunk and/or line callbacks to the diff foreach iteration function.  It
 * will just use the git attributes for those files.
 *
 * The similarity score is zero unless you call `git_diff_find_similar()`
 * which does a similarity analysis of files in the diff.  Use that
 * function to do rename and copy detection, and to split heavily modified
 * files in add/delete pairs.  After that call, deltas with a status of
 * GIT_DELTA_RENAMED or GIT_DELTA_COPIED will have a similarity score
 * between 0 and 100 indicating how similar the old and new sides are.
 *
 * If you ask `git_diff_find_similar` to find heavily modified files to
 * break, but to not *actually* break the records, then GIT_DELTA_MODIFIED
 * records may have a non-zero similarity score if the self-similarity is
 * below the split threshold.  To display this value like core Git, invert
 * the score (a la `printf("M%03d", 100 - delta->similarity)`).
 */
typedef struct {
	git_delta_t   status;
	uint32_t      flags;	   /**< git_diff_flag_t values */
	uint16_t      similarity;  /**< for RENAMED and COPIED, value 0-100 */
	uint16_t      nfiles;	   /**< number of files in this delta */
	git_diff_file old_file;
	git_diff_file new_file;
} git_diff_delta;

/**
 * Diff notification callback function.
 *
 * The callback will be called for each file, just before the `git_diff_delta`
 * gets inserted into the diff.
 *
 * When the callback:
 * - returns < 0, the diff process will be aborted.
 * - returns > 0, the delta will not be inserted into the diff, but the
 *		diff process continues.
 * - returns 0, the delta is inserted into the diff, and the diff process
 *		continues.
 */
typedef int GIT_CALLBACK(git_diff_notify_cb)(
	const git_diff *diff_so_far,
	const git_diff_delta *delta_to_add,
	const char *matched_pathspec,
	void *payload);

/**
 * Diff progress callback.
 *
 * Called before each file comparison.
 *
 * @param diff_so_far The diff being generated.
 * @param old_path The path to the old file or NULL.
 * @param new_path The path to the new file or NULL.
 * @return Non-zero to abort the diff.
 */
typedef int GIT_CALLBACK(git_diff_progress_cb)(
	const git_diff *diff_so_far,
	const char *old_path,
	const char *new_path,
	void *payload);

/**
 * Structure describing options about how the diff should be executed.
 *
 * Setting all values of the structure to zero will yield the default
 * values.  Similarly, passing NULL for the options structure will
 * give the defaults.  The default values are marked below.
 *
 */
typedef struct {
	unsigned int version;      /**< version for the struct */

	/**
	 * A combination of `git_diff_option_t` values above.
	 * Defaults to GIT_DIFF_NORMAL
	 */
	uint32_t flags;

	/* options controlling which files are in the diff */

	/** Overrides the submodule ignore setting for all submodules in the diff. */
	git_submodule_ignore_t ignore_submodules;

	/**
	 * An array of paths / fnmatch patterns to constrain diff.
	 * All paths are included by default.
	 */
	git_strarray       pathspec;

	/**
	 * An optional callback function, notifying the consumer of changes to
	 * the diff as new deltas are added.
	 */
	git_diff_notify_cb   notify_cb;

	/**
	 * An optional callback function, notifying the consumer of which files
	 * are being examined as the diff is generated.
	 */
	git_diff_progress_cb progress_cb;

	/** The payload to pass to the callback functions. */
	void                *payload;

	/* options controlling how to diff text is generated */

	/**
	 * The number of unchanged lines that define the boundary of a hunk
	 * (and to display before and after). Defaults to 3.
	 */
	uint32_t    context_lines;
	/**
	 * The maximum number of unchanged lines between hunk boundaries before
	 * the hunks will be merged into one. Defaults to 0.
	 */
	uint32_t    interhunk_lines;

	/**
	 * The object ID type to emit in diffs; this is used by functions
	 * that operate without a repository - namely `git_diff_buffers`,
	 * or `git_diff_blobs` and `git_diff_blob_to_buffer` when one blob
	 * is `NULL`.
	 *
	 * This may be omitted (set to `0`). If a repository is available,
	 * the object ID format of the repository will be used. If no
	 * repository is available then the default is `GIT_OID_SHA`.
	 *
	 * If this is specified and a repository is available, then the
	 * specified `oid_type` must match the repository's object ID
	 * format.
	 */
	git_oid_t   oid_type;

	/**
	 * The abbreviation length to use when formatting object ids.
	 * Defaults to the value of 'core.abbrev' from the config, or 7 if unset.
	 */
	uint16_t    id_abbrev;

	/**
	 * A size (in bytes) above which a blob will be marked as binary
	 * automatically; pass a negative value to disable.
	 * Defaults to 512MB.
	 */
	git_off_t   max_size;

	/**
	 * The virtual "directory" prefix for old file names in hunk headers.
	 * Default is "a".
	 */
	const char *old_prefix;

	/**
	 * The virtual "directory" prefix for new file names in hunk headers.
	 * Defaults to "b".
	 */
	const char *new_prefix;
} git_diff_options;

/* The current version of the diff options structure */
#define GIT_DIFF_OPTIONS_VERSION 1

/* Stack initializer for diff options.  Alternatively use
 * `git_diff_options_init` programmatic initialization.
 */
#define GIT_DIFF_OPTIONS_INIT \
	{GIT_DIFF_OPTIONS_VERSION, 0, GIT_SUBMODULE_IGNORE_UNSPECIFIED, {NULL,0}, NULL, NULL, NULL, 3}

/**
 * Initialize git_diff_options structure
 *
 * Initializes a `git_diff_options` with default values. Equivalent to creating
 * an instance with GIT_DIFF_OPTIONS_INIT.
 *
 * @param opts The `git_diff_options` struct to initialize.
 * @param version The struct version; pass `GIT_DIFF_OPTIONS_VERSION`.
 * @return Zero on success; -1 on failure.
 */
GIT_EXTERN(int) git_diff_options_init(
	git_diff_options *opts,
	unsigned int version);

/**
 * When iterating over a diff, callback that will be made per file.
 *
 * @param delta A pointer to the delta data for the file
 * @param progress Goes from 0 to 1 over the diff
 * @param payload User-specified pointer from foreach function
 */
typedef int GIT_CALLBACK(git_diff_file_cb)(
	const git_diff_delta *delta,
	float progress,
	void *payload);

#define GIT_DIFF_HUNK_HEADER_SIZE	128

/**
 * When producing a binary diff, the binary data returned will be
 * either the deflated full ("literal") contents of the file, or
 * the deflated binary delta between the two sides (whichever is
 * smaller).
 */
typedef enum {
	/** There is no binary delta. */
	GIT_DIFF_BINARY_NONE,

	/** The binary data is the literal contents of the file. */
	GIT_DIFF_BINARY_LITERAL,

	/** The binary data is the delta from one side to the other. */
	GIT_DIFF_BINARY_DELTA
} git_diff_binary_t;

/** The contents of one of the files in a binary diff. */
typedef struct {
	/** The type of binary data for this file. */
	git_diff_binary_t type;

	/** The binary data, deflated. */
	const char *data;

	/** The length of the binary data. */
	size_t datalen;

	/** The length of the binary data after inflation. */
	size_t inflatedlen;
} git_diff_binary_file;

/**
 * Structure describing the binary contents of a diff.
 *
 * A `binary` file / delta is a file (or pair) for which no text diffs
 * should be generated. A diff can contain delta entries that are
 * binary, but no diff content will be output for those files. There is
 * a base heuristic for binary detection and you can further tune the
 * behavior with git attributes or diff flags and option settings.
 */
typedef struct {
	/**
	 * Whether there is data in this binary structure or not.
	 *
	 * If this is `1`, then this was produced and included binary content.
	 * If this is `0` then this was generated knowing only that a binary
	 * file changed but without providing the data, probably from a patch
	 * that said `Binary files a/file.txt and b/file.txt differ`.
	 */
	unsigned int contains_data;
	git_diff_binary_file old_file; /**< The contents of the old file. */
	git_diff_binary_file new_file; /**< The contents of the new file. */
} git_diff_binary;

/**
 * When iterating over a diff, callback that will be made for
 * binary content within the diff.
 */
typedef int GIT_CALLBACK(git_diff_binary_cb)(
	const git_diff_delta *delta,
	const git_diff_binary *binary,
	void *payload);

/**
 * Structure describing a hunk of a diff.
 *
 * A `hunk` is a span of modified lines in a delta along with some stable
 * surrounding context. You can configure the amount of context and other
 * properties of how hunks are generated. Each hunk also comes with a
 * header that described where it starts and ends in both the old and new
 * versions in the delta.
 */
typedef struct {
	int    old_start;     /**< Starting line number in old_file */
	int    old_lines;     /**< Number of lines in old_file */
	int    new_start;     /**< Starting line number in new_file */
	int    new_lines;     /**< Number of lines in new_file */
	size_t header_len;    /**< Number of bytes in header text */
	char   header[GIT_DIFF_HUNK_HEADER_SIZE];   /**< Header text, NUL-byte terminated */
} git_diff_hunk;

/**
 * When iterating over a diff, callback that will be made per hunk.
 */
typedef int GIT_CALLBACK(git_diff_hunk_cb)(
	const git_diff_delta *delta,
	const git_diff_hunk *hunk,
	void *payload);

/**
 * Line origin constants.
 *
 * These values describe where a line came from and will be passed to
 * the git_diff_line_cb when iterating over a diff.  There are some
 * special origin constants at the end that are used for the text
 * output callbacks to demarcate lines that are actually part of
 * the file or hunk headers.
 */
typedef enum {
	/* These values will be sent to `git_diff_line_cb` along with the line */
	GIT_DIFF_LINE_CONTEXT   = ' ',
	GIT_DIFF_LINE_ADDITION  = '+',
	GIT_DIFF_LINE_DELETION  = '-',

	GIT_DIFF_LINE_CONTEXT_EOFNL = '=', /**< Both files have no LF at end */
	GIT_DIFF_LINE_ADD_EOFNL = '>',     /**< Old has no LF at end, new does */
	GIT_DIFF_LINE_DEL_EOFNL = '<',     /**< Old has LF at end, new does not */

	/* The following values will only be sent to a `git_diff_line_cb` when
	 * the content of a diff is being formatted through `git_diff_print`.
	 */
	GIT_DIFF_LINE_FILE_HDR  = 'F',
	GIT_DIFF_LINE_HUNK_HDR  = 'H',
	GIT_DIFF_LINE_BINARY    = 'B' /**< For "Binary files x and y differ" */
} git_diff_line_t;

/**
 * Structure describing a line (or data span) of a diff.
 *
 * A `line` is a range of characters inside a hunk.  It could be a context
 * line (i.e. in both old and new versions), an added line (i.e. only in
 * the new version), or a removed line (i.e. only in the old version).
 * Unfortunately, we don't know anything about the encoding of data in the
 * file being diffed, so we cannot tell you much about the line content.
 * Line data will not be NUL-byte terminated, however, because it will be
 * just a span of bytes inside the larger file.
 */
typedef struct {
	char   origin;       /**< A git_diff_line_t value */
	int    old_lineno;   /**< Line number in old file or -1 for added line */
	int    new_lineno;   /**< Line number in new file or -1 for deleted line */
	int    num_lines;    /**< Number of newline characters in content */
	size_t content_len;  /**< Number of bytes of data */
	git_off_t content_offset; /**< Offset in the original file to the content */
	const char *content; /**< Pointer to diff text, not NUL-byte terminated */
} git_diff_line;

/**
 * When iterating over a diff, callback that will be made per text diff
 * line. In this context, the provided range will be NULL.
 *
 * When printing a diff, callback that will be made to output each line
 * of text.  This uses some extra GIT_DIFF_LINE_... constants for output
 * of lines of file and hunk headers.
 */
typedef int GIT_CALLBACK(git_diff_line_cb)(
	const git_diff_delta *delta, /**< delta that contains this data */
	const git_diff_hunk *hunk,   /**< hunk containing this data */
	const git_diff_line *line,   /**< line data */
	void *payload);              /**< user reference data */

/**
 * Flags to control the behavior of diff rename/copy detection.
 */
typedef enum {
	/** Obey `diff.renames`. Overridden by any other GIT_DIFF_FIND_... flag. */
	GIT_DIFF_FIND_BY_CONFIG = 0,

	/** Look for renames? (`--find-renames`) */
	GIT_DIFF_FIND_RENAMES = (1u << 0),

	/** Consider old side of MODIFIED for renames? (`--break-rewrites=N`) */
	GIT_DIFF_FIND_RENAMES_FROM_REWRITES = (1u << 1),

	/** Look for copies? (a la `--find-copies`). */
	GIT_DIFF_FIND_COPIES = (1u << 2),

	/** Consider UNMODIFIED as copy sources? (`--find-copies-harder`).
	 *
	 * For this to work correctly, use GIT_DIFF_INCLUDE_UNMODIFIED when
	 * the initial `git_diff` is being generated.
	 */
	GIT_DIFF_FIND_COPIES_FROM_UNMODIFIED = (1u << 3),

	/** Mark significant rewrites for split (`--break-rewrites=/M`) */
	GIT_DIFF_FIND_REWRITES = (1u << 4),
	/** Actually split large rewrites into delete/add pairs */
	GIT_DIFF_BREAK_REWRITES = (1u << 5),
	/** Mark rewrites for split and break into delete/add pairs */
	GIT_DIFF_FIND_AND_BREAK_REWRITES =
		(GIT_DIFF_FIND_REWRITES | GIT_DIFF_BREAK_REWRITES),

	/** Find renames/copies for UNTRACKED items in working directory.
	 *
	 * For this to work correctly, use GIT_DIFF_INCLUDE_UNTRACKED when the
	 * initial `git_diff` is being generated (and obviously the diff must
	 * be against the working directory for this to make sense).
	 */
	GIT_DIFF_FIND_FOR_UNTRACKED = (1u << 6),

	/** Turn on all finding features. */
	GIT_DIFF_FIND_ALL = (0x0ff),

	/** Measure similarity ignoring leading whitespace (default) */
	GIT_DIFF_FIND_IGNORE_LEADING_WHITESPACE = 0,
	/** Measure similarity ignoring all whitespace */
	GIT_DIFF_FIND_IGNORE_WHITESPACE = (1u << 12),
	/** Measure similarity including all data */
	GIT_DIFF_FIND_DONT_IGNORE_WHITESPACE = (1u << 13),
	/** Measure similarity only by comparing SHAs (fast and cheap) */
	GIT_DIFF_FIND_EXACT_MATCH_ONLY = (1u << 14),

	/** Do not break rewrites unless they contribute to a rename.
	 *
	 * Normally, GIT_DIFF_FIND_AND_BREAK_REWRITES will measure the self-
	 * similarity of modified files and split the ones that have changed a
	 * lot into a DELETE / ADD pair.  Then the sides of that pair will be
	 * considered candidates for rename and copy detection.
	 *
	 * If you add this flag in and the split pair is *not* used for an
	 * actual rename or copy, then the modified record will be restored to
	 * a regular MODIFIED record instead of being split.
	 */
	GIT_DIFF_BREAK_REWRITES_FOR_RENAMES_ONLY  = (1u << 15),

	/** Remove any UNMODIFIED deltas after find_similar is done.
	 *
	 * Using GIT_DIFF_FIND_COPIES_FROM_UNMODIFIED to emulate the
	 * --find-copies-harder behavior requires building a diff with the
	 * GIT_DIFF_INCLUDE_UNMODIFIED flag.  If you do not want UNMODIFIED
	 * records in the final result, pass this flag to have them removed.
	 */
	GIT_DIFF_FIND_REMOVE_UNMODIFIED = (1u << 16)
} git_diff_find_t;

/**
 * Pluggable similarity metric
 */
typedef struct {
	int GIT_CALLBACK(file_signature)(
		void **out, const git_diff_file *file,
		const char *fullpath, void *payload);
	int GIT_CALLBACK(buffer_signature)(
		void **out, const git_diff_file *file,
		const char *buf, size_t buflen, void *payload);
	void GIT_CALLBACK(free_signature)(void *sig, void *payload);
	int GIT_CALLBACK(similarity)(int *score, void *siga, void *sigb, void *payload);
	void *payload;
} git_diff_similarity_metric;

/**
 * Control behavior of rename and copy detection
 *
 * These options mostly mimic parameters that can be passed to git-diff.
 */
typedef struct {
	unsigned int version;

	/**
	 * Combination of git_diff_find_t values (default GIT_DIFF_FIND_BY_CONFIG).
	 * NOTE: if you don't explicitly set this, `diff.renames` could be set
	 * to false, resulting in `git_diff_find_similar` doing nothing.
	 */
	uint32_t flags;

	/**
	 * Threshold above which similar files will be considered renames.
	 * This is equivalent to the -M option. Defaults to 50.
	 */
	uint16_t rename_threshold;

	/**
	 * Threshold below which similar files will be eligible to be a rename source.
	 * This is equivalent to the first part of the -B option. Defaults to 50.
	 */
	uint16_t rename_from_rewrite_threshold;

	/**
	 * Threshold above which similar files will be considered copies.
	 * This is equivalent to the -C option. Defaults to 50.
	 */
	uint16_t copy_threshold;

	/**
	 * Threshold below which similar files will be split into a delete/add pair.
	 * This is equivalent to the last part of the -B option. Defaults to 60.
	 */
	uint16_t break_rewrite_threshold;

	/**
	 * Maximum number of matches to consider for a particular file.
	 *
	 * This is a little different from the `-l` option from Git because we
	 * will still process up to this many matches before abandoning the search.
	 * Defaults to 1000.
	 */
	size_t rename_limit;

	/**
	 * The `metric` option allows you to plug in a custom similarity metric.
	 *
	 * Set it to NULL to use the default internal metric.
	 *
	 * The default metric is based on sampling hashes of ranges of data in
	 * the file, which is a pretty good similarity approximation that should
	 * work fairly well for both text and binary data while still being
	 * pretty fast with a fixed memory overhead.
	 */
	git_diff_similarity_metric *metric;
} git_diff_find_options;

#define GIT_DIFF_FIND_OPTIONS_VERSION 1
#define GIT_DIFF_FIND_OPTIONS_INIT {GIT_DIFF_FIND_OPTIONS_VERSION}

/**
 * Initialize git_diff_find_options structure
 *
 * Initializes a `git_diff_find_options` with default values. Equivalent to creating
 * an instance with GIT_DIFF_FIND_OPTIONS_INIT.
 *
 * @param opts The `git_diff_find_options` struct to initialize.
 * @param version The struct version; pass `GIT_DIFF_FIND_OPTIONS_VERSION`.
 * @return Zero on success; -1 on failure.
 */
GIT_EXTERN(int) git_diff_find_options_init(
	git_diff_find_options *opts,
	unsigned int version);

/** @name Diff Generator Functions
 *
 * These are the functions you would use to create (or destroy) a
 * git_diff from various objects in a repository.
 */
/**@{*/

/**
 * Deallocate a diff.
 *
 * @param diff The previously created diff; cannot be used after free.
 */
GIT_EXTERN(void) git_diff_free(git_diff *diff);

/**
 * Create a diff with the difference between two tree objects.
 *
 * This is equivalent to `git diff <old-tree> <new-tree>`
 *
 * The first tree will be used for the "old_file" side of the delta and the
 * second tree will be used for the "new_file" side of the delta.  You can
 * pass NULL to indicate an empty tree, although it is an error to pass
 * NULL for both the `old_tree` and `new_tree`.
 *
 * @param diff Output pointer to a git_diff pointer to be allocated.
 * @param repo The repository containing the trees.
 * @param old_tree A git_tree object to diff from, or NULL for empty tree.
 * @param new_tree A git_tree object to diff to, or NULL for empty tree.
 * @param opts Structure with options to influence diff or NULL for defaults.
 * @return 0 or an error code.
 */
GIT_EXTERN(int) git_diff_tree_to_tree(
	git_diff **diff,
	git_repository *repo,
	git_tree *old_tree,
	git_tree *new_tree,
	const git_diff_options *opts);

/**
 * Create a diff between a tree and repository index.
 *
 * This is equivalent to `git diff --cached <treeish>` or if you pass
 * the HEAD tree, then like `git diff --cached`.
 *
 * The tree you pass will be used for the "old_file" side of the delta, and
 * the index will be used for the "new_file" side of the delta.
 *
 * If you pass NULL for the index, then the existing index of the `repo`
 * will be used.  In this case, the index will be refreshed from disk
 * (if it has changed) before the diff is generated.
 *
 * @param diff Output pointer to a git_diff pointer to be allocated.
 * @param repo The repository containing the tree and index.
 * @param old_tree A git_tree object to diff from, or NULL for empty tree.
 * @param index The index to diff with; repo index used if NULL.
 * @param opts Structure with options to influence diff or NULL for defaults.
 * @return 0 or an error code.
 */
GIT_EXTERN(int) git_diff_tree_to_index(
	git_diff **diff,
	git_repository *repo,
	git_tree *old_tree,
	git_index *index,
	const git_diff_options *opts);

/**
 * Create a diff between the repository index and the workdir directory.
 *
 * This matches the `git diff` command.  See the note below on
 * `git_diff_tree_to_workdir` for a discussion of the difference between
 * `git diff` and `git diff HEAD` and how to emulate a `git diff <treeish>`
 * using libgit2.
 *
 * The index will be used for the "old_file" side of the delta, and the
 * working directory will be used for the "new_file" side of the delta.
 *
 * If you pass NULL for the index, then the existing index of the `repo`
 * will be used.  In this case, the index will be refreshed from disk
 * (if it has changed) before the diff is generated.
 *
 * @param diff Output pointer to a git_diff pointer to be allocated.
 * @param repo The repository.
 * @param index The index to diff from; repo index used if NULL.
 * @param opts Structure with options to influence diff or NULL for defaults.
 * @return 0 or an error code.
 */
GIT_EXTERN(int) git_diff_index_to_workdir(
	git_diff **diff,
	git_repository *repo,
	git_index *index,
	const git_diff_options *opts);

/**
 * Create a diff between a tree and the working directory.
 *
 * The tree you provide will be used for the "old_file" side of the delta,
 * and the working directory will be used for the "new_file" side.
 *
 * This is not the same as `git diff <treeish>` or `git diff-index
 * <treeish>`.  Those commands use information from the index, whereas this
 * function strictly returns the differences between the tree and the files
 * in the working directory, regardless of the state of the index.  Use
 * `git_diff_tree_to_workdir_with_index` to emulate those commands.
 *
 * To see difference between this and `git_diff_tree_to_workdir_with_index`,
 * consider the example of a staged file deletion where the file has then
 * been put back into the working dir and further modified.  The
 * tree-to-workdir diff for that file is 'modified', but `git diff` would
 * show status 'deleted' since there is a staged delete.
 *
 * @param diff A pointer to a git_diff pointer that will be allocated.
 * @param repo The repository containing the tree.
 * @param old_tree A git_tree object to diff from, or NULL for empty tree.
 * @param opts Structure with options to influence diff or NULL for defaults.
 * @return 0 or an error code.
 */
GIT_EXTERN(int) git_diff_tree_to_workdir(
	git_diff **diff,
	git_repository *repo,
	git_tree *old_tree,
	const git_diff_options *opts);

/**
 * Create a diff between a tree and the working directory using index data
 * to account for staged deletes, tracked files, etc.
 *
 * This emulates `git diff <tree>` by diffing the tree to the index and
 * the index to the working directory and blending the results into a
 * single diff that includes staged deleted, etc.
 *
 * @param diff A pointer to a git_diff pointer that will be allocated.
 * @param repo The repository containing the tree.
 * @param old_tree A git_tree object to diff from, or NULL for empty tree.
 * @param opts Structure with options to influence diff or NULL for defaults.
 * @return 0 or an error code.
 */
GIT_EXTERN(int) git_diff_tree_to_workdir_with_index(
	git_diff **diff,
	git_repository *repo,
	git_tree *old_tree,
	const git_diff_options *opts);

/**
 * Create a diff with the difference between two index objects.
 *
 * The first index will be used for the "old_file" side of the delta and the
 * second index will be used for the "new_file" side of the delta.
 *
 * @param diff Output pointer to a git_diff pointer to be allocated.
 * @param repo The repository containing the indexes.
 * @param old_index A git_index object to diff from.
 * @param new_index A git_index object to diff to.
 * @param opts Structure with options to influence diff or NULL for defaults.
 * @return 0 or an error code.
 */
GIT_EXTERN(int) git_diff_index_to_index(
	git_diff **diff,
	git_repository *repo,
	git_index *old_index,
	git_index *new_index,
	const git_diff_options *opts);

/**
 * Merge one diff into another.
 *
 * This merges items from the "from" list into the "onto" list.  The
 * resulting diff will have all items that appear in either list.
 * If an item appears in both lists, then it will be "merged" to appear
 * as if the old version was from the "onto" list and the new version
 * is from the "from" list (with the exception that if the item has a
 * pending DELETE in the middle, then it will show as deleted).
 *
 * @param onto Diff to merge into.
 * @param from Diff to merge.
 * @return 0 or an error code.
 */
GIT_EXTERN(int) git_diff_merge(
	git_diff *onto,
	const git_diff *from);

/**
 * Transform a diff marking file renames, copies, etc.
 *
 * This modifies a diff in place, replacing old entries that look
 * like renames or copies with new entries reflecting those changes.
 * This also will, if requested, break modified files into add/remove
 * pairs if the amount of change is above a threshold.
 *
 * @param diff diff to run detection algorithms on
 * @param options Control how detection should be run, NULL for defaults
 * @return 0 on success, -1 on failure
 */
GIT_EXTERN(int) git_diff_find_similar(
	git_diff *diff,
	const git_diff_find_options *options);

/**@}*/


/** @name Diff Processor Functions
 *
 * These are the functions you apply to a diff to process it
 * or read it in some way.
 */
/**@{*/

/**
 * Query how many diff records are there in a diff.
 *
 * @param diff A git_diff generated by one of the above functions
 * @return Count of number of deltas in the list
 */
GIT_EXTERN(size_t) git_diff_num_deltas(const git_diff *diff);

/**
 * Query how many diff deltas are there in a diff filtered by type.
 *
 * This works just like `git_diff_num_deltas()` with an extra parameter
 * that is a `git_delta_t` and returns just the count of how many deltas
 * match that particular type.
 *
 * @param diff A git_diff generated by one of the above functions
 * @param type A git_delta_t value to filter the count
 * @return Count of number of deltas matching delta_t type
 */
GIT_EXTERN(size_t) git_diff_num_deltas_of_type(
	const git_diff *diff, git_delta_t type);

/**
 * Return the diff delta for an entry in the diff list.
 *
 * The `git_diff_delta` pointer points to internal data and you do not
 * have to release it when you are done with it.  It will go away when
 * the * `git_diff` (or any associated `git_patch`) goes away.
 *
 * Note that the flags on the delta related to whether it has binary
 * content or not may not be set if there are no attributes set for the
 * file and there has been no reason to load the file data at this point.
 * For now, if you need those flags to be up to date, your only option is
 * to either use `git_diff_foreach` or create a `git_patch`.
 *
 * @param diff Diff list object
 * @param idx Index into diff list
 * @return Pointer to git_diff_delta (or NULL if `idx` out of range)
 */
GIT_EXTERN(const git_diff_delta *) git_diff_get_delta(
	const git_diff *diff, size_t idx);

/**
 * Check if deltas are sorted case sensitively or insensitively.
 *
 * @param diff diff to check
 * @return 0 if case sensitive, 1 if case is ignored
 */
GIT_EXTERN(int) git_diff_is_sorted_icase(const git_diff *diff);

/**
 * Loop over all deltas in a diff issuing callbacks.
 *
 * This will iterate through all of the files described in a diff.  You
 * should provide a file callback to learn about each file.
 *
 * The "hunk" and "line" callbacks are optional, and the text diff of the
 * files will only be calculated if they are not NULL.  Of course, these
 * callbacks will not be invoked for binary files on the diff or for
 * files whose only changed is a file mode change.
 *
 * Returning a non-zero value from any of the callbacks will terminate
 * the iteration and return the value to the user.
 *
 * @param diff A git_diff generated by one of the above functions.
 * @param file_cb Callback function to make per file in the diff.
 * @param binary_cb Optional callback to make for binary files.
 * @param hunk_cb Optional callback to make per hunk of text diff.  This
 *                callback is called to describe a range of lines in the
 *                diff.  It will not be issued for binary files.
 * @param line_cb Optional callback to make per line of diff text.  This
 *                same callback will be made for context lines, added, and
 *                removed lines, and even for a deleted trailing newline.
 * @param payload Reference pointer that will be passed to your callbacks.
 * @return 0 on success, non-zero callback return value, or error code
 */
GIT_EXTERN(int) git_diff_foreach(
	git_diff *diff,
	git_diff_file_cb file_cb,
	git_diff_binary_cb binary_cb,
	git_diff_hunk_cb hunk_cb,
	git_diff_line_cb line_cb,
	void *payload);

/**
 * Look up the single character abbreviation for a delta status code.
 *
 * When you run `git diff --name-status` it uses single letter codes in
 * the output such as 'A' for added, 'D' for deleted, 'M' for modified,
 * etc.  This function converts a git_delta_t value into these letters for
 * your own purposes.  GIT_DELTA_UNTRACKED will return a space (i.e. ' ').
 *
 * @param status The git_delta_t value to look up
 * @return The single character label for that code
 */
GIT_EXTERN(char) git_diff_status_char(git_delta_t status);

/**
 * Possible output formats for diff data
 */
typedef enum {
	GIT_DIFF_FORMAT_PATCH        = 1u, /**< full git diff */
	GIT_DIFF_FORMAT_PATCH_HEADER = 2u, /**< just the file headers of patch */
	GIT_DIFF_FORMAT_RAW          = 3u, /**< like git diff --raw */
	GIT_DIFF_FORMAT_NAME_ONLY    = 4u, /**< like git diff --name-only */
	GIT_DIFF_FORMAT_NAME_STATUS  = 5u, /**< like git diff --name-status */
	GIT_DIFF_FORMAT_PATCH_ID     = 6u  /**< git diff as used by git patch-id */
} git_diff_format_t;

/**
 * Iterate over a diff generating formatted text output.
 *
 * Returning a non-zero value from the callbacks will terminate the
 * iteration and return the non-zero value to the caller.
 *
 * @param diff A git_diff generated by one of the above functions.
 * @param format A git_diff_format_t value to pick the text format.
 * @param print_cb Callback to make per line of diff text.
 * @param payload Reference pointer that will be passed to your callback.
 * @return 0 on success, non-zero callback return value, or error code
 */
GIT_EXTERN(int) git_diff_print(
	git_diff *diff,
	git_diff_format_t format,
	git_diff_line_cb print_cb,
	void *payload);

/**
 * Produce the complete formatted text output from a diff into a
 * buffer.
 *
 * @param out A pointer to a user-allocated git_buf that will
 *            contain the diff text
 * @param diff A git_diff generated by one of the above functions.
 * @param format A git_diff_format_t value to pick the text format.
 * @return 0 on success or error code
 */
GIT_EXTERN(int) git_diff_to_buf(
	git_buf *out,
	git_diff *diff,
	git_diff_format_t format);

/**@}*/

/*
 * Low-level file comparison, invoking callbacks per difference.
 */

/**
 * Directly run a diff on two blobs.
 *
 * Compared to a file, a blob lacks some contextual information. As such,
 * the `git_diff_file` given to the callback will have some fake data; i.e.
 * `mode` will be 0 and `path` will be NULL.
 *
 * NULL is allowed for either `old_blob` or `new_blob` and will be treated
 * as an empty blob, with the `oid` set to NULL in the `git_diff_file` data.
 * Passing NULL for both blobs is a noop; no callbacks will be made at all.
 *
 * We do run a binary content check on the blob content and if either blob
 * looks like binary data, the `git_diff_delta` binary attribute will be set
 * to 1 and no call to the hunk_cb nor line_cb will be made (unless you pass
 * `GIT_DIFF_FORCE_TEXT` of course).
 *
 * @param old_blob Blob for old side of diff, or NULL for empty blob
 * @param old_as_path Treat old blob as if it had this filename; can be NULL
 * @param new_blob Blob for new side of diff, or NULL for empty blob
 * @param new_as_path Treat new blob as if it had this filename; can be NULL
 * @param options Options for diff, or NULL for default options
 * @param file_cb Callback for "file"; made once if there is a diff; can be NULL
 * @param binary_cb Callback for binary files; can be NULL
 * @param hunk_cb Callback for each hunk in diff; can be NULL
 * @param line_cb Callback for each line in diff; can be NULL
 * @param payload Payload passed to each callback function
 * @return 0 on success, non-zero callback return value, or error code
 */
GIT_EXTERN(int) git_diff_blobs(
	const git_blob *old_blob,
	const char *old_as_path,
	const git_blob *new_blob,
	const char *new_as_path,
	const git_diff_options *options,
	git_diff_file_cb file_cb,
	git_diff_binary_cb binary_cb,
	git_diff_hunk_cb hunk_cb,
	git_diff_line_cb line_cb,
	void *payload);

/**
 * Directly run a diff between a blob and a buffer.
 *
 * As with `git_diff_blobs`, comparing a blob and buffer lacks some context,
 * so the `git_diff_file` parameters to the callbacks will be faked a la the
 * rules for `git_diff_blobs()`.
 *
 * Passing NULL for `old_blob` will be treated as an empty blob (i.e. the
 * `file_cb` will be invoked with GIT_DELTA_ADDED and the diff will be the
 * entire content of the buffer added).  Passing NULL to the buffer will do
 * the reverse, with GIT_DELTA_REMOVED and blob content removed.
 *
 * @param old_blob Blob for old side of diff, or NULL for empty blob
 * @param old_as_path Treat old blob as if it had this filename; can be NULL
 * @param buffer Raw data for new side of diff, or NULL for empty
 * @param buffer_len Length of raw data for new side of diff
 * @param buffer_as_path Treat buffer as if it had this filename; can be NULL
 * @param options Options for diff, or NULL for default options
 * @param file_cb Callback for "file"; made once if there is a diff; can be NULL
 * @param binary_cb Callback for binary files; can be NULL
 * @param hunk_cb Callback for each hunk in diff; can be NULL
 * @param line_cb Callback for each line in diff; can be NULL
 * @param payload Payload passed to each callback function
 * @return 0 on success, non-zero callback return value, or error code
 */
GIT_EXTERN(int) git_diff_blob_to_buffer(
	const git_blob *old_blob,
	const char *old_as_path,
	const char *buffer,
	size_t buffer_len,
	const char *buffer_as_path,
	const git_diff_options *options,
	git_diff_file_cb file_cb,
	git_diff_binary_cb binary_cb,
	git_diff_hunk_cb hunk_cb,
	git_diff_line_cb line_cb,
	void *payload);

/**
 * Directly run a diff between two buffers.
 *
 * Even more than with `git_diff_blobs`, comparing two buffer lacks
 * context, so the `git_diff_file` parameters to the callbacks will be
 * faked a la the rules for `git_diff_blobs()`.
 *
 * @param old_buffer Raw data for old side of diff, or NULL for empty
 * @param old_len Length of the raw data for old side of the diff
 * @param old_as_path Treat old buffer as if it had this filename; can be NULL
 * @param new_buffer Raw data for new side of diff, or NULL for empty
 * @param new_len Length of raw data for new side of diff
 * @param new_as_path Treat buffer as if it had this filename; can be NULL
 * @param options Options for diff, or NULL for default options
 * @param file_cb Callback for "file"; made once if there is a diff; can be NULL
 * @param binary_cb Callback for binary files; can be NULL
 * @param hunk_cb Callback for each hunk in diff; can be NULL
 * @param line_cb Callback for each line in diff; can be NULL
 * @param payload Payload passed to each callback function
 * @return 0 on success, non-zero callback return value, or error code
 */
GIT_EXTERN(int) git_diff_buffers(
	const void *old_buffer,
	size_t old_len,
	const char *old_as_path,
	const void *new_buffer,
	size_t new_len,
	const char *new_as_path,
	const git_diff_options *options,
	git_diff_file_cb file_cb,
	git_diff_binary_cb binary_cb,
	git_diff_hunk_cb hunk_cb,
	git_diff_line_cb line_cb,
	void *payload);

/* Patch file parsing. */

/**
 * Options for parsing a diff / patch file.
 */
typedef struct {
	unsigned int version;
	git_oid_t oid_type;
} git_diff_parse_options;

/* The current version of the diff parse options structure */
#define GIT_DIFF_PARSE_OPTIONS_VERSION 1

/* Stack initializer for diff parse options.  Alternatively use
 * `git_diff_parse_options_init` programmatic initialization.
 */
#define GIT_DIFF_PARSE_OPTIONS_INIT \
	{ GIT_DIFF_PARSE_OPTIONS_VERSION, GIT_OID_DEFAULT }

/**
 * Read the contents of a git patch file into a `git_diff` object.
 *
 * The diff object produced is similar to the one that would be
 * produced if you actually produced it computationally by comparing
 * two trees, however there may be subtle differences.  For example,
 * a patch file likely contains abbreviated object IDs, so the
 * object IDs in a `git_diff_delta` produced by this function will
 * also be abbreviated.
 *
 * This function will only read patch files created by a git
 * implementation, it will not read unified diffs produced by
 * the `diff` program, nor any other types of patch files.
 *
 * @param out A pointer to a git_diff pointer that will be allocated.
 * @param content The contents of a patch file
 * @param content_len The length of the patch file contents
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_diff_from_buffer(
	git_diff **out,
	const char *content,
	size_t content_len
#ifdef GIT_EXPERIMENTAL_SHA256
	, git_diff_parse_options *opts
#endif
	);

/**
 * This is an opaque structure which is allocated by `git_diff_get_stats`.
 * You are responsible for releasing the object memory when done, using the
 * `git_diff_stats_free()` function.
 */
typedef struct git_diff_stats git_diff_stats;

/**
 * Formatting options for diff stats
 */
typedef enum {
	/** No stats*/
	GIT_DIFF_STATS_NONE = 0,

	/** Full statistics, equivalent of `--stat` */
	GIT_DIFF_STATS_FULL = (1u << 0),

	/** Short statistics, equivalent of `--shortstat` */
	GIT_DIFF_STATS_SHORT = (1u << 1),

	/** Number statistics, equivalent of `--numstat` */
	GIT_DIFF_STATS_NUMBER = (1u << 2),

	/** Extended header information such as creations, renames and mode changes, equivalent of `--summary` */
	GIT_DIFF_STATS_INCLUDE_SUMMARY = (1u << 3)
} git_diff_stats_format_t;

/**
 * Accumulate diff statistics for all patches.
 *
 * @param out Structure containing the diff statistics.
 * @param diff A git_diff generated by one of the above functions.
 * @return 0 on success; non-zero on error
 */
GIT_EXTERN(int) git_diff_get_stats(
	git_diff_stats **out,
	git_diff *diff);

/**
 * Get the total number of files changed in a diff
 *
 * @param stats A `git_diff_stats` generated by one of the above functions.
 * @return total number of files changed in the diff
 */
GIT_EXTERN(size_t) git_diff_stats_files_changed(
	const git_diff_stats *stats);

/**
 * Get the total number of insertions in a diff
 *
 * @param stats A `git_diff_stats` generated by one of the above functions.
 * @return total number of insertions in the diff
 */
GIT_EXTERN(size_t) git_diff_stats_insertions(
	const git_diff_stats *stats);

/**
 * Get the total number of deletions in a diff
 *
 * @param stats A `git_diff_stats` generated by one of the above functions.
 * @return total number of deletions in the diff
 */
GIT_EXTERN(size_t) git_diff_stats_deletions(
	const git_diff_stats *stats);

/**
 * Print diff statistics to a `git_buf`.
 *
 * @param out buffer to store the formatted diff statistics in.
 * @param stats A `git_diff_stats` generated by one of the above functions.
 * @param format Formatting option.
 * @param width Target width for output (only affects GIT_DIFF_STATS_FULL)
 * @return 0 on success; non-zero on error
 */
GIT_EXTERN(int) git_diff_stats_to_buf(
	git_buf *out,
	const git_diff_stats *stats,
	git_diff_stats_format_t format,
	size_t width);

/**
 * Deallocate a `git_diff_stats`.
 *
 * @param stats The previously created statistics object;
 * cannot be used after free.
 */
GIT_EXTERN(void) git_diff_stats_free(git_diff_stats *stats);

/**
 * Patch ID options structure
 *
 * Initialize with `GIT_PATCHID_OPTIONS_INIT`. Alternatively, you can
 * use `git_diff_patchid_options_init`.
 *
 */
typedef struct git_diff_patchid_options {
	unsigned int version;
} git_diff_patchid_options;

#define GIT_DIFF_PATCHID_OPTIONS_VERSION 1
#define GIT_DIFF_PATCHID_OPTIONS_INIT { GIT_DIFF_PATCHID_OPTIONS_VERSION }

/**
 * Initialize git_diff_patchid_options structure
 *
 * Initializes a `git_diff_patchid_options` with default values. Equivalent to
 * creating an instance with `GIT_DIFF_PATCHID_OPTIONS_INIT`.
 *
 * @param opts The `git_diff_patchid_options` struct to initialize.
 * @param version The struct version; pass `GIT_DIFF_PATCHID_OPTIONS_VERSION`.
 * @return Zero on success; -1 on failure.
 */
GIT_EXTERN(int) git_diff_patchid_options_init(
	git_diff_patchid_options *opts,
	unsigned int version);

/**
 * Calculate the patch ID for the given patch.
 *
 * Calculate a stable patch ID for the given patch by summing the
 * hash of the file diffs, ignoring whitespace and line numbers.
 * This can be used to derive whether two diffs are the same with
 * a high probability.
 *
 * Currently, this function only calculates stable patch IDs, as
 * defined in git-patch-id(1), and should in fact generate the
 * same IDs as the upstream git project does.
 *
 * @param out Pointer where the calculated patch ID should be stored
 * @param diff The diff to calculate the ID for
 * @param opts Options for how to calculate the patch ID. This is
 *  intended for future changes, as currently no options are
 *  available.
 * @return 0 on success, an error code otherwise.
 */
GIT_EXTERN(int) git_diff_patchid(git_oid *out, git_diff *diff, git_diff_patchid_options *opts);

GIT_END_DECL

/** @} */

#endif
