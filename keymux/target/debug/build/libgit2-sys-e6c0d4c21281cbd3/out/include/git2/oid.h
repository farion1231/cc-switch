/*
 * Copyright (C) the libgit2 contributors. All rights reserved.
 *
 * This file is part of libgit2, distributed under the GNU GPL v2 with
 * a Linking Exception. For full terms see the included COPYING file.
 */
#ifndef INCLUDE_git_oid_h__
#define INCLUDE_git_oid_h__

#include "common.h"
#include "types.h"
#include "experimental.h"

/**
 * @file git2/oid.h
 * @brief Git object id routines
 * @defgroup git_oid Git object id routines
 * @ingroup Git
 * @{
 */
GIT_BEGIN_DECL

/** The type of object id. */
typedef enum {

#ifdef GIT_EXPERIMENTAL_SHA256
	GIT_OID_SHA1 = 1,  /**< SHA1 */
	GIT_OID_SHA256 = 2 /**< SHA256 */
#else
	GIT_OID_SHA1 = 1   /**< SHA1 */
#endif

} git_oid_t;

/*
 * SHA1 is currently the only supported object ID type.
 */

/** SHA1 is currently libgit2's default oid type. */
#define GIT_OID_DEFAULT         GIT_OID_SHA1

/** Size (in bytes) of a raw/binary sha1 oid */
#define GIT_OID_SHA1_SIZE       20
/** Size (in bytes) of a hex formatted sha1 oid */
#define GIT_OID_SHA1_HEXSIZE   (GIT_OID_SHA1_SIZE * 2)

/**
 * The binary representation of the null sha1 object ID.
 */
#ifndef GIT_EXPERIMENTAL_SHA256
# define GIT_OID_SHA1_ZERO   { { 0 } }
#else
# define GIT_OID_SHA1_ZERO   { GIT_OID_SHA1, { 0 } }
#endif

/**
 * The string representation of the null sha1 object ID.
 */
#define GIT_OID_SHA1_HEXZERO   "0000000000000000000000000000000000000000"

/*
 * Experimental SHA256 support is a breaking change to the API.
 * This exists for application compatibility testing.
 */

#ifdef GIT_EXPERIMENTAL_SHA256

/** Size (in bytes) of a raw/binary sha256 oid */
# define GIT_OID_SHA256_SIZE     32
/** Size (in bytes) of a hex formatted sha256 oid */
# define GIT_OID_SHA256_HEXSIZE (GIT_OID_SHA256_SIZE * 2)

/**
 * The binary representation of the null sha256 object ID.
 */
# define GIT_OID_SHA256_ZERO { GIT_OID_SHA256, { 0 } }

/**
 * The string representation of the null sha256 object ID.
 */
# define GIT_OID_SHA256_HEXZERO "0000000000000000000000000000000000000000000000000000000000000000"

#endif

/* Maximum possible object ID size in raw / hex string format. */
#ifndef GIT_EXPERIMENTAL_SHA256
# define GIT_OID_MAX_SIZE        GIT_OID_SHA1_SIZE
# define GIT_OID_MAX_HEXSIZE     GIT_OID_SHA1_HEXSIZE
#else
# define GIT_OID_MAX_SIZE        GIT_OID_SHA256_SIZE
# define GIT_OID_MAX_HEXSIZE     GIT_OID_SHA256_HEXSIZE
#endif

/** Minimum length (in number of hex characters,
 * i.e. packets of 4 bits) of an oid prefix */
#define GIT_OID_MINPREFIXLEN 4

/** Unique identity of any object (commit, tree, blob, tag). */
typedef struct git_oid {

#ifdef GIT_EXPERIMENTAL_SHA256
	/** type of object id */
	unsigned char type;
#endif

	/** raw binary formatted id */
	unsigned char id[GIT_OID_MAX_SIZE];
} git_oid;

/**
 * Parse a hex formatted object id into a git_oid.
 *
 * The appropriate number of bytes for the given object ID type will
 * be read from the string - 40 bytes for SHA1, 64 bytes for SHA256.
 * The given string need not be NUL terminated.
 *
 * @param out oid structure the result is written into.
 * @param str input hex string; must be pointing at the start of
 *		the hex sequence and have at least the number of bytes
 *		needed for an oid encoded in hex (40 bytes for sha1,
 *		256 bytes for sha256).
 * @param type the type of object id
 * @return 0 or an error code
 */
#ifdef GIT_EXPERIMENTAL_SHA256
GIT_EXTERN(int) git_oid_fromstr(git_oid *out, const char *str, git_oid_t type);
#else
GIT_EXTERN(int) git_oid_fromstr(git_oid *out, const char *str);
#endif

/**
 * Parse a hex formatted NUL-terminated string into a git_oid.
 *
 * @param out oid structure the result is written into.
 * @param str input hex string; must be null-terminated.
 * @param type the type of object id
 * @return 0 or an error code
 */
#ifdef GIT_EXPERIMENTAL_SHA256
GIT_EXTERN(int) git_oid_fromstrp(git_oid *out, const char *str, git_oid_t type);
#else
GIT_EXTERN(int) git_oid_fromstrp(git_oid *out, const char *str);
#endif

/**
 * Parse N characters of a hex formatted object id into a git_oid.
 *
 * If N is odd, the last byte's high nibble will be read in and the
 * low nibble set to zero.
 *
 * @param out oid structure the result is written into.
 * @param str input hex string of at least size `length`
 * @param length length of the input string
 * @param type the type of object id
 * @return 0 or an error code
 */
#ifdef GIT_EXPERIMENTAL_SHA256
GIT_EXTERN(int) git_oid_fromstrn(git_oid *out, const char *str, size_t length, git_oid_t type);
#else
GIT_EXTERN(int) git_oid_fromstrn(git_oid *out, const char *str, size_t length);
#endif

/**
 * Copy an already raw oid into a git_oid structure.
 *
 * @param out oid structure the result is written into.
 * @param raw the raw input bytes to be copied.
 * @return 0 on success or error code
 */
#ifdef GIT_EXPERIMENTAL_SHA256
GIT_EXTERN(int) git_oid_fromraw(git_oid *out, const unsigned char *raw, git_oid_t type);
#else
GIT_EXTERN(int) git_oid_fromraw(git_oid *out, const unsigned char *raw);
#endif

/**
 * Format a git_oid into a hex string.
 *
 * @param out output hex string; must be pointing at the start of
 *		the hex sequence and have at least the number of bytes
 *		needed for an oid encoded in hex (40 bytes for SHA1,
 *		64 bytes for SHA256). Only the oid digits are written;
 *		a '\\0' terminator must be added by the caller if it is
 *		required.
 * @param id oid structure to format.
 * @return 0 on success or error code
 */
GIT_EXTERN(int) git_oid_fmt(char *out, const git_oid *id);

/**
 * Format a git_oid into a partial hex string.
 *
 * @param out output hex string; you say how many bytes to write.
 *		If the number of bytes is > GIT_OID_SHA1_HEXSIZE, extra bytes
 *		will be zeroed; if not, a '\0' terminator is NOT added.
 * @param n number of characters to write into out string
 * @param id oid structure to format.
 * @return 0 on success or error code
 */
GIT_EXTERN(int) git_oid_nfmt(char *out, size_t n, const git_oid *id);

/**
 * Format a git_oid into a loose-object path string.
 *
 * The resulting string is "aa/...", where "aa" is the first two
 * hex digits of the oid and "..." is the remaining 38 digits.
 *
 * @param out output hex string; must be pointing at the start of
 *		the hex sequence and have at least the number of bytes
 *		needed for an oid encoded in hex (41 bytes for SHA1,
 *		65 bytes for SHA256). Only the oid digits are written;
 *		a '\\0' terminator must be added by the caller if it
 *		is required.
 * @param id oid structure to format.
 * @return 0 on success, non-zero callback return value, or error code
 */
GIT_EXTERN(int) git_oid_pathfmt(char *out, const git_oid *id);

/**
 * Format a git_oid into a statically allocated c-string.
 *
 * The c-string is owned by the library and should not be freed
 * by the user. If libgit2 is built with thread support, the string
 * will be stored in TLS (i.e. one buffer per thread) to allow for
 * concurrent calls of the function.
 *
 * @param oid The oid structure to format
 * @return the c-string or NULL on failure
 */
GIT_EXTERN(char *) git_oid_tostr_s(const git_oid *oid);

/**
 * Format a git_oid into a buffer as a hex format c-string.
 *
 * If the buffer is smaller than the size of a hex-formatted oid string
 * plus an additional byte (GIT_OID_SHA_HEXSIZE + 1 for SHA1 or
 * GIT_OID_SHA256_HEXSIZE + 1 for SHA256), then the resulting
 * oid c-string will be truncated to n-1 characters (but will still be
 * NUL-byte terminated).
 *
 * If there are any input parameter errors (out == NULL, n == 0, oid ==
 * NULL), then a pointer to an empty string is returned, so that the
 * return value can always be printed.
 *
 * @param out the buffer into which the oid string is output.
 * @param n the size of the out buffer.
 * @param id the oid structure to format.
 * @return the out buffer pointer, assuming no input parameter
 *			errors, otherwise a pointer to an empty string.
 */
GIT_EXTERN(char *) git_oid_tostr(char *out, size_t n, const git_oid *id);

/**
 * Copy an oid from one structure to another.
 *
 * @param out oid structure the result is written into.
 * @param src oid structure to copy from.
 * @return 0 on success or error code
 */
GIT_EXTERN(int) git_oid_cpy(git_oid *out, const git_oid *src);

/**
 * Compare two oid structures.
 *
 * @param a first oid structure.
 * @param b second oid structure.
 * @return <0, 0, >0 if a < b, a == b, a > b.
 */
GIT_EXTERN(int) git_oid_cmp(const git_oid *a, const git_oid *b);

/**
 * Compare two oid structures for equality
 *
 * @param a first oid structure.
 * @param b second oid structure.
 * @return true if equal, false otherwise
 */
GIT_EXTERN(int) git_oid_equal(const git_oid *a, const git_oid *b);

/**
 * Compare the first 'len' hexadecimal characters (packets of 4 bits)
 * of two oid structures.
 *
 * @param a first oid structure.
 * @param b second oid structure.
 * @param len the number of hex chars to compare
 * @return 0 in case of a match
 */
GIT_EXTERN(int) git_oid_ncmp(const git_oid *a, const git_oid *b, size_t len);

/**
 * Check if an oid equals an hex formatted object id.
 *
 * @param id oid structure.
 * @param str input hex string of an object id.
 * @return 0 in case of a match, -1 otherwise.
 */
GIT_EXTERN(int) git_oid_streq(const git_oid *id, const char *str);

/**
 * Compare an oid to an hex formatted object id.
 *
 * @param id oid structure.
 * @param str input hex string of an object id.
 * @return -1 if str is not valid, <0 if id sorts before str,
 *         0 if id matches str, >0 if id sorts after str.
 */
GIT_EXTERN(int) git_oid_strcmp(const git_oid *id, const char *str);

/**
 * Check is an oid is all zeros.
 *
 * @return 1 if all zeros, 0 otherwise.
 */
GIT_EXTERN(int) git_oid_is_zero(const git_oid *id);

/**
 * OID Shortener object
 */
typedef struct git_oid_shorten git_oid_shorten;

/**
 * Create a new OID shortener.
 *
 * The OID shortener is used to process a list of OIDs
 * in text form and return the shortest length that would
 * uniquely identify all of them.
 *
 * E.g. look at the result of `git log --abbrev`.
 *
 * @param min_length The minimal length for all identifiers,
 *		which will be used even if shorter OIDs would still
 *		be unique.
 *	@return a `git_oid_shorten` instance, NULL if OOM
 */
GIT_EXTERN(git_oid_shorten *) git_oid_shorten_new(size_t min_length);

/**
 * Add a new OID to set of shortened OIDs and calculate
 * the minimal length to uniquely identify all the OIDs in
 * the set.
 *
 * The OID is expected to be a 40-char hexadecimal string.
 * The OID is owned by the user and will not be modified
 * or freed.
 *
 * For performance reasons, there is a hard-limit of how many
 * OIDs can be added to a single set (around ~32000, assuming
 * a mostly randomized distribution), which should be enough
 * for any kind of program, and keeps the algorithm fast and
 * memory-efficient.
 *
 * Attempting to add more than those OIDs will result in a
 * GIT_ERROR_INVALID error
 *
 * @param os a `git_oid_shorten` instance
 * @param text_id an OID in text form
 * @return the minimal length to uniquely identify all OIDs
 *		added so far to the set; or an error code (<0) if an
 *		error occurs.
 */
GIT_EXTERN(int) git_oid_shorten_add(git_oid_shorten *os, const char *text_id);

/**
 * Free an OID shortener instance
 *
 * @param os a `git_oid_shorten` instance
 */
GIT_EXTERN(void) git_oid_shorten_free(git_oid_shorten *os);

/** @} */
GIT_END_DECL
#endif
