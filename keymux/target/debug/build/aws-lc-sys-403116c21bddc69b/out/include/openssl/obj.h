/* Copyright (C) 1995-1998 Eric Young (eay@cryptsoft.com)
 * All rights reserved.
 *
 * This package is an SSL implementation written
 * by Eric Young (eay@cryptsoft.com).
 * The implementation was written so as to conform with Netscapes SSL.
 *
 * This library is free for commercial and non-commercial use as long as
 * the following conditions are aheared to.  The following conditions
 * apply to all code found in this distribution, be it the RC4, RSA,
 * lhash, DES, etc., code; not just the SSL code.  The SSL documentation
 * included with this distribution is covered by the same copyright terms
 * except that the holder is Tim Hudson (tjh@cryptsoft.com).
 *
 * Copyright remains Eric Young's, and as such any Copyright notices in
 * the code are not to be removed.
 * If this package is used in a product, Eric Young should be given attribution
 * as the author of the parts of the library used.
 * This can be in the form of a textual message at program startup or
 * in documentation (online or textual) provided with the package.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *    "This product includes cryptographic software written by
 *     Eric Young (eay@cryptsoft.com)"
 *    The word 'cryptographic' can be left out if the rouines from the library
 *    being used are not cryptographic related :-).
 * 4. If you include any Windows specific code (or a derivative thereof) from
 *    the apps directory (application code) you must include an acknowledgement:
 *    "This product includes software written by Tim Hudson (tjh@cryptsoft.com)"
 *
 * THIS SOFTWARE IS PROVIDED BY ERIC YOUNG ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 * The licence and distribution terms for any publically available version or
 * derivative of this code cannot be changed.  i.e. this code cannot simply be
 * copied and put under another distribution licence
 * [including the GNU Public Licence.] */

#ifndef OPENSSL_HEADER_OBJ_H
#define OPENSSL_HEADER_OBJ_H

#include <openssl/base.h>

#include <openssl/bytestring.h>
#include <openssl/nid.h>  // IWYU pragma: export

#if defined(__cplusplus)
extern "C" {
#endif


// The objects library deals with the registration and indexing of ASN.1 object
// identifiers. These values are often written as a dotted sequence of numbers,
// e.g. 1.2.840.113549.1.9.16.3.9.
//
// Internally, OpenSSL likes to deal with these values by numbering them with
// numbers called "nids". OpenSSL has a large, built-in database of common
// object identifiers and also has both short and long names for them.
//
// This library provides functions for translating between object identifiers,
// nids, short names and long names.
//
// The nid values should not be used outside of a single process: they are not
// stable identifiers.


// Basic operations.

// OBJ_dup returns a duplicate copy of |obj| or NULL on allocation failure. The
// caller must call |ASN1_OBJECT_free| on the result to release it.
OPENSSL_EXPORT ASN1_OBJECT *OBJ_dup(const ASN1_OBJECT *obj);

// OBJ_cmp returns a value less than, equal to or greater than zero if |a| is
// less than, equal to or greater than |b|, respectively.
OPENSSL_EXPORT int OBJ_cmp(const ASN1_OBJECT *a, const ASN1_OBJECT *b);

// OBJ_get0_data returns a pointer to the DER representation of |obj|. This is
// the contents of the DER-encoded identifier, not including the tag and length.
// If |obj| does not have an associated object identifier (i.e. it is a nid-only
// value), this value is the empty string.
OPENSSL_EXPORT const uint8_t *OBJ_get0_data(const ASN1_OBJECT *obj);

// OBJ_length returns the length of the DER representation of |obj|. This is the
// contents of the DER-encoded identifier, not including the tag and length. If
// |obj| does not have an associated object identifier (i.e. it is a nid-only
// value), this value is the empty string.
OPENSSL_EXPORT size_t OBJ_length(const ASN1_OBJECT *obj);


// Looking up nids.

// OBJ_obj2nid returns the nid corresponding to |obj|, or |NID_undef| if no
// such object is known.
OPENSSL_EXPORT int OBJ_obj2nid(const ASN1_OBJECT *obj);

// OBJ_cbs2nid returns the nid corresponding to the DER data in |cbs|, or
// |NID_undef| if no such object is known.
OPENSSL_EXPORT int OBJ_cbs2nid(const CBS *cbs);

// OBJ_sn2nid returns the nid corresponding to |short_name|, or |NID_undef| if
// no such short name is known.
OPENSSL_EXPORT int OBJ_sn2nid(const char *short_name);

// OBJ_ln2nid returns the nid corresponding to |long_name|, or |NID_undef| if
// no such long name is known.
OPENSSL_EXPORT int OBJ_ln2nid(const char *long_name);

// OBJ_txt2nid returns the nid corresponding to |s|, which may be a short name,
// long name, or an ASCII string containing a dotted sequence of numbers. It
// returns the nid or NID_undef if unknown.
OPENSSL_EXPORT int OBJ_txt2nid(const char *s);


// Getting information about nids.

// OBJ_nid2obj returns the |ASN1_OBJECT| corresponding to |nid|, or NULL if
// |nid| is unknown.
//
// Although the output is not const, this function returns a static, immutable
// |ASN1_OBJECT|. It is not necessary to release the object with
// |ASN1_OBJECT_free|.
//
// However, functions like |X509_ALGOR_set0| expect to take ownership of a
// possibly dynamically-allocated |ASN1_OBJECT|. |ASN1_OBJECT_free| is a no-op
// for static |ASN1_OBJECT|s, so |OBJ_nid2obj| is compatible with such
// functions.
//
// Callers are encouraged to store the result of this function in a const
// pointer. However, if using functions like |X509_ALGOR_set0|, callers may use
// a non-const pointer and manage ownership.
OPENSSL_EXPORT ASN1_OBJECT *OBJ_nid2obj(int nid);

// OBJ_get_undef returns the object for |NID_undef|. Prefer this function over
// |OBJ_nid2obj| to avoid pulling in the full OID table.
OPENSSL_EXPORT const ASN1_OBJECT *OBJ_get_undef(void);

// OBJ_nid2sn returns the short name for |nid|, or NULL if |nid| is unknown.
OPENSSL_EXPORT const char *OBJ_nid2sn(int nid);

// OBJ_nid2ln returns the long name for |nid|, or NULL if |nid| is unknown.
OPENSSL_EXPORT const char *OBJ_nid2ln(int nid);

// OBJ_nid2cbb writes |nid| as an ASN.1 OBJECT IDENTIFIER to |out|. It returns
// one on success or zero otherwise.
OPENSSL_EXPORT int OBJ_nid2cbb(CBB *out, int nid);


// Dealing with textual representations of object identifiers.

// OBJ_txt2obj returns an ASN1_OBJECT for the textual representation in |s|.
// If |dont_search_names| is zero, then |s| will be matched against the long
// and short names of a known objects to find a match. Otherwise |s| must
// contain an ASCII string with a dotted sequence of numbers. The resulting
// object need not be previously known. It returns a freshly allocated
// |ASN1_OBJECT| or NULL on error.
OPENSSL_EXPORT ASN1_OBJECT *OBJ_txt2obj(const char *s, int dont_search_names);

// OBJ_obj2txt converts |obj| to a textual representation. If
// |always_return_oid| is zero then |obj| will be matched against known objects
// and the long (preferably) or short name will be used if found. Otherwise
// |obj| will be converted into a dotted sequence of integers. If |out| is not
// NULL, then at most |out_len| bytes of the textual form will be written
// there. If |out_len| is at least one, then string written to |out| will
// always be NUL terminated. It returns the number of characters that could
// have been written, not including the final NUL, or -1 on error.
OPENSSL_EXPORT int OBJ_obj2txt(char *out, int out_len, const ASN1_OBJECT *obj,
                               int always_return_oid);


// Adding objects at runtime.

// OBJ_create adds a known object and returns the NID of the new object, or
// NID_undef on error.
//
// WARNING: This function modifies global state. The table cannot contain
// duplicate OIDs, short names, or long names. If two callers in the same
// address space add conflicting values, only one registration will take effect.
// Avoid this function if possible. Instead, callers can process OIDs unknown to
// BoringSSL by acting on the byte representation directly. See
// |ASN1_OBJECT_create|, |OBJ_get0_data|, and |OBJ_length|.
OPENSSL_EXPORT int OBJ_create(const char *oid, const char *short_name,
                              const char *long_name);


// Handling signature algorithm identifiers.
//
// Some NIDs (e.g. sha256WithRSAEncryption) specify both a digest algorithm and
// a public key algorithm. The following functions map between pairs of digest
// and public-key algorithms and the NIDs that specify their combination.
//
// Sometimes the combination NID leaves the digest unspecified (e.g.
// rsassaPss). In these cases, the digest NID is |NID_undef|.

// OBJ_find_sigid_algs finds the digest and public-key NIDs that correspond to
// the signing algorithm |sign_nid|. If successful, it sets |*out_digest_nid|
// and |*out_pkey_nid| and returns one. Otherwise it returns zero. Any of
// |out_digest_nid| or |out_pkey_nid| can be NULL if the caller doesn't need
// that output value.
OPENSSL_EXPORT int OBJ_find_sigid_algs(int sign_nid, int *out_digest_nid,
                                       int *out_pkey_nid);

// OBJ_find_sigid_by_algs finds the signature NID that corresponds to the
// combination of |digest_nid| and |pkey_nid|. If success, it sets
// |*out_sign_nid| and returns one. Otherwise it returns zero. The
// |out_sign_nid| argument can be NULL if the caller only wishes to learn
// whether the combination is valid.
OPENSSL_EXPORT int OBJ_find_sigid_by_algs(int *out_sign_nid, int digest_nid,
                                          int pkey_nid);


// Deprecated functions.

typedef struct obj_name_st {
  int type;
  int alias;
  const char *name;
  const char *data;
} OBJ_NAME;

#define OBJ_NAME_TYPE_MD_METH 1
#define OBJ_NAME_TYPE_CIPHER_METH 2

// OBJ_NAME_do_all_sorted calls |callback| zero or more times, each time with
// the name of a different primitive. If |type| is |OBJ_NAME_TYPE_MD_METH| then
// the primitives will be hash functions, alternatively if |type| is
// |OBJ_NAME_TYPE_CIPHER_METH| then the primitives will be ciphers or cipher
// modes.
//
// This function is ill-specified and should never be used.
OPENSSL_EXPORT void OBJ_NAME_do_all_sorted(
    int type, void (*callback)(const OBJ_NAME *, void *arg), void *arg);


// OBJ_cleanup does nothing.
OPENSSL_EXPORT void OBJ_cleanup(void);


#if defined(__cplusplus)
}  // extern C
#endif

#define OBJ_R_UNKNOWN_NID 100
#define OBJ_R_INVALID_OID_STRING 101

#endif  // OPENSSL_HEADER_OBJ_H
