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
 * [including the GNU Public Licence.]
 */
/* ====================================================================
 * Copyright 2002 Sun Microsystems, Inc. ALL RIGHTS RESERVED.
 * ECDH support in OpenSSL originally developed by
 * SUN MICROSYSTEMS, INC., and contributed to the OpenSSL project.
 */

#ifndef OPENSSL_HEADER_X509_H
#define OPENSSL_HEADER_X509_H

#include <openssl/base.h>

#include <time.h>

#include <openssl/asn1.h>
#include <openssl/bio.h>
#include <openssl/cipher.h>
#include <openssl/conf.h>
#include <openssl/dh.h>
#include <openssl/dsa.h>
#include <openssl/ec.h>
#include <openssl/ecdh.h>
#include <openssl/ecdsa.h>
#include <openssl/evp.h>
#include <openssl/lhash.h>
#include <openssl/obj.h>
#include <openssl/pkcs7.h>
#include <openssl/pool.h>
#include <openssl/rsa.h>
#include <openssl/sha.h>
#include <openssl/stack.h>
#include <openssl/thread.h>
#include <openssl/x509v3_errors.h>  // IWYU pragma: export

#if defined(__cplusplus)
extern "C" {
#endif


// Legacy X.509 library.
//
// This header is part of OpenSSL's X.509 implementation. It is retained for
// compatibility but should not be used by new code. The functions are difficult
// to use correctly, and have buggy or non-standard behaviors. They are thus
// particularly prone to behavior changes and API removals, as BoringSSL
// iterates on these issues.
//
// In the future, a replacement library will be available. Meanwhile, minimize
// dependencies on this header where possible.


// Certificates.
//
// An |X509| object represents an X.509 certificate, defined in RFC 5280.
//
// Although an |X509| is a mutable object, mutating an |X509| can give incorrect
// results. Callers typically obtain |X509|s by parsing some input with
// |d2i_X509|, etc. Such objects carry information such as the serialized
// TBSCertificate and decoded extensions, which will become inconsistent when
// mutated.
//
// Instead, mutation functions should only be used when issuing new
// certificates, as described in a later section.

DEFINE_STACK_OF(X509)

// X509 is an |ASN1_ITEM| whose ASN.1 type is X.509 Certificate (RFC 5280) and C
// type is |X509*|.
DECLARE_ASN1_ITEM(X509)

// X509_up_ref adds one to the reference count of |x509| and returns one.
OPENSSL_EXPORT int X509_up_ref(X509 *x509);

// X509_chain_up_ref returns a newly-allocated |STACK_OF(X509)| containing a
// shallow copy of |chain|, or NULL on error. That is, the return value has the
// same contents as |chain|, and each |X509|'s reference count is incremented by
// one.
OPENSSL_EXPORT STACK_OF(X509) *X509_chain_up_ref(STACK_OF(X509) *chain);

// X509_dup returns a newly-allocated copy of |x509|, or NULL on error. This
// function works by serializing the structure, so auxiliary properties (see
// |i2d_X509_AUX|) are not preserved. Additionally, if |x509| is incomplete,
// this function may fail.
//
// TODO(https://crbug.com/boringssl/407): This function should be const and
// thread-safe but is currently neither in some cases, notably if |crl| was
// mutated.
OPENSSL_EXPORT X509 *X509_dup(X509 *x509);

// X509_free decrements |x509|'s reference count and, if zero, releases memory
// associated with |x509|.
OPENSSL_EXPORT void X509_free(X509 *x509);

// d2i_X509 parses up to |len| bytes from |*inp| as a DER-encoded X.509
// Certificate (RFC 5280), as described in |d2i_SAMPLE_with_reuse|.
OPENSSL_EXPORT X509 *d2i_X509(X509 **out, const uint8_t **inp, long len);

// X509_parse_from_buffer parses an X.509 structure from |buf| and returns a
// fresh X509 or NULL on error. There must not be any trailing data in |buf|.
// The returned structure (if any) holds a reference to |buf| rather than
// copying parts of it as a normal |d2i_X509| call would do.
OPENSSL_EXPORT X509 *X509_parse_from_buffer(CRYPTO_BUFFER *buf);

// i2d_X509 marshals |x509| as a DER-encoded X.509 Certificate (RFC 5280), as
// described in |i2d_SAMPLE|.
//
// TODO(https://crbug.com/boringssl/407): This function should be const and
// thread-safe but is currently neither in some cases, notably if |x509| was
// mutated.
OPENSSL_EXPORT int i2d_X509(X509 *x509, uint8_t **outp);

// X509_VERSION_* are X.509 version numbers. Note the numerical values of all
// defined X.509 versions are one less than the named version.
#define X509_VERSION_1 0
#define X509_VERSION_2 1
#define X509_VERSION_3 2

// X509_get_version returns the numerical value of |x509|'s version, which will
// be one of the |X509_VERSION_*| constants.
OPENSSL_EXPORT long X509_get_version(const X509 *x509);

// X509_get0_serialNumber returns |x509|'s serial number.
OPENSSL_EXPORT const ASN1_INTEGER *X509_get0_serialNumber(const X509 *x509);

// X509_get0_notBefore returns |x509|'s notBefore time.
OPENSSL_EXPORT const ASN1_TIME *X509_get0_notBefore(const X509 *x509);

// X509_get0_notAfter returns |x509|'s notAfter time.
OPENSSL_EXPORT const ASN1_TIME *X509_get0_notAfter(const X509 *x509);

// X509_get_issuer_name returns |x509|'s issuer.
OPENSSL_EXPORT X509_NAME *X509_get_issuer_name(const X509 *x509);

// X509_get_subject_name returns |x509|'s subject.
OPENSSL_EXPORT X509_NAME *X509_get_subject_name(const X509 *x509);

// X509_get_X509_PUBKEY returns the public key of |x509|. Note this function is
// not const-correct for legacy reasons. Callers should not modify the returned
// object.
OPENSSL_EXPORT X509_PUBKEY *X509_get_X509_PUBKEY(const X509 *x509);

// X509_get0_pubkey returns |x509|'s public key as an |EVP_PKEY|, or NULL if the
// public key was unsupported or could not be decoded. The |EVP_PKEY| is cached
// in |x509|, so callers must not mutate the result.
OPENSSL_EXPORT EVP_PKEY *X509_get0_pubkey(const X509 *x509);

// X509_get_pubkey behaves like |X509_get0_pubkey| but increments the reference
// count on the |EVP_PKEY|. The caller must release the result with
// |EVP_PKEY_free| when done. The |EVP_PKEY| is cached in |x509|, so callers
// must not mutate the result.
OPENSSL_EXPORT EVP_PKEY *X509_get_pubkey(const X509 *x509);

// X509_get0_pubkey_bitstr returns the BIT STRING portion of |x509|'s public
// key. Note this does not contain the AlgorithmIdentifier portion.
//
// WARNING: This function returns a non-const pointer for OpenSSL compatibility,
// but the caller must not modify the resulting object. Doing so will break
// internal invariants in |x509|.
OPENSSL_EXPORT ASN1_BIT_STRING *X509_get0_pubkey_bitstr(const X509 *x509);

// X509_check_private_key returns one if |x509|'s public key matches |pkey| and
// zero otherwise.
OPENSSL_EXPORT int X509_check_private_key(const X509 *x509,
                                          const EVP_PKEY *pkey);

// X509_get0_uids sets |*out_issuer_uid| to a non-owning pointer to the
// issuerUID field of |x509|, or NULL if |x509| has no issuerUID. It similarly
// outputs |x509|'s subjectUID field to |*out_subject_uid|.
//
// Callers may pass NULL to either |out_issuer_uid| or |out_subject_uid| to
// ignore the corresponding field.
OPENSSL_EXPORT void X509_get0_uids(const X509 *x509,
                                   const ASN1_BIT_STRING **out_issuer_uid,
                                   const ASN1_BIT_STRING **out_subject_uid);

// The following bits are returned from |X509_get_extension_flags|.

// EXFLAG_BCONS indicates the certificate has a basic constraints extension.
#define EXFLAG_BCONS 0x1
// EXFLAG_KUSAGE indicates the certifcate has a key usage extension.
#define EXFLAG_KUSAGE 0x2
// EXFLAG_XKUSAGE indicates the certifcate has an extended key usage extension.
#define EXFLAG_XKUSAGE 0x4
// EXFLAG_NSCERT indicates the certificate has a legacy Netscape certificate
// type extension.
#define EXFLAG_NSCERT 0x8
// EXFLAG_CA indicates the certificate has a basic constraints extension with
// the CA bit set.
#define EXFLAG_CA 0x10
// EXFLAG_SI indicates the certificate is self-issued, i.e. its subject and
// issuer names match.
#define EXFLAG_SI 0x20
// EXFLAG_V1 indicates an X.509v1 certificate.
#define EXFLAG_V1 0x40
// EXFLAG_INVALID indicates an error processing some extension. The certificate
// should not be accepted. Note the lack of this bit does not imply all
// extensions are valid, only those used to compute extension flags.
#define EXFLAG_INVALID 0x80
// EXFLAG_SET is an internal bit that indicates extension flags were computed.
#define EXFLAG_SET 0x100
// EXFLAG_CRITICAL indicates an unsupported critical extension. The certificate
// should not be accepted.
#define EXFLAG_CRITICAL 0x200
// EXFLAG_SS indicates the certificate is likely self-signed. That is, if it is
// self-issued, its authority key identifier (if any) matches itself, and its
// key usage extension (if any) allows certificate signatures. The signature
// itself is not checked in computing this bit.
#define EXFLAG_SS 0x2000

// X509_get_extension_flags decodes a set of extensions from |x509| and returns
// a collection of |EXFLAG_*| bits which reflect |x509|. If there was an error
// in computing this bitmask, the result will include the |EXFLAG_INVALID| bit.
OPENSSL_EXPORT uint32_t X509_get_extension_flags(X509 *x509);

// X509_get_pathlen returns path length constraint from the basic constraints
// extension in |x509|. (See RFC 5280, section 4.2.1.9.) It returns -1 if the
// constraint is not present, or if some extension in |x509| was invalid.
//
// TODO(crbug.com/boringssl/381): Decoding an |X509| object will not check for
// invalid extensions. To detect the error case, call
// |X509_get_extension_flags| and check the |EXFLAG_INVALID| bit.
OPENSSL_EXPORT long X509_get_pathlen(X509 *x509);

// X509v3_KU_* are key usage bits returned from |X509_get_key_usage|.
#define X509v3_KU_DIGITAL_SIGNATURE 0x0080
#define X509v3_KU_NON_REPUDIATION 0x0040
#define X509v3_KU_KEY_ENCIPHERMENT 0x0020
#define X509v3_KU_DATA_ENCIPHERMENT 0x0010
#define X509v3_KU_KEY_AGREEMENT 0x0008
#define X509v3_KU_KEY_CERT_SIGN 0x0004
#define X509v3_KU_CRL_SIGN 0x0002
#define X509v3_KU_ENCIPHER_ONLY 0x0001
#define X509v3_KU_DECIPHER_ONLY 0x8000

// X509_get_key_usage returns a bitmask of key usages (see Section 4.2.1.3 of
// RFC 5280) which |x509| is valid for. This function only reports the first 16
// bits, in a little-endian byte order, but big-endian bit order. That is, bits
// 0 though 7 are reported at 1<<7 through 1<<0, and bits 8 through 15 are
// reported at 1<<15 through 1<<8.
//
// Instead of depending on this bit order, callers should compare against the
// |X509v3_KU_*| constants.
//
// If |x509| has no key usage extension, all key usages are valid and this
// function returns |UINT32_MAX|. If there was an error processing |x509|'s
// extensions, or if the first 16 bits in the key usage extension were all zero,
// this function returns zero.
OPENSSL_EXPORT uint32_t X509_get_key_usage(X509 *x509);

// XKU_* are extended key usage bits returned from
// |X509_get_extended_key_usage|.
#define XKU_SSL_SERVER 0x1
#define XKU_SSL_CLIENT 0x2
#define XKU_SMIME 0x4
#define XKU_CODE_SIGN 0x8
#define XKU_SGC 0x10
#define XKU_OCSP_SIGN 0x20
#define XKU_TIMESTAMP 0x40
#define XKU_DVCS 0x80
#define XKU_ANYEKU 0x100

// X509_get_extended_key_usage returns a bitmask of extended key usages (see
// Section 4.2.1.12 of RFC 5280) which |x509| is valid for. The result will be
// a combination of |XKU_*| constants. If checking an extended key usage not
// defined above, callers should extract the extended key usage extension
// separately, e.g. via |X509_get_ext_d2i|.
//
// If |x509| has no extended key usage extension, all extended key usages are
// valid and this function returns |UINT32_MAX|. If there was an error
// processing |x509|'s extensions, or if |x509|'s extended key usage extension
// contained no recognized usages, this function returns zero.
OPENSSL_EXPORT uint32_t X509_get_extended_key_usage(X509 *x509);

// X509_get0_subject_key_id returns |x509|'s subject key identifier, if present.
// (See RFC 5280, section 4.2.1.2.) It returns NULL if the extension is not
// present or if some extension in |x509| was invalid.
//
// TODO(crbug.com/boringssl/381): Decoding an |X509| object will not check for
// invalid extensions. To detect the error case, call
// |X509_get_extension_flags| and check the |EXFLAG_INVALID| bit.
OPENSSL_EXPORT const ASN1_OCTET_STRING *X509_get0_subject_key_id(X509 *x509);

// X509_get0_authority_key_id returns keyIdentifier of |x509|'s authority key
// identifier, if the extension and field are present. (See RFC 5280,
// section 4.2.1.1.) It returns NULL if the extension is not present, if it is
// present but lacks a keyIdentifier field, or if some extension in |x509| was
// invalid.
//
// TODO(crbug.com/boringssl/381): Decoding an |X509| object will not check for
// invalid extensions. To detect the error case, call
// |X509_get_extension_flags| and check the |EXFLAG_INVALID| bit.
OPENSSL_EXPORT const ASN1_OCTET_STRING *X509_get0_authority_key_id(X509 *x509);

DEFINE_STACK_OF(GENERAL_NAME)
typedef STACK_OF(GENERAL_NAME) GENERAL_NAMES;

// X509_get0_authority_issuer returns the authorityCertIssuer of |x509|'s
// authority key identifier, if the extension and field are present. (See
// RFC 5280, section 4.2.1.1.) It returns NULL if the extension is not present,
// if it is present but lacks a authorityCertIssuer field, or if some extension
// in |x509| was invalid.
//
// TODO(crbug.com/boringssl/381): Decoding an |X509| object will not check for
// invalid extensions. To detect the error case, call
// |X509_get_extension_flags| and check the |EXFLAG_INVALID| bit.
OPENSSL_EXPORT const GENERAL_NAMES *X509_get0_authority_issuer(X509 *x509);

// X509_get0_authority_serial returns the authorityCertSerialNumber of |x509|'s
// authority key identifier, if the extension and field are present. (See
// RFC 5280, section 4.2.1.1.) It returns NULL if the extension is not present,
// if it is present but lacks a authorityCertSerialNumber field, or if some
// extension in |x509| was invalid.
//
// TODO(crbug.com/boringssl/381): Decoding an |X509| object will not check for
// invalid extensions. To detect the error case, call
// |X509_get_extension_flags| and check the |EXFLAG_INVALID| bit.
OPENSSL_EXPORT const ASN1_INTEGER *X509_get0_authority_serial(X509 *x509);

// X509_get0_extensions returns |x509|'s extension list, or NULL if |x509| omits
// it.
OPENSSL_EXPORT const STACK_OF(X509_EXTENSION) *X509_get0_extensions(
    const X509 *x509);

// X509_get_ext_count returns the number of extensions in |x|.
OPENSSL_EXPORT int X509_get_ext_count(const X509 *x);

// X509_get_ext_by_NID behaves like |X509v3_get_ext_by_NID| but searches for
// extensions in |x|.
OPENSSL_EXPORT int X509_get_ext_by_NID(const X509 *x, int nid, int lastpos);

// X509_get_ext_by_OBJ behaves like |X509v3_get_ext_by_OBJ| but searches for
// extensions in |x|.
OPENSSL_EXPORT int X509_get_ext_by_OBJ(const X509 *x, const ASN1_OBJECT *obj,
                                       int lastpos);

// X509_get_ext_by_critical behaves like |X509v3_get_ext_by_critical| but
// searches for extensions in |x|.
OPENSSL_EXPORT int X509_get_ext_by_critical(const X509 *x, int crit,
                                            int lastpos);

// X509_get_ext returns the extension in |x| at index |loc|, or NULL if |loc| is
// out of bounds. This function returns a non-const pointer for OpenSSL
// compatibility, but callers should not mutate the result.
OPENSSL_EXPORT X509_EXTENSION *X509_get_ext(const X509 *x, int loc);

// X509_get_ext_d2i behaves like |X509V3_get_d2i| but looks for the extension in
// |x509|'s extension list.
//
// WARNING: This function is difficult to use correctly. See the documentation
// for |X509V3_get_d2i| for details.
OPENSSL_EXPORT void *X509_get_ext_d2i(const X509 *x509, int nid,
                                      int *out_critical, int *out_idx);

// X509_get0_tbs_sigalg returns the signature algorithm in |x509|'s
// TBSCertificate. For the outer signature algorithm, see |X509_get0_signature|.
//
// Certificates with mismatched signature algorithms will successfully parse,
// but they will be rejected when verifying.
OPENSSL_EXPORT const X509_ALGOR *X509_get0_tbs_sigalg(const X509 *x509);

// X509_SIG_INFO_* are flags for |X509_get_signature_info|.
// X509_SIG_INFO_VALID means that the signature info is valid.
#define X509_SIG_INFO_VALID 0x1
// X509_SIG_INFO_TLS means that the signature is suitable for TLS use.
#define X509_SIG_INFO_TLS 0x2

// X509_get_signature_info retrieves information about the signature of |x509|.
// The NID of the signing digest is written to |digest_nid|, the public key
// algorithm to |pubkey_nid|, the effective security bits to |sec_bits|, and
// flag details to |flags|. Parameters other than |x509| can be set to NULL if
// the information is not required. It is an error to pass a null pointer to
// |x509|.
OPENSSL_EXPORT int X509_get_signature_info(X509 *x509, int *digest_nid,
                                           int *pubkey_nid, int *sec_bits,
                                           uint32_t *flags);

// X509_get0_signature sets |*out_sig| and |*out_alg| to the signature and
// signature algorithm of |x509|, respectively. Either output pointer may be
// NULL to ignore the value.
//
// This function outputs the outer signature algorithm. For the one in the
// TBSCertificate, see |X509_get0_tbs_sigalg|. Certificates with mismatched
// signature algorithms will successfully parse, but they will be rejected when
// verifying.
OPENSSL_EXPORT void X509_get0_signature(const ASN1_BIT_STRING **out_sig,
                                        const X509_ALGOR **out_alg,
                                        const X509 *x509);

// X509_get_signature_nid returns the NID corresponding to |x509|'s signature
// algorithm, or |NID_undef| if the signature algorithm does not correspond to
// a known NID.
OPENSSL_EXPORT int X509_get_signature_nid(const X509 *x509);

// i2d_X509_tbs serializes the TBSCertificate portion of |x509|, as described in
// |i2d_SAMPLE|.
//
// This function preserves the original encoding of the TBSCertificate and may
// not reflect modifications made to |x509|. It may be used to manually verify
// the signature of an existing certificate. To generate certificates, use
// |i2d_re_X509_tbs| instead.
OPENSSL_EXPORT int i2d_X509_tbs(X509 *x509, unsigned char **outp);

// X509_verify checks that |x509| has a valid signature by |pkey|. It returns
// one if the signature is valid and zero otherwise. Note this function only
// checks the signature itself and does not perform a full certificate
// validation.
OPENSSL_EXPORT int X509_verify(X509 *x509, EVP_PKEY *pkey);

// X509_get1_email returns a newly-allocated list of NUL-terminated strings
// containing all email addresses in |x509|'s subject and all rfc822name names
// in |x509|'s subject alternative names. Email addresses which contain embedded
// NUL bytes are skipped.
//
// On error, or if there are no such email addresses, it returns NULL. When
// done, the caller must release the result with |X509_email_free|.
OPENSSL_EXPORT STACK_OF(OPENSSL_STRING) *X509_get1_email(const X509 *x509);

// X509_get1_ocsp returns a newly-allocated list of NUL-terminated strings
// containing all OCSP URIs in |x509|. That is, it collects all URI
// AccessDescriptions with an accessMethod of id-ad-ocsp in |x509|'s authority
// information access extension. URIs which contain embedded NUL bytes are
// skipped.
//
// On error, or if there are no such URIs, it returns NULL. When done, the
// caller must release the result with |X509_email_free|.
OPENSSL_EXPORT STACK_OF(OPENSSL_STRING) *X509_get1_ocsp(const X509 *x509);

// X509_email_free releases memory associated with |sk|, including |sk| itself.
// Each |OPENSSL_STRING| in |sk| must be a NUL-terminated string allocated with
// |OPENSSL_malloc|. If |sk| is NULL, no action is taken.
OPENSSL_EXPORT void X509_email_free(STACK_OF(OPENSSL_STRING) *sk);

// X509_cmp compares |a| and |b| and returns zero if they are equal, a negative
// number if |b| sorts after |a| and a negative number if |a| sorts after |b|.
// The sort order implemented by this function is arbitrary and does not
// reflect properties of the certificate such as expiry. Applications should not
// rely on the order itself.
//
// TODO(https://crbug.com/boringssl/355): This function works by comparing a
// cached hash of the encoded certificate. If |a| or |b| could not be
// serialized, the current behavior is to compare all unencodable certificates
// as equal. This function should only be used with |X509| objects that were
// parsed from bytes and never mutated.
//
// TODO(https://crbug.com/boringssl/407): This function is const, but it is not
// always thread-safe, notably if |a| and |b| were mutated.
OPENSSL_EXPORT int X509_cmp(const X509 *a, const X509 *b);


// Issuing certificates.
//
// An |X509| object may also represent an incomplete certificate. Callers may
// construct empty |X509| objects, fill in fields individually, and finally sign
// the result. The following functions may be used for this purpose.

// X509_new returns a newly-allocated, empty |X509| object, or NULL on error.
// This produces an incomplete certificate which may be filled in to issue a new
// certificate.
OPENSSL_EXPORT X509 *X509_new(void);

// X509_set_version sets |x509|'s version to |version|, which should be one of
// the |X509V_VERSION_*| constants. It returns one on success and zero on error.
//
// If unsure, use |X509_VERSION_3|.
OPENSSL_EXPORT int X509_set_version(X509 *x509, long version);

// X509_set_serialNumber sets |x509|'s serial number to |serial|. It returns one
// on success and zero on error.
OPENSSL_EXPORT int X509_set_serialNumber(X509 *x509,
                                         const ASN1_INTEGER *serial);

// X509_set1_notBefore sets |x509|'s notBefore time to |tm|. It returns one on
// success and zero on error.
OPENSSL_EXPORT int X509_set1_notBefore(X509 *x509, const ASN1_TIME *tm);

// X509_set1_notAfter sets |x509|'s notAfter time to |tm|. it returns one on
// success and zero on error.
OPENSSL_EXPORT int X509_set1_notAfter(X509 *x509, const ASN1_TIME *tm);

// X509_getm_notBefore returns a mutable pointer to |x509|'s notBefore time.
OPENSSL_EXPORT ASN1_TIME *X509_getm_notBefore(X509 *x509);

// X509_getm_notAfter returns a mutable pointer to |x509|'s notAfter time.
OPENSSL_EXPORT ASN1_TIME *X509_getm_notAfter(X509 *x);

// X509_set_issuer_name sets |x509|'s issuer to a copy of |name|. It returns one
// on success and zero on error.
OPENSSL_EXPORT int X509_set_issuer_name(X509 *x509, X509_NAME *name);

// X509_set_subject_name sets |x509|'s subject to a copy of |name|. It returns
// one on success and zero on error.
OPENSSL_EXPORT int X509_set_subject_name(X509 *x509, X509_NAME *name);

// X509_set_pubkey sets |x509|'s public key to |pkey|. It returns one on success
// and zero on error. This function does not take ownership of |pkey| and
// internally copies and updates reference counts as needed.
OPENSSL_EXPORT int X509_set_pubkey(X509 *x509, EVP_PKEY *pkey);

// X509_delete_ext removes the extension in |x| at index |loc| and returns the
// removed extension, or NULL if |loc| was out of bounds. If non-NULL, the
// caller must release the result with |X509_EXTENSION_free|.
OPENSSL_EXPORT X509_EXTENSION *X509_delete_ext(X509 *x, int loc);

// X509_add_ext adds a copy of |ex| to |x|. It returns one on success and zero
// on failure. The caller retains ownership of |ex| and can release it
// independently of |x|.
//
// The new extension is inserted at index |loc|, shifting extensions to the
// right. If |loc| is -1 or out of bounds, the new extension is appended to the
// list.
OPENSSL_EXPORT int X509_add_ext(X509 *x, const X509_EXTENSION *ex, int loc);

// X509_add1_ext_i2d behaves like |X509V3_add1_i2d| but adds the extension to
// |x|'s extension list.
//
// WARNING: This function may return zero or -1 on error. The caller must also
// ensure |value|'s type matches |nid|. See the documentation for
// |X509V3_add1_i2d| for details.
OPENSSL_EXPORT int X509_add1_ext_i2d(X509 *x, int nid, void *value, int crit,
                                     unsigned long flags);

// X509_sign signs |x509| with |pkey| and replaces the signature algorithm and
// signature fields. It returns the length of the signature on success and zero
// on error. This function uses digest algorithm |md|, or |pkey|'s default if
// NULL. Other signing parameters use |pkey|'s defaults. To customize them, use
// |X509_sign_ctx|.
OPENSSL_EXPORT int X509_sign(X509 *x509, EVP_PKEY *pkey, const EVP_MD *md);

// X509_sign_ctx signs |x509| with |ctx| and replaces the signature algorithm
// and signature fields. It returns the length of the signature on success and
// zero on error. The signature algorithm and parameters come from |ctx|, which
// must have been initialized with |EVP_DigestSignInit|. The caller should
// configure the corresponding |EVP_PKEY_CTX| before calling this function.
//
// On success or failure, this function mutates |ctx| and resets it to the empty
// state. Caller should not rely on its contents after the function returns.
OPENSSL_EXPORT int X509_sign_ctx(X509 *x509, EVP_MD_CTX *ctx);

// i2d_re_X509_tbs serializes the TBSCertificate portion of |x509|, as described
// in |i2d_SAMPLE|.
//
// This function re-encodes the TBSCertificate and may not reflect |x509|'s
// original encoding. It may be used to manually generate a signature for a new
// certificate. To verify certificates, use |i2d_X509_tbs| instead.
OPENSSL_EXPORT int i2d_re_X509_tbs(X509 *x509, unsigned char **outp);

// X509_set1_signature_algo sets |x509|'s signature algorithm to |algo| and
// returns one on success or zero on error. It updates both the signature field
// of the TBSCertificate structure, and the signatureAlgorithm field of the
// Certificate.
OPENSSL_EXPORT int X509_set1_signature_algo(X509 *x509, const X509_ALGOR *algo);

// X509_set1_signature_value sets |x509|'s signature to a copy of the |sig_len|
// bytes pointed by |sig|. It returns one on success and zero on error.
//
// Due to a specification error, X.509 certificates store signatures in ASN.1
// BIT STRINGs, but signature algorithms return byte strings rather than bit
// strings. This function creates a BIT STRING containing a whole number of
// bytes, with the bit order matching the DER encoding. This matches the
// encoding used by all X.509 signature algorithms.
OPENSSL_EXPORT int X509_set1_signature_value(X509 *x509, const uint8_t *sig,
                                             size_t sig_len);


// Auxiliary certificate properties.
//
// |X509| objects optionally maintain auxiliary properties. These are not part
// of the certificates themselves, and thus are not covered by signatures or
// preserved by the standard serialization. They are used as inputs or outputs
// to other functions in this library.

// i2d_X509_AUX marshals |x509| as a DER-encoded X.509 Certificate (RFC 5280),
// followed optionally by a separate, OpenSSL-specific structure with auxiliary
// properties. It behaves as described in |i2d_SAMPLE|.
//
// Unlike similarly-named functions, this function does not output a single
// ASN.1 element. Directly embedding the output in a larger ASN.1 structure will
// not behave correctly.
//
// TODO(crbug.com/boringssl/407): |x509| should be const.
OPENSSL_EXPORT int i2d_X509_AUX(X509 *x509, uint8_t **outp);

// d2i_X509_AUX parses up to |length| bytes from |*inp| as a DER-encoded X.509
// Certificate (RFC 5280), followed optionally by a separate, OpenSSL-specific
// structure with auxiliary properties. It behaves as described in
// |d2i_SAMPLE_with_reuse|.
//
// WARNING: Passing untrusted input to this function allows an attacker to
// control auxiliary properties. This can allow unexpected influence over the
// application if the certificate is used in a context that reads auxiliary
// properties. This includes PKCS#12 serialization, trusted certificates in
// |X509_STORE|, and callers of |X509_alias_get0| or |X509_keyid_get0|.
//
// Unlike similarly-named functions, this function does not parse a single
// ASN.1 element. Trying to parse data directly embedded in a larger ASN.1
// structure will not behave correctly.
OPENSSL_EXPORT X509 *d2i_X509_AUX(X509 **x509, const uint8_t **inp,
                                  long length);

// X509_alias_set1 sets |x509|'s alias to |len| bytes from |name|. If |name| is
// NULL, the alias is cleared instead. Aliases are not part of the certificate
// itself and will not be serialized by |i2d_X509|. If |x509| is serialized in
// a PKCS#12 structure, the friendlyName attribute (RFC 2985) will contain this
// alias.
OPENSSL_EXPORT int X509_alias_set1(X509 *x509, const uint8_t *name,
                                   ossl_ssize_t len);

// X509_keyid_set1 sets |x509|'s key ID to |len| bytes from |id|. If |id| is
// NULL, the key ID is cleared instead. Key IDs are not part of the certificate
// itself and will not be serialized by |i2d_X509|.
OPENSSL_EXPORT int X509_keyid_set1(X509 *x509, const uint8_t *id,
                                   ossl_ssize_t len);

// X509_alias_get0 looks up |x509|'s alias. If found, it sets |*out_len| to the
// alias's length and returns a pointer to a buffer containing the contents. If
// not found, it outputs the empty string by returning NULL and setting
// |*out_len| to zero.
//
// If |x509| was parsed from a PKCS#12 structure (see
// |PKCS12_get_key_and_certs|), the alias will reflect the friendlyName
// attribute (RFC 2985).
//
// WARNING: In OpenSSL, this function did not set |*out_len| when the alias was
// missing. Callers that target both OpenSSL and BoringSSL should set the value
// to zero before calling this function.
OPENSSL_EXPORT const uint8_t *X509_alias_get0(const X509 *x509, int *out_len);

// X509_keyid_get0 looks up |x509|'s key ID. If found, it sets |*out_len| to the
// key ID's length and returns a pointer to a buffer containing the contents. If
// not found, it outputs the empty string by returning NULL and setting
// |*out_len| to zero.
//
// WARNING: In OpenSSL, this function did not set |*out_len| when the alias was
// missing. Callers that target both OpenSSL and BoringSSL should set the value
// to zero before calling this function.
OPENSSL_EXPORT const uint8_t *X509_keyid_get0(const X509 *x509, int *out_len);

// X509_add1_trust_object configures |x509| as a valid trust anchor for |obj|.
// It returns one on success and zero on error. |obj| should be a certificate
// usage OID associated with an |X509_TRUST| object.
//
// See |X509_VERIFY_PARAM_set_trust| for details on how this value is evaluated.
// Note this only takes effect if |x509| was configured as a trusted certificate
// via |X509_STORE|.
OPENSSL_EXPORT int X509_add1_trust_object(X509 *x509, const ASN1_OBJECT *obj);

// X509_add1_reject_object configures |x509| as distrusted for |obj|. It returns
// one on success and zero on error. |obj| should be a certificate usage OID
// associated with an |X509_TRUST| object.
//
// See |X509_VERIFY_PARAM_set_trust| for details on how this value is evaluated.
// Note this only takes effect if |x509| was configured as a trusted certificate
// via |X509_STORE|.
OPENSSL_EXPORT int X509_add1_reject_object(X509 *x509, const ASN1_OBJECT *obj);

// X509_trust_clear clears the list of OIDs for which |x509| is trusted. See
// also |X509_add1_trust_object|.
OPENSSL_EXPORT void X509_trust_clear(X509 *x509);

// X509_reject_clear clears the list of OIDs for which |x509| is distrusted. See
// also |X509_add1_reject_object|.
OPENSSL_EXPORT void X509_reject_clear(X509 *x509);


// Certificate revocation lists.
//
// An |X509_CRL| object represents an X.509 certificate revocation list (CRL),
// defined in RFC 5280. A CRL is a signed list of certificates, the
// revokedCertificates field, which are no longer considered valid. Each entry
// of this list is represented with an |X509_REVOKED| object, documented in the
// "CRL entries" section below.
//
// Although an |X509_CRL| is a mutable object, mutating an |X509_CRL| or its
// |X509_REVOKED|s can give incorrect results. Callers typically obtain
// |X509_CRL|s by parsing some input with |d2i_X509_CRL|, etc. Such objects
// carry information such as the serialized TBSCertList and decoded extensions,
// which will become inconsistent when mutated.
//
// Instead, mutation functions should only be used when issuing new CRLs, as
// described in a later section.

DEFINE_STACK_OF(X509_CRL)
DEFINE_STACK_OF(X509_REVOKED)

// X509_CRL is an |ASN1_ITEM| whose ASN.1 type is X.509 CertificateList (RFC
// 5280) and C type is |X509_CRL*|.
DECLARE_ASN1_ITEM(X509_CRL)

// X509_CRL_up_ref adds one to the reference count of |crl| and returns one.
OPENSSL_EXPORT int X509_CRL_up_ref(X509_CRL *crl);

// X509_CRL_dup returns a newly-allocated copy of |crl|, or NULL on error. This
// function works by serializing the structure, so if |crl| is incomplete, it
// may fail.
//
// TODO(https://crbug.com/boringssl/407): This function should be const and
// thread-safe but is currently neither in some cases, notably if |crl| was
// mutated.
OPENSSL_EXPORT X509_CRL *X509_CRL_dup(X509_CRL *crl);

// X509_CRL_free decrements |crl|'s reference count and, if zero, releases
// memory associated with |crl|.
OPENSSL_EXPORT void X509_CRL_free(X509_CRL *crl);

// d2i_X509_CRL parses up to |len| bytes from |*inp| as a DER-encoded X.509
// CertificateList (RFC 5280), as described in |d2i_SAMPLE_with_reuse|.
OPENSSL_EXPORT X509_CRL *d2i_X509_CRL(X509_CRL **out, const uint8_t **inp,
                                      long len);

// i2d_X509_CRL marshals |crl| as a X.509 CertificateList (RFC 5280), as
// described in |i2d_SAMPLE|.
//
// TODO(https://crbug.com/boringssl/407): This function should be const and
// thread-safe but is currently neither in some cases, notably if |crl| was
// mutated.
OPENSSL_EXPORT int i2d_X509_CRL(X509_CRL *crl, uint8_t **outp);

// X509_CRL_match compares |a| and |b| and returns zero if they are equal, a
// negative number if |b| sorts after |a| and a negative number if |a| sorts
// after |b|. The sort order implemented by this function is arbitrary and does
// not reflect properties of the CRL such as expiry. Applications should not
// rely on the order itself.
//
// TODO(https://crbug.com/boringssl/355): This function works by comparing a
// cached hash of the encoded CRL. This cached hash is computed when the CRL is
// parsed, but not when mutating or issuing CRLs. This function should only be
// used with |X509_CRL| objects that were parsed from bytes and never mutated.
OPENSSL_EXPORT int X509_CRL_match(const X509_CRL *a, const X509_CRL *b);

#define X509_CRL_VERSION_1 0
#define X509_CRL_VERSION_2 1

// X509_CRL_get_version returns the numerical value of |crl|'s version, which
// will be one of the |X509_CRL_VERSION_*| constants.
OPENSSL_EXPORT long X509_CRL_get_version(const X509_CRL *crl);

// X509_CRL_get0_lastUpdate returns |crl|'s thisUpdate time. The OpenSSL API
// refers to this field as lastUpdate.
OPENSSL_EXPORT const ASN1_TIME *X509_CRL_get0_lastUpdate(const X509_CRL *crl);

// X509_CRL_get0_nextUpdate returns |crl|'s nextUpdate time, or NULL if |crl|
// has none.
OPENSSL_EXPORT const ASN1_TIME *X509_CRL_get0_nextUpdate(const X509_CRL *crl);

// X509_CRL_get_issuer returns |crl|'s issuer name. Note this function is not
// const-correct for legacy reasons.
OPENSSL_EXPORT X509_NAME *X509_CRL_get_issuer(const X509_CRL *crl);

// X509_CRL_get0_by_serial finds the entry in |crl| whose serial number is
// |serial|. If found, it sets |*out| to the entry and returns one. If not
// found, it returns zero.
//
// On success, |*out| continues to be owned by |crl|. It is an error to free or
// otherwise modify |*out|.
//
// TODO(crbug.com/boringssl/600): Ideally |crl| would be const. It is broadly
// thread-safe, but changes the order of entries in |crl|. It cannot be called
// concurrently with |i2d_X509_CRL|.
OPENSSL_EXPORT int X509_CRL_get0_by_serial(X509_CRL *crl, X509_REVOKED **out,
                                           const ASN1_INTEGER *serial);

// X509_CRL_get0_by_cert behaves like |X509_CRL_get0_by_serial|, except it looks
// for the entry that matches |x509|.
OPENSSL_EXPORT int X509_CRL_get0_by_cert(X509_CRL *crl, X509_REVOKED **out,
                                         X509 *x509);

// X509_CRL_get_REVOKED returns the list of revoked certificates in |crl|, or
// NULL if |crl| omits it.
//
// TOOD(davidben): This function was originally a macro, without clear const
// semantics. It should take a const input and give const output, but the latter
// would break existing callers. For now, we match upstream.
OPENSSL_EXPORT STACK_OF(X509_REVOKED) *X509_CRL_get_REVOKED(X509_CRL *crl);

// X509_CRL_get0_extensions returns |crl|'s extension list, or NULL if |crl|
// omits it. A CRL can have extensions on individual entries, which is
// |X509_REVOKED_get0_extensions|, or on the overall CRL, which is this
// function.
OPENSSL_EXPORT const STACK_OF(X509_EXTENSION) *X509_CRL_get0_extensions(
    const X509_CRL *crl);

// X509_CRL_get_ext_count returns the number of extensions in |x|.
OPENSSL_EXPORT int X509_CRL_get_ext_count(const X509_CRL *x);

// X509_CRL_get_ext_by_NID behaves like |X509v3_get_ext_by_NID| but searches for
// extensions in |x|.
OPENSSL_EXPORT int X509_CRL_get_ext_by_NID(const X509_CRL *x, int nid,
                                           int lastpos);

// X509_CRL_get_ext_by_OBJ behaves like |X509v3_get_ext_by_OBJ| but searches for
// extensions in |x|.
OPENSSL_EXPORT int X509_CRL_get_ext_by_OBJ(const X509_CRL *x,
                                           const ASN1_OBJECT *obj, int lastpos);

// X509_CRL_get_ext_by_critical behaves like |X509v3_get_ext_by_critical| but
// searches for extensions in |x|.
OPENSSL_EXPORT int X509_CRL_get_ext_by_critical(const X509_CRL *x, int crit,
                                                int lastpos);

// X509_CRL_get_ext returns the extension in |x| at index |loc|, or NULL if
// |loc| is out of bounds. This function returns a non-const pointer for OpenSSL
// compatibility, but callers should not mutate the result.
OPENSSL_EXPORT X509_EXTENSION *X509_CRL_get_ext(const X509_CRL *x, int loc);

// X509_CRL_get_ext_d2i behaves like |X509V3_get_d2i| but looks for the
// extension in |crl|'s extension list.
//
// WARNING: This function is difficult to use correctly. See the documentation
// for |X509V3_get_d2i| for details.
OPENSSL_EXPORT void *X509_CRL_get_ext_d2i(const X509_CRL *crl, int nid,
                                          int *out_critical, int *out_idx);

// X509_CRL_get0_signature sets |*out_sig| and |*out_alg| to the signature and
// signature algorithm of |crl|, respectively. Either output pointer may be NULL
// to ignore the value.
//
// This function outputs the outer signature algorithm, not the one in the
// TBSCertList. CRLs with mismatched signature algorithms will successfully
// parse, but they will be rejected when verifying.
OPENSSL_EXPORT void X509_CRL_get0_signature(const X509_CRL *crl,
                                            const ASN1_BIT_STRING **out_sig,
                                            const X509_ALGOR **out_alg);

// X509_CRL_get_signature_nid returns the NID corresponding to |crl|'s signature
// algorithm, or |NID_undef| if the signature algorithm does not correspond to
// a known NID.
OPENSSL_EXPORT int X509_CRL_get_signature_nid(const X509_CRL *crl);

// i2d_X509_CRL_tbs serializes the TBSCertList portion of |crl|, as described in
// |i2d_SAMPLE|.
//
// This function preserves the original encoding of the TBSCertList and may not
// reflect modifications made to |crl|. It may be used to manually verify the
// signature of an existing CRL. To generate CRLs, use |i2d_re_X509_CRL_tbs|
// instead.
OPENSSL_EXPORT int i2d_X509_CRL_tbs(X509_CRL *crl, unsigned char **outp);

// X509_CRL_verify checks that |crl| has a valid signature by |pkey|. It returns
// one if the signature is valid and zero otherwise.
OPENSSL_EXPORT int X509_CRL_verify(X509_CRL *crl, EVP_PKEY *pkey);


// Issuing certificate revocation lists.
//
// An |X509_CRL| object may also represent an incomplete CRL. Callers may
// construct empty |X509_CRL| objects, fill in fields individually, and finally
// sign the result. The following functions may be used for this purpose.

// X509_CRL_new returns a newly-allocated, empty |X509_CRL| object, or NULL on
// error. This object may be filled in and then signed to construct a CRL.
OPENSSL_EXPORT X509_CRL *X509_CRL_new(void);

// X509_CRL_set_version sets |crl|'s version to |version|, which should be one
// of the |X509_CRL_VERSION_*| constants. It returns one on success and zero on
// error.
//
// If unsure, use |X509_CRL_VERSION_2|. Note that, unlike certificates, CRL
// versions are only defined up to v2. Callers should not use |X509_VERSION_3|.
OPENSSL_EXPORT int X509_CRL_set_version(X509_CRL *crl, long version);

// X509_CRL_set_issuer_name sets |crl|'s issuer to a copy of |name|. It returns
// one on success and zero on error.
OPENSSL_EXPORT int X509_CRL_set_issuer_name(X509_CRL *crl, X509_NAME *name);

// X509_CRL_set1_lastUpdate sets |crl|'s thisUpdate time to |tm|. It returns one
// on success and zero on error. The OpenSSL API refers to this field as
// lastUpdate.
OPENSSL_EXPORT int X509_CRL_set1_lastUpdate(X509_CRL *crl, const ASN1_TIME *tm);

// X509_CRL_set1_nextUpdate sets |crl|'s nextUpdate time to |tm|. It returns one
// on success and zero on error.
OPENSSL_EXPORT int X509_CRL_set1_nextUpdate(X509_CRL *crl, const ASN1_TIME *tm);

// X509_CRL_add0_revoked adds |rev| to |crl|. On success, it takes ownership of
// |rev| and returns one. On error, it returns zero. If this function fails, the
// caller retains ownership of |rev| and must release it when done.
OPENSSL_EXPORT int X509_CRL_add0_revoked(X509_CRL *crl, X509_REVOKED *rev);

// X509_CRL_sort sorts the entries in |crl| by serial number. It returns one on
// success and zero on error.
OPENSSL_EXPORT int X509_CRL_sort(X509_CRL *crl);

// X509_CRL_delete_ext removes the extension in |x| at index |loc| and returns
// the removed extension, or NULL if |loc| was out of bounds. If non-NULL, the
// caller must release the result with |X509_EXTENSION_free|.
OPENSSL_EXPORT X509_EXTENSION *X509_CRL_delete_ext(X509_CRL *x, int loc);

// X509_CRL_add_ext adds a copy of |ex| to |x|. It returns one on success and
// zero on failure. The caller retains ownership of |ex| and can release it
// independently of |x|.
//
// The new extension is inserted at index |loc|, shifting extensions to the
// right. If |loc| is -1 or out of bounds, the new extension is appended to the
// list.
OPENSSL_EXPORT int X509_CRL_add_ext(X509_CRL *x, const X509_EXTENSION *ex,
                                    int loc);

// X509_CRL_add1_ext_i2d behaves like |X509V3_add1_i2d| but adds the extension
// to |x|'s extension list.
//
// WARNING: This function may return zero or -1 on error. The caller must also
// ensure |value|'s type matches |nid|. See the documentation for
// |X509V3_add1_i2d| for details.
OPENSSL_EXPORT int X509_CRL_add1_ext_i2d(X509_CRL *x, int nid, void *value,
                                         int crit, unsigned long flags);

// X509_CRL_sign signs |crl| with |pkey| and replaces the signature algorithm
// and signature fields. It returns the length of the signature on success and
// zero on error. This function uses digest algorithm |md|, or |pkey|'s default
// if NULL. Other signing parameters use |pkey|'s defaults. To customize them,
// use |X509_CRL_sign_ctx|.
OPENSSL_EXPORT int X509_CRL_sign(X509_CRL *crl, EVP_PKEY *pkey,
                                 const EVP_MD *md);

// X509_CRL_sign_ctx signs |crl| with |ctx| and replaces the signature algorithm
// and signature fields. It returns the length of the signature on success and
// zero on error. The signature algorithm and parameters come from |ctx|, which
// must have been initialized with |EVP_DigestSignInit|. The caller should
// configure the corresponding |EVP_PKEY_CTX| before calling this function.
//
// On success or failure, this function mutates |ctx| and resets it to the empty
// state. Caller should not rely on its contents after the function returns.
OPENSSL_EXPORT int X509_CRL_sign_ctx(X509_CRL *crl, EVP_MD_CTX *ctx);

// i2d_re_X509_CRL_tbs serializes the TBSCertList portion of |crl|, as described
// in |i2d_SAMPLE|.
//
// This function re-encodes the TBSCertList and may not reflect |crl|'s original
// encoding. It may be used to manually generate a signature for a new CRL. To
// verify CRLs, use |i2d_X509_CRL_tbs| instead.
OPENSSL_EXPORT int i2d_re_X509_CRL_tbs(X509_CRL *crl, unsigned char **outp);

// X509_CRL_set1_signature_algo sets |crl|'s signature algorithm to |algo| and
// returns one on success or zero on error. It updates both the signature field
// of the TBSCertList structure, and the signatureAlgorithm field of the CRL.
OPENSSL_EXPORT int X509_CRL_set1_signature_algo(X509_CRL *crl,
                                                const X509_ALGOR *algo);

// X509_CRL_set1_signature_value sets |crl|'s signature to a copy of the
// |sig_len| bytes pointed by |sig|. It returns one on success and zero on
// error.
//
// Due to a specification error, X.509 CRLs store signatures in ASN.1 BIT
// STRINGs, but signature algorithms return byte strings rather than bit
// strings. This function creates a BIT STRING containing a whole number of
// bytes, with the bit order matching the DER encoding. This matches the
// encoding used by all X.509 signature algorithms.
OPENSSL_EXPORT int X509_CRL_set1_signature_value(X509_CRL *crl,
                                                 const uint8_t *sig,
                                                 size_t sig_len);

// X509_CRL_http_nbio calls |OCSP_REQ_CTX_nbio_d2i| to exchange the request
// via http. On success, it parses the response as a DER-encoded |X509_CRL|
// ASN.1 structure.
OPENSSL_EXPORT int X509_CRL_http_nbio(OCSP_REQ_CTX *rctx, X509_CRL **pcrl);


// CRL entries.
//
// Each entry of a CRL is represented as an |X509_REVOKED| object, which
// describes a revoked certificate by serial number.
//
// When an |X509_REVOKED| is obtained from an |X509_CRL| object, it is an error
// to mutate the object. Doing so may break |X509_CRL|'s and cause the library
// to behave incorrectly.

// X509_REVOKED is an |ASN1_ITEM| whose ASN.1 type is an element of the
// revokedCertificates field of TBSCertList (RFC 5280) and C type is
// |X509_REVOKED*|.
DECLARE_ASN1_ITEM(X509_REVOKED)

// X509_REVOKED_new returns a newly-allocated, empty |X509_REVOKED| object, or
// NULL on allocation error.
OPENSSL_EXPORT X509_REVOKED *X509_REVOKED_new(void);

// X509_REVOKED_free releases memory associated with |rev|.
OPENSSL_EXPORT void X509_REVOKED_free(X509_REVOKED *rev);

// d2i_X509_REVOKED parses up to |len| bytes from |*inp| as a DER-encoded X.509
// CRL entry, as described in |d2i_SAMPLE|.
OPENSSL_EXPORT X509_REVOKED *d2i_X509_REVOKED(X509_REVOKED **out,
                                              const uint8_t **inp, long len);

// i2d_X509_REVOKED marshals |alg| as a DER-encoded X.509 CRL entry, as
// described in |i2d_SAMPLE|.
OPENSSL_EXPORT int i2d_X509_REVOKED(const X509_REVOKED *alg, uint8_t **outp);

// X509_REVOKED_dup returns a newly-allocated copy of |rev|, or NULL on error.
// This function works by serializing the structure, so if |rev| is incomplete,
// it may fail.
OPENSSL_EXPORT X509_REVOKED *X509_REVOKED_dup(const X509_REVOKED *rev);

// X509_REVOKED_get0_serialNumber returns the serial number of the certificate
// revoked by |revoked|.
OPENSSL_EXPORT const ASN1_INTEGER *X509_REVOKED_get0_serialNumber(
    const X509_REVOKED *revoked);

// X509_REVOKED_set_serialNumber sets |revoked|'s serial number to |serial|. It
// returns one on success or zero on error.
OPENSSL_EXPORT int X509_REVOKED_set_serialNumber(X509_REVOKED *revoked,
                                                 const ASN1_INTEGER *serial);

// X509_REVOKED_get0_revocationDate returns the revocation time of the
// certificate revoked by |revoked|.
OPENSSL_EXPORT const ASN1_TIME *X509_REVOKED_get0_revocationDate(
    const X509_REVOKED *revoked);

// X509_REVOKED_set_revocationDate sets |revoked|'s revocation time to |tm|. It
// returns one on success or zero on error.
OPENSSL_EXPORT int X509_REVOKED_set_revocationDate(X509_REVOKED *revoked,
                                                   const ASN1_TIME *tm);

// X509_REVOKED_get0_extensions returns |r|'s extensions list, or NULL if |r|
// omits it. A CRL can have extensions on individual entries, which is this
// function, or on the overall CRL, which is |X509_CRL_get0_extensions|.
OPENSSL_EXPORT const STACK_OF(X509_EXTENSION) *X509_REVOKED_get0_extensions(
    const X509_REVOKED *r);

// X509_REVOKED_get_ext_count returns the number of extensions in |x|.
OPENSSL_EXPORT int X509_REVOKED_get_ext_count(const X509_REVOKED *x);

// X509_REVOKED_get_ext_by_NID behaves like |X509v3_get_ext_by_NID| but searches
// for extensions in |x|.
OPENSSL_EXPORT int X509_REVOKED_get_ext_by_NID(const X509_REVOKED *x, int nid,
                                               int lastpos);

// X509_REVOKED_get_ext_by_OBJ behaves like |X509v3_get_ext_by_OBJ| but searches
// for extensions in |x|.
OPENSSL_EXPORT int X509_REVOKED_get_ext_by_OBJ(const X509_REVOKED *x,
                                               const ASN1_OBJECT *obj,
                                               int lastpos);

// X509_REVOKED_get_ext_by_critical behaves like |X509v3_get_ext_by_critical|
// but searches for extensions in |x|.
OPENSSL_EXPORT int X509_REVOKED_get_ext_by_critical(const X509_REVOKED *x,
                                                    int crit, int lastpos);

// X509_REVOKED_get_ext returns the extension in |x| at index |loc|, or NULL if
// |loc| is out of bounds. This function returns a non-const pointer for OpenSSL
// compatibility, but callers should not mutate the result.
OPENSSL_EXPORT X509_EXTENSION *X509_REVOKED_get_ext(const X509_REVOKED *x,
                                                    int loc);

// X509_REVOKED_delete_ext removes the extension in |x| at index |loc| and
// returns the removed extension, or NULL if |loc| was out of bounds. If
// non-NULL, the caller must release the result with |X509_EXTENSION_free|.
OPENSSL_EXPORT X509_EXTENSION *X509_REVOKED_delete_ext(X509_REVOKED *x,
                                                       int loc);

// X509_REVOKED_add_ext adds a copy of |ex| to |x|. It returns one on success
// and zero on failure. The caller retains ownership of |ex| and can release it
// independently of |x|.
//
// The new extension is inserted at index |loc|, shifting extensions to the
// right. If |loc| is -1 or out of bounds, the new extension is appended to the
// list.
OPENSSL_EXPORT int X509_REVOKED_add_ext(X509_REVOKED *x,
                                        const X509_EXTENSION *ex, int loc);

// X509_REVOKED_get_ext_d2i behaves like |X509V3_get_d2i| but looks for the
// extension in |revoked|'s extension list.
//
// WARNING: This function is difficult to use correctly. See the documentation
// for |X509V3_get_d2i| for details.
OPENSSL_EXPORT void *X509_REVOKED_get_ext_d2i(const X509_REVOKED *revoked,
                                              int nid, int *out_critical,
                                              int *out_idx);

// X509_REVOKED_add1_ext_i2d behaves like |X509V3_add1_i2d| but adds the
// extension to |x|'s extension list.
//
// WARNING: This function may return zero or -1 on error. The caller must also
// ensure |value|'s type matches |nid|. See the documentation for
// |X509V3_add1_i2d| for details.
OPENSSL_EXPORT int X509_REVOKED_add1_ext_i2d(X509_REVOKED *x, int nid,
                                             void *value, int crit,
                                             unsigned long flags);


// Certificate requests.
//
// An |X509_REQ| represents a PKCS #10 certificate request (RFC 2986). These are
// also referred to as certificate signing requests or CSRs. CSRs are a common
// format used to request a certificate from a CA.
//
// Although an |X509_REQ| is a mutable object, mutating an |X509_REQ| can give
// incorrect results. Callers typically obtain |X509_REQ|s by parsing some input
// with |d2i_X509_REQ|, etc. Such objects carry information such as the
// serialized CertificationRequestInfo, which will become inconsistent when
// mutated.
//
// Instead, mutation functions should only be used when issuing new CRLs, as
// described in a later section.

// X509_REQ is an |ASN1_ITEM| whose ASN.1 type is CertificateRequest (RFC 2986)
// and C type is |X509_REQ*|.
DECLARE_ASN1_ITEM(X509_REQ)

// X509_REQ_dup returns a newly-allocated copy of |req|, or NULL on error. This
// function works by serializing the structure, so if |req| is incomplete, it
// may fail.
//
// TODO(https://crbug.com/boringssl/407): This function should be const and
// thread-safe but is currently neither in some cases, notably if |req| was
// mutated.
OPENSSL_EXPORT X509_REQ *X509_REQ_dup(X509_REQ *req);

// X509_REQ_free releases memory associated with |req|.
OPENSSL_EXPORT void X509_REQ_free(X509_REQ *req);

// d2i_X509_REQ parses up to |len| bytes from |*inp| as a DER-encoded
// CertificateRequest (RFC 2986), as described in |d2i_SAMPLE_with_reuse|.
OPENSSL_EXPORT X509_REQ *d2i_X509_REQ(X509_REQ **out, const uint8_t **inp,
                                      long len);

// i2d_X509_REQ marshals |req| as a CertificateRequest (RFC 2986), as described
// in |i2d_SAMPLE|.
//
// TODO(https://crbug.com/boringssl/407): This function should be const and
// thread-safe but is currently neither in some cases, notably if |req| was
// mutated.
OPENSSL_EXPORT int i2d_X509_REQ(X509_REQ *req, uint8_t **outp);

// X509_REQ_VERSION_1 is the version constant for |X509_REQ| objects. No other
// versions are defined.
#define X509_REQ_VERSION_1 0

// X509_REQ_get_version returns the numerical value of |req|'s version. This
// will always be |X509_REQ_VERSION_1| for valid CSRs. For compatibility,
// |d2i_X509_REQ| also accepts some invalid version numbers, in which case this
// function may return other values.
OPENSSL_EXPORT long X509_REQ_get_version(const X509_REQ *req);

// X509_REQ_get_subject_name returns |req|'s subject name. Note this function is
// not const-correct for legacy reasons.
OPENSSL_EXPORT X509_NAME *X509_REQ_get_subject_name(const X509_REQ *req);

// X509_REQ_get0_pubkey returns |req|'s public key as an |EVP_PKEY|, or NULL if
// the public key was unsupported or could not be decoded. The |EVP_PKEY| is
// cached in |req|, so callers must not mutate the result.
OPENSSL_EXPORT EVP_PKEY *X509_REQ_get0_pubkey(const X509_REQ *req);

// X509_REQ_get_pubkey behaves like |X509_REQ_get0_pubkey| but increments the
// reference count on the |EVP_PKEY|. The caller must release the result with
// |EVP_PKEY_free| when done. The |EVP_PKEY| is cached in |req|, so callers must
// not mutate the result.
OPENSSL_EXPORT EVP_PKEY *X509_REQ_get_pubkey(const X509_REQ *req);

// X509_REQ_check_private_key returns one if |req|'s public key matches |pkey|
// and zero otherwise.
OPENSSL_EXPORT int X509_REQ_check_private_key(const X509_REQ *req,
                                              const EVP_PKEY *pkey);

// X509_REQ_get_attr_count returns the number of attributes in |req|.
OPENSSL_EXPORT int X509_REQ_get_attr_count(const X509_REQ *req);

// X509_REQ_get_attr returns the attribute at index |loc| in |req|, or NULL if
// out of bounds.
OPENSSL_EXPORT X509_ATTRIBUTE *X509_REQ_get_attr(const X509_REQ *req, int loc);

// X509_REQ_get_attr_by_NID returns the index of the attribute in |req| of type
// |nid|, or a negative number if not found. If found, callers can use
// |X509_REQ_get_attr| to look up the attribute by index.
//
// If |lastpos| is non-negative, it begins searching at |lastpos| + 1. Callers
// can thus loop over all matching attributes by first passing -1 and then
// passing the previously-returned value until no match is returned.
OPENSSL_EXPORT int X509_REQ_get_attr_by_NID(const X509_REQ *req, int nid,
                                            int lastpos);

// X509_REQ_get_attr_by_OBJ behaves like |X509_REQ_get_attr_by_NID| but looks
// for attributes of type |obj|.
OPENSSL_EXPORT int X509_REQ_get_attr_by_OBJ(const X509_REQ *req,
                                            const ASN1_OBJECT *obj,
                                            int lastpos);

// X509_REQ_extension_nid returns one if |nid| is a supported CSR attribute type
// for carrying extensions and zero otherwise. The supported types are
// |NID_ext_req| (pkcs-9-at-extensionRequest from RFC 2985) and |NID_ms_ext_req|
// (a Microsoft szOID_CERT_EXTENSIONS variant).
OPENSSL_EXPORT int X509_REQ_extension_nid(int nid);

// X509_REQ_get_extensions decodes the most preferred list of requested
// extensions in |req| and returns a newly-allocated |STACK_OF(X509_EXTENSION)|
// containing the result. It returns NULL on error, or if |req| did not request
// extensions.
//
// CSRs do not store extensions directly. Instead there are attribute types
// which are defined to hold extensions. See |X509_REQ_extension_nid|. This
// function supports both pkcs-9-at-extensionRequest from RFC 2985 and the
// Microsoft szOID_CERT_EXTENSIONS variant. If both are present,
// pkcs-9-at-extensionRequest is preferred.
OPENSSL_EXPORT STACK_OF(X509_EXTENSION) *X509_REQ_get_extensions(
    const X509_REQ *req);

// X509_REQ_get0_signature sets |*out_sig| and |*out_alg| to the signature and
// signature algorithm of |req|, respectively. Either output pointer may be NULL
// to ignore the value.
OPENSSL_EXPORT void X509_REQ_get0_signature(const X509_REQ *req,
                                            const ASN1_BIT_STRING **out_sig,
                                            const X509_ALGOR **out_alg);

// X509_REQ_get_signature_nid returns the NID corresponding to |req|'s signature
// algorithm, or |NID_undef| if the signature algorithm does not correspond to
// a known NID.
OPENSSL_EXPORT int X509_REQ_get_signature_nid(const X509_REQ *req);

// X509_REQ_verify checks that |req| has a valid signature by |pkey|. It returns
// one if the signature is valid and zero otherwise.
OPENSSL_EXPORT int X509_REQ_verify(X509_REQ *req, EVP_PKEY *pkey);

// X509_REQ_get1_email returns a newly-allocated list of NUL-terminated strings
// containing all email addresses in |req|'s subject and all rfc822name names
// in |req|'s subject alternative names. The subject alternative names extension
// is extracted from the result of |X509_REQ_get_extensions|. Email addresses
// which contain embedded NUL bytes are skipped.
//
// On error, or if there are no such email addresses, it returns NULL. When
// done, the caller must release the result with |X509_email_free|.
OPENSSL_EXPORT STACK_OF(OPENSSL_STRING) *X509_REQ_get1_email(
    const X509_REQ *req);


// Issuing certificate requests.
//
// An |X509_REQ| object may also represent an incomplete CSR. Callers may
// construct empty |X509_REQ| objects, fill in fields individually, and finally
// sign the result. The following functions may be used for this purpose.

// X509_REQ_new returns a newly-allocated, empty |X509_REQ| object, or NULL on
// error. This object may be filled in and then signed to construct a CSR.
OPENSSL_EXPORT X509_REQ *X509_REQ_new(void);

// X509_REQ_set_version sets |req|'s version to |version|, which should be
// |X509_REQ_VERSION_1|. It returns one on success and zero on error.
//
// The only defined CSR version is |X509_REQ_VERSION_1|, so there is no need to
// call this function.
OPENSSL_EXPORT int X509_REQ_set_version(X509_REQ *req, long version);

// X509_REQ_set_subject_name sets |req|'s subject to a copy of |name|. It
// returns one on success and zero on error.
OPENSSL_EXPORT int X509_REQ_set_subject_name(X509_REQ *req, X509_NAME *name);

// X509_REQ_set_pubkey sets |req|'s public key to |pkey|. It returns one on
// success and zero on error. This function does not take ownership of |pkey|
// and internally copies and updates reference counts as needed.
OPENSSL_EXPORT int X509_REQ_set_pubkey(X509_REQ *req, EVP_PKEY *pkey);

// X509_REQ_delete_attr removes the attribute at index |loc| in |req|. It
// returns the removed attribute to the caller, or NULL if |loc| was out of
// bounds. If non-NULL, the caller must release the result with
// |X509_ATTRIBUTE_free| when done. It is also safe, but not necessary, to call
// |X509_ATTRIBUTE_free| if the result is NULL.
OPENSSL_EXPORT X509_ATTRIBUTE *X509_REQ_delete_attr(X509_REQ *req, int loc);

// X509_REQ_add1_attr appends a copy of |attr| to |req|'s list of attributes. It
// returns one on success and zero on error.
OPENSSL_EXPORT int X509_REQ_add1_attr(X509_REQ *req,
                                      const X509_ATTRIBUTE *attr);

// X509_REQ_add1_attr_by_OBJ appends a new attribute to |req| with type |obj|.
// It returns one on success and zero on error. The value is determined by
// |X509_ATTRIBUTE_set1_data|.
//
// WARNING: The interpretation of |attrtype|, |data|, and |len| is complex and
// error-prone. See |X509_ATTRIBUTE_set1_data| for details.
OPENSSL_EXPORT int X509_REQ_add1_attr_by_OBJ(X509_REQ *req,
                                             const ASN1_OBJECT *obj,
                                             int attrtype,
                                             const unsigned char *data,
                                             int len);

// X509_REQ_add1_attr_by_NID behaves like |X509_REQ_add1_attr_by_OBJ| except the
// attribute type is determined by |nid|.
OPENSSL_EXPORT int X509_REQ_add1_attr_by_NID(X509_REQ *req, int nid,
                                             int attrtype,
                                             const unsigned char *data,
                                             int len);

// X509_REQ_add1_attr_by_txt behaves like |X509_REQ_add1_attr_by_OBJ| except the
// attribute type is determined by calling |OBJ_txt2obj| with |attrname|.
OPENSSL_EXPORT int X509_REQ_add1_attr_by_txt(X509_REQ *req,
                                             const char *attrname, int attrtype,
                                             const unsigned char *data,
                                             int len);

// X509_REQ_add_extensions_nid adds an attribute to |req| of type |nid|, to
// request the certificate extensions in |exts|. It returns one on success and
// zero on error. |nid| should be |NID_ext_req| or |NID_ms_ext_req|.
OPENSSL_EXPORT int X509_REQ_add_extensions_nid(
    X509_REQ *req, const STACK_OF(X509_EXTENSION) *exts, int nid);

// X509_REQ_add_extensions behaves like |X509_REQ_add_extensions_nid|, using the
// standard |NID_ext_req| for the attribute type.
OPENSSL_EXPORT int X509_REQ_add_extensions(
    X509_REQ *req, const STACK_OF(X509_EXTENSION) *exts);

// X509_REQ_sign signs |req| with |pkey| and replaces the signature algorithm
// and signature fields. It returns the length of the signature on success and
// zero on error. This function uses digest algorithm |md|, or |pkey|'s default
// if NULL. Other signing parameters use |pkey|'s defaults. To customize them,
// use |X509_REQ_sign_ctx|.
OPENSSL_EXPORT int X509_REQ_sign(X509_REQ *req, EVP_PKEY *pkey,
                                 const EVP_MD *md);

// X509_REQ_sign_ctx signs |req| with |ctx| and replaces the signature algorithm
// and signature fields. It returns the length of the signature on success and
// zero on error. The signature algorithm and parameters come from |ctx|, which
// must have been initialized with |EVP_DigestSignInit|. The caller should
// configure the corresponding |EVP_PKEY_CTX| before calling this function.
//
// On success or failure, this function mutates |ctx| and resets it to the empty
// state. Caller should not rely on its contents after the function returns.
OPENSSL_EXPORT int X509_REQ_sign_ctx(X509_REQ *req, EVP_MD_CTX *ctx);

// i2d_re_X509_REQ_tbs serializes the CertificationRequestInfo (see RFC 2986)
// portion of |req|, as described in |i2d_SAMPLE|.
//
// This function re-encodes the CertificationRequestInfo and may not reflect
// |req|'s original encoding. It may be used to manually generate a signature
// for a new certificate request.
OPENSSL_EXPORT int i2d_re_X509_REQ_tbs(X509_REQ *req, uint8_t **outp);

// X509_REQ_set1_signature_algo sets |req|'s signature algorithm to |algo| and
// returns one on success or zero on error.
OPENSSL_EXPORT int X509_REQ_set1_signature_algo(X509_REQ *req,
                                                const X509_ALGOR *algo);

// X509_REQ_set1_signature_value sets |req|'s signature to a copy of the
// |sig_len| bytes pointed by |sig|. It returns one on success and zero on
// error.
//
// Due to a specification error, PKCS#10 certificate requests store signatures
// in ASN.1 BIT STRINGs, but signature algorithms return byte strings rather
// than bit strings. This function creates a BIT STRING containing a whole
// number of bytes, with the bit order matching the DER encoding. This matches
// the encoding used by all X.509 signature algorithms.
OPENSSL_EXPORT int X509_REQ_set1_signature_value(X509_REQ *req,
                                                 const uint8_t *sig,
                                                 size_t sig_len);


// Names.
//
// An |X509_NAME| represents an X.509 Name structure (RFC 5280). X.509 names are
// a complex, hierarchical structure over a collection of attributes. Each name
// is sequence of relative distinguished names (RDNs), decreasing in
// specificity. For example, the first RDN may specify the country, while the
// next RDN may specify a locality. Each RDN is, itself, a set of attributes.
// Having more than one attribute in an RDN is uncommon, but possible. Within an
// RDN, attributes have the same level in specificity. Attribute types are
// OBJECT IDENTIFIERs. This determines the ASN.1 type of the value, which is
// commonly a string but may be other types.
//
// The |X509_NAME| representation flattens this two-level structure into a
// single list of attributes. Each attribute is stored in an |X509_NAME_ENTRY|,
// with also maintains the index of the RDN it is part of, accessible via
// |X509_NAME_ENTRY_set|. This can be used to recover the two-level structure.
//
// X.509 names are largely vestigial. Historically, DNS names were parsed out of
// the subject's common name attribute, but this is deprecated and has since
// moved to the subject alternative name extension. In modern usage, X.509 names
// are primarily opaque identifiers to link a certificate with its issuer.

DEFINE_STACK_OF(X509_NAME_ENTRY)
DEFINE_STACK_OF(X509_NAME)

// X509_NAME is an |ASN1_ITEM| whose ASN.1 type is X.509 Name (RFC 5280) and C
// type is |X509_NAME*|.
DECLARE_ASN1_ITEM(X509_NAME)

// X509_NAME_new returns a new, empty |X509_NAME|, or NULL on error.
OPENSSL_EXPORT X509_NAME *X509_NAME_new(void);

// X509_NAME_free releases memory associated with |name|.
OPENSSL_EXPORT void X509_NAME_free(X509_NAME *name);

// d2i_X509_NAME parses up to |len| bytes from |*inp| as a DER-encoded X.509
// Name (RFC 5280), as described in |d2i_SAMPLE_with_reuse|.
OPENSSL_EXPORT X509_NAME *d2i_X509_NAME(X509_NAME **out, const uint8_t **inp,
                                        long len);

// i2d_X509_NAME marshals |in| as a DER-encoded X.509 Name (RFC 5280), as
// described in |i2d_SAMPLE|.
//
// TODO(https://crbug.com/boringssl/407): This function should be const and
// thread-safe but is currently neither in some cases, notably if |in| was
// mutated.
OPENSSL_EXPORT int i2d_X509_NAME(X509_NAME *in, uint8_t **outp);

// X509_NAME_dup returns a newly-allocated copy of |name|, or NULL on error.
//
// TODO(https://crbug.com/boringssl/407): This function should be const and
// thread-safe but is currently neither in some cases, notably if |name| was
// mutated.
OPENSSL_EXPORT X509_NAME *X509_NAME_dup(X509_NAME *name);

// X509_NAME_cmp compares |a| and |b|'s canonicalized forms. It returns zero if
// they are equal, one if |a| sorts after |b|, -1 if |b| sorts after |a|, and -2
// on error.
//
// TODO(https://crbug.com/boringssl/407): This function is const, but it is not
// always thread-safe, notably if |name| was mutated.
//
// TODO(https://crbug.com/boringssl/355): The -2 return is very inconvenient to
// pass to a sorting function. Can we make this infallible? In the meantime,
// prefer to use this function only for equality checks rather than comparisons.
// Although even the library itself passes this to a sorting function.
OPENSSL_EXPORT int X509_NAME_cmp(const X509_NAME *a, const X509_NAME *b);

// X509_NAME_get0_der marshals |name| as a DER-encoded X.509 Name (RFC 5280). On
// success, it returns one and sets |*out_der| and |*out_der_len| to a buffer
// containing the result. Otherwise, it returns zero. |*out_der| is owned by
// |name| and must not be freed by the caller. It is invalidated after |name| is
// mutated or freed.
//
// Avoid this function and prefer |i2d_X509_NAME|. It is one of the reasons
// |X509_NAME| functions, including this one, are not consistently thread-safe
// or const-correct. Depending on the resolution of
// https://crbug.com/boringssl/407, this function may be removed or cause poor
// performance.
OPENSSL_EXPORT int X509_NAME_get0_der(X509_NAME *name, const uint8_t **out_der,
                                      size_t *out_der_len);

// X509_NAME_set makes a copy of |name|. On success, it frees |*xn|, sets |*xn|
// to the copy, and returns one. Otherwise, it returns zero.
//
// TODO(https://crbug.com/boringssl/407): This function should be const and
// thread-safe but is currently neither in some cases, notably if |name| was
// mutated.
OPENSSL_EXPORT int X509_NAME_set(X509_NAME **xn, X509_NAME *name);

// X509_NAME_entry_count returns the number of entries in |name|.
OPENSSL_EXPORT int X509_NAME_entry_count(const X509_NAME *name);

// X509_NAME_get_index_by_NID returns the zero-based index of the first
// attribute in |name| with type |nid|, or -1 if there is none. |nid| should be
// one of the |NID_*| constants. If |lastpos| is non-negative, it begins
// searching at |lastpos+1|. To search all attributes, pass in -1, not zero.
//
// Indices from this function refer to |X509_NAME|'s flattened representation.
OPENSSL_EXPORT int X509_NAME_get_index_by_NID(const X509_NAME *name, int nid,
                                              int lastpos);

// X509_NAME_get_index_by_OBJ behaves like |X509_NAME_get_index_by_NID| but
// looks for attributes with type |obj|.
OPENSSL_EXPORT int X509_NAME_get_index_by_OBJ(const X509_NAME *name,
                                              const ASN1_OBJECT *obj,
                                              int lastpos);

// X509_NAME_get_entry returns the attribute in |name| at index |loc|, or NULL
// if |loc| is out of range. |loc| is interpreted using |X509_NAME|'s flattened
// representation. This function returns a non-const pointer for OpenSSL
// compatibility, but callers should not mutate the result. Doing so will break
// internal invariants in the library.
OPENSSL_EXPORT X509_NAME_ENTRY *X509_NAME_get_entry(const X509_NAME *name,
                                                    int loc);

// X509_NAME_delete_entry removes and returns the attribute in |name| at index
// |loc|, or NULL if |loc| is out of range. |loc| is interpreted using
// |X509_NAME|'s flattened representation. If the attribute is found, the caller
// is responsible for releasing the result with |X509_NAME_ENTRY_free|.
//
// This function will internally update RDN indices (see |X509_NAME_ENTRY_set|)
// so they continue to be consecutive.
OPENSSL_EXPORT X509_NAME_ENTRY *X509_NAME_delete_entry(X509_NAME *name,
                                                       int loc);

// X509_NAME_add_entry adds a copy of |entry| to |name| and returns one on
// success or zero on error. If |loc| is -1, the entry is appended to |name|.
// Otherwise, it is inserted at index |loc|. If |set| is -1, the entry is added
// to the previous entry's RDN. If it is 0, the entry becomes a singleton RDN.
// If 1, it is added to next entry's RDN.
//
// This function will internally update RDN indices (see |X509_NAME_ENTRY_set|)
// so they continue to be consecutive.
OPENSSL_EXPORT int X509_NAME_add_entry(X509_NAME *name,
                                       const X509_NAME_ENTRY *entry, int loc,
                                       int set);

// X509_NAME_add_entry_by_OBJ adds a new entry to |name| and returns one on
// success or zero on error. The entry's attribute type is |obj|. The entry's
// attribute value is determined by |type|, |bytes|, and |len|, as in
// |X509_NAME_ENTRY_set_data|. The entry's position is determined by |loc| and
// |set| as in |X509_NAME_add_entry|.
OPENSSL_EXPORT int X509_NAME_add_entry_by_OBJ(X509_NAME *name,
                                              const ASN1_OBJECT *obj, int type,
                                              const uint8_t *bytes,
                                              ossl_ssize_t len, int loc,
                                              int set);

// X509_NAME_add_entry_by_NID behaves like |X509_NAME_add_entry_by_OBJ| but sets
// the entry's attribute type to |nid|, which should be one of the |NID_*|
// constants.
OPENSSL_EXPORT int X509_NAME_add_entry_by_NID(X509_NAME *name, int nid,
                                              int type, const uint8_t *bytes,
                                              ossl_ssize_t len, int loc,
                                              int set);

// X509_NAME_add_entry_by_txt behaves like |X509_NAME_add_entry_by_OBJ| but sets
// the entry's attribute type to |field|, which is passed to |OBJ_txt2obj|.
OPENSSL_EXPORT int X509_NAME_add_entry_by_txt(X509_NAME *name,
                                              const char *field, int type,
                                              const uint8_t *bytes,
                                              ossl_ssize_t len, int loc,
                                              int set);

// X509_NAME_ENTRY is an |ASN1_ITEM| whose ASN.1 type is AttributeTypeAndValue
// (RFC 5280) and C type is |X509_NAME_ENTRY*|.
DECLARE_ASN1_ITEM(X509_NAME_ENTRY)

// X509_NAME_ENTRY_new returns a new, empty |X509_NAME_ENTRY|, or NULL on error.
OPENSSL_EXPORT X509_NAME_ENTRY *X509_NAME_ENTRY_new(void);

// X509_NAME_ENTRY_free releases memory associated with |entry|.
OPENSSL_EXPORT void X509_NAME_ENTRY_free(X509_NAME_ENTRY *entry);

// d2i_X509_NAME_ENTRY parses up to |len| bytes from |*inp| as a DER-encoded
// AttributeTypeAndValue (RFC 5280), as described in |d2i_SAMPLE_with_reuse|.
OPENSSL_EXPORT X509_NAME_ENTRY *d2i_X509_NAME_ENTRY(X509_NAME_ENTRY **out,
                                                    const uint8_t **inp,
                                                    long len);

// i2d_X509_NAME_ENTRY marshals |in| as a DER-encoded AttributeTypeAndValue (RFC
// 5280), as described in |i2d_SAMPLE|.
OPENSSL_EXPORT int i2d_X509_NAME_ENTRY(const X509_NAME_ENTRY *in,
                                       uint8_t **outp);

// X509_NAME_ENTRY_dup returns a newly-allocated copy of |entry|, or NULL on
// error.
OPENSSL_EXPORT X509_NAME_ENTRY *X509_NAME_ENTRY_dup(
    const X509_NAME_ENTRY *entry);

// X509_NAME_ENTRY_get_object returns |entry|'s attribute type. This function
// returns a non-const pointer for OpenSSL compatibility, but callers should not
// mutate the result. Doing so will break internal invariants in the library.
OPENSSL_EXPORT ASN1_OBJECT *X509_NAME_ENTRY_get_object(
    const X509_NAME_ENTRY *entry);

// X509_NAME_ENTRY_set_object sets |entry|'s attribute type to |obj|. It returns
// one on success and zero on error.
OPENSSL_EXPORT int X509_NAME_ENTRY_set_object(X509_NAME_ENTRY *entry,
                                              const ASN1_OBJECT *obj);

// X509_NAME_ENTRY_get_data returns |entry|'s attribute value, represented as an
// |ASN1_STRING|. This value may have any ASN.1 type, so callers must check the
// type before interpreting the contents. This function returns a non-const
// pointer for OpenSSL compatibility, but callers should not mutate the result.
// Doing so will break internal invariants in the library.
//
// TODO(https://crbug.com/boringssl/412): Although the spec says any ASN.1 type
// is allowed, we currently only allow an ad-hoc set of types. Additionally, it
// is unclear if some types can even be represented by this function.
OPENSSL_EXPORT ASN1_STRING *X509_NAME_ENTRY_get_data(
    const X509_NAME_ENTRY *entry);

// X509_NAME_ENTRY_set_data sets |entry|'s value to |len| bytes from |bytes|. It
// returns one on success and zero on error. If |len| is -1, |bytes| must be a
// NUL-terminated C string and the length is determined by |strlen|. |bytes| is
// converted to an ASN.1 type as follows:
//
// If |type| is a |MBSTRING_*| constant, the value is an ASN.1 string. The
// string is determined by decoding |bytes| in the encoding specified by |type|,
// and then re-encoding it in a form appropriate for |entry|'s attribute type.
// See |ASN1_STRING_set_by_NID| for details.
//
// Otherwise, the value is an |ASN1_STRING| with type |type| and value |bytes|.
// See |ASN1_STRING| for how to format ASN.1 types as an |ASN1_STRING|. If
// |type| is |V_ASN1_UNDEF| the previous |ASN1_STRING| type is reused.
OPENSSL_EXPORT int X509_NAME_ENTRY_set_data(X509_NAME_ENTRY *entry, int type,
                                            const uint8_t *bytes,
                                            ossl_ssize_t len);

// X509_NAME_ENTRY_set returns the zero-based index of the RDN which contains
// |entry|. Consecutive entries with the same index are part of the same RDN.
OPENSSL_EXPORT int X509_NAME_ENTRY_set(const X509_NAME_ENTRY *entry);

// X509_NAME_ENTRY_create_by_OBJ creates a new |X509_NAME_ENTRY| with attribute
// type |obj|. The attribute value is determined from |type|, |bytes|, and |len|
// as in |X509_NAME_ENTRY_set_data|. It returns the |X509_NAME_ENTRY| on success
// and NULL on error.
//
// If |out| is non-NULL and |*out| is NULL, it additionally sets |*out| to the
// result on success. If both |out| and |*out| are non-NULL, it updates the
// object at |*out| instead of allocating a new one.
OPENSSL_EXPORT X509_NAME_ENTRY *X509_NAME_ENTRY_create_by_OBJ(
    X509_NAME_ENTRY **out, const ASN1_OBJECT *obj, int type,
    const uint8_t *bytes, ossl_ssize_t len);

// X509_NAME_ENTRY_create_by_NID behaves like |X509_NAME_ENTRY_create_by_OBJ|
// except the attribute type is |nid|, which should be one of the |NID_*|
// constants.
OPENSSL_EXPORT X509_NAME_ENTRY *X509_NAME_ENTRY_create_by_NID(
    X509_NAME_ENTRY **out, int nid, int type, const uint8_t *bytes,
    ossl_ssize_t len);

// X509_NAME_ENTRY_create_by_txt behaves like |X509_NAME_ENTRY_create_by_OBJ|
// except the attribute type is |field|, which is passed to |OBJ_txt2obj|.
OPENSSL_EXPORT X509_NAME_ENTRY *X509_NAME_ENTRY_create_by_txt(
    X509_NAME_ENTRY **out, const char *field, int type, const uint8_t *bytes,
    ossl_ssize_t len);


// Public keys.
//
// X.509 encodes public keys as SubjectPublicKeyInfo (RFC 5280), sometimes
// referred to as SPKI. These are represented in this library by |X509_PUBKEY|.

// X509_PUBKEY is an |ASN1_ITEM| whose ASN.1 type is SubjectPublicKeyInfo and C
// type is |X509_PUBKEY*|.
DECLARE_ASN1_ITEM(X509_PUBKEY)

// X509_PUBKEY_new returns a newly-allocated, empty |X509_PUBKEY| object, or
// NULL on error.
OPENSSL_EXPORT X509_PUBKEY *X509_PUBKEY_new(void);

// X509_PUBKEY_free releases memory associated with |key|.
OPENSSL_EXPORT void X509_PUBKEY_free(X509_PUBKEY *key);

// d2i_X509_PUBKEY parses up to |len| bytes from |*inp| as a DER-encoded
// SubjectPublicKeyInfo, as described in |d2i_SAMPLE|.
OPENSSL_EXPORT X509_PUBKEY *d2i_X509_PUBKEY(X509_PUBKEY **out,
                                            const uint8_t **inp, long len);

// i2d_X509_PUBKEY marshals |key| as a DER-encoded SubjectPublicKeyInfo, as
// described in |i2d_SAMPLE|.
OPENSSL_EXPORT int i2d_X509_PUBKEY(const X509_PUBKEY *key, uint8_t **outp);

// X509_PUBKEY_set serializes |pkey| into a newly-allocated |X509_PUBKEY|
// structure. On success, it frees |*x| if non-NULL, then sets |*x| to the new
// object, and returns one. Otherwise, it returns zero.
OPENSSL_EXPORT int X509_PUBKEY_set(X509_PUBKEY **x, EVP_PKEY *pkey);

// X509_PUBKEY_get0 returns |key| as an |EVP_PKEY|, or NULL if |key| either
// could not be parsed or is an unrecognized algorithm. The |EVP_PKEY| is cached
// in |key|, so callers must not mutate the result.
OPENSSL_EXPORT EVP_PKEY *X509_PUBKEY_get0(const X509_PUBKEY *key);

// X509_PUBKEY_get behaves like |X509_PUBKEY_get0| but increments the reference
// count on the |EVP_PKEY|. The caller must release the result with
// |EVP_PKEY_free| when done. The |EVP_PKEY| is cached in |key|, so callers must
// not mutate the result.
OPENSSL_EXPORT EVP_PKEY *X509_PUBKEY_get(const X509_PUBKEY *key);

// X509_PUBKEY_set0_param sets |pub| to a key with AlgorithmIdentifier
// determined by |obj|, |param_type|, and |param_value|, and an encoded
// public key of |key|. On success, it gives |pub| ownership of all the other
// parameters and returns one. Otherwise, it returns zero. |key| must have been
// allocated by |OPENSSL_malloc|. |obj| and, if applicable, |param_value| must
// not be freed after a successful call, and must have been allocated in a
// manner compatible with |ASN1_OBJECT_free| or |ASN1_STRING_free|.
//
// |obj|, |param_type|, and |param_value| are interpreted as in
// |X509_ALGOR_set0|. See |X509_ALGOR_set0| for details.
OPENSSL_EXPORT int X509_PUBKEY_set0_param(X509_PUBKEY *pub, ASN1_OBJECT *obj,
                                          int param_type, void *param_value,
                                          uint8_t *key, int key_len);

// X509_PUBKEY_get0_param outputs fields of |pub| and returns one. If |out_obj|
// is not NULL, it sets |*out_obj| to AlgorithmIdentifier's OID. If |out_key|
// is not NULL, it sets |*out_key| and |*out_key_len| to the encoded public key.
// If |out_alg| is not NULL, it sets |*out_alg| to the AlgorithmIdentifier.
//
// All pointers outputted by this function are internal to |pub| and must not be
// freed by the caller. Additionally, although some outputs are non-const,
// callers must not mutate the resulting objects.
//
// Note: X.509 SubjectPublicKeyInfo structures store the encoded public key as a
// BIT STRING. |*out_key| and |*out_key_len| will silently pad the key with zero
// bits if |pub| did not contain a whole number of bytes. Use
// |X509_PUBKEY_get0_public_key| to preserve this information.
OPENSSL_EXPORT int X509_PUBKEY_get0_param(ASN1_OBJECT **out_obj,
                                          const uint8_t **out_key,
                                          int *out_key_len,
                                          X509_ALGOR **out_alg,
                                          X509_PUBKEY *pub);

// X509_PUBKEY_get0_public_key returns |pub|'s encoded public key.
OPENSSL_EXPORT const ASN1_BIT_STRING *X509_PUBKEY_get0_public_key(
    const X509_PUBKEY *pub);


// Extensions.
//
// X.509 certificates and CRLs may contain a list of extensions (RFC 5280).
// Extensions have a type, specified by an object identifier (|ASN1_OBJECT|) and
// a byte string value, which should a DER-encoded structure whose type is
// determined by the extension type. This library represents extensions with the
// |X509_EXTENSION| type.

// X509_EXTENSION is an |ASN1_ITEM| whose ASN.1 type is X.509 Extension (RFC
// 5280) and C type is |X509_EXTENSION*|.
DECLARE_ASN1_ITEM(X509_EXTENSION)

// X509_EXTENSION_new returns a newly-allocated, empty |X509_EXTENSION| object
// or NULL on error.
OPENSSL_EXPORT X509_EXTENSION *X509_EXTENSION_new(void);

// X509_EXTENSION_free releases memory associated with |ex|.
OPENSSL_EXPORT void X509_EXTENSION_free(X509_EXTENSION *ex);

// d2i_X509_EXTENSION parses up to |len| bytes from |*inp| as a DER-encoded
// X.509 Extension (RFC 5280), as described in |d2i_SAMPLE_with_reuse|.
OPENSSL_EXPORT X509_EXTENSION *d2i_X509_EXTENSION(X509_EXTENSION **out,
                                                  const uint8_t **inp,
                                                  long len);

// i2d_X509_EXTENSION marshals |ex| as a DER-encoded X.509 Extension (RFC
// 5280), as described in |i2d_SAMPLE|.
OPENSSL_EXPORT int i2d_X509_EXTENSION(const X509_EXTENSION *ex, uint8_t **outp);

// X509_EXTENSION_dup returns a newly-allocated copy of |ex|, or NULL on error.
// This function works by serializing the structure, so if |ex| is incomplete,
// it may fail.
OPENSSL_EXPORT X509_EXTENSION *X509_EXTENSION_dup(const X509_EXTENSION *ex);

// X509_EXTENSION_create_by_NID creates a new |X509_EXTENSION| with type |nid|,
// value |data|, and critical bit |crit|. It returns an |X509_EXTENSION| on
// success, and NULL on error. |nid| should be a |NID_*| constant.
//
// If |ex| and |*ex| are both non-NULL, |*ex| is used to hold the result,
// otherwise a new object is allocated. If |ex| is non-NULL and |*ex| is NULL,
// the function sets |*ex| to point to the newly allocated result, in addition
// to returning the result.
OPENSSL_EXPORT X509_EXTENSION *X509_EXTENSION_create_by_NID(
    X509_EXTENSION **ex, int nid, int crit, const ASN1_OCTET_STRING *data);

// X509_EXTENSION_create_by_OBJ behaves like |X509_EXTENSION_create_by_NID|, but
// the extension type is determined by an |ASN1_OBJECT|.
OPENSSL_EXPORT X509_EXTENSION *X509_EXTENSION_create_by_OBJ(
    X509_EXTENSION **ex, const ASN1_OBJECT *obj, int crit,
    const ASN1_OCTET_STRING *data);

// X509_EXTENSION_get_object returns |ex|'s extension type. This function
// returns a non-const pointer for OpenSSL compatibility, but callers should not
// mutate the result.
OPENSSL_EXPORT ASN1_OBJECT *X509_EXTENSION_get_object(const X509_EXTENSION *ex);

// X509_EXTENSION_get_data returns |ne|'s extension value. This function returns
// a non-const pointer for OpenSSL compatibility, but callers should not mutate
// the result.
OPENSSL_EXPORT ASN1_OCTET_STRING *X509_EXTENSION_get_data(
    const X509_EXTENSION *ne);

// X509_EXTENSION_get_critical returns one if |ex| is critical and zero
// otherwise.
OPENSSL_EXPORT int X509_EXTENSION_get_critical(const X509_EXTENSION *ex);

// X509_EXTENSION_set_object sets |ex|'s extension type to |obj|. It returns one
// on success and zero on error.
OPENSSL_EXPORT int X509_EXTENSION_set_object(X509_EXTENSION *ex,
                                             const ASN1_OBJECT *obj);

// X509_EXTENSION_set_critical sets |ex| to critical if |crit| is non-zero and
// to non-critical if |crit| is zero.
OPENSSL_EXPORT int X509_EXTENSION_set_critical(X509_EXTENSION *ex, int crit);

// X509_EXTENSION_set_data set's |ex|'s extension value to a copy of |data|. It
// returns one on success and zero on error.
OPENSSL_EXPORT int X509_EXTENSION_set_data(X509_EXTENSION *ex,
                                           const ASN1_OCTET_STRING *data);


// Extension lists.
//
// The following functions manipulate lists of extensions. Most of them have
// corresponding functions on the containing |X509|, |X509_CRL|, or
// |X509_REVOKED|.

DEFINE_STACK_OF(X509_EXTENSION)
typedef STACK_OF(X509_EXTENSION) X509_EXTENSIONS;

// X509_EXTENSIONS is an |ASN1_ITEM| whose ASN.1 type is SEQUENCE of Extension
// (RFC 5280) and C type is |STACK_OF(X509_EXTENSION)*|.
DECLARE_ASN1_ITEM(X509_EXTENSIONS)

// d2i_X509_EXTENSIONS parses up to |len| bytes from |*inp| as a DER-encoded
// SEQUENCE OF Extension (RFC 5280), as described in |d2i_SAMPLE_with_reuse|.
OPENSSL_EXPORT X509_EXTENSIONS *d2i_X509_EXTENSIONS(X509_EXTENSIONS **out,
                                                    const uint8_t **inp,
                                                    long len);

// i2d_X509_EXTENSIONS marshals |alg| as a DER-encoded SEQUENCE OF Extension
// (RFC 5280), as described in |i2d_SAMPLE|.
OPENSSL_EXPORT int i2d_X509_EXTENSIONS(const X509_EXTENSIONS *alg,
                                       uint8_t **outp);

// X509v3_get_ext_count returns the number of extensions in |x|.
OPENSSL_EXPORT int X509v3_get_ext_count(const STACK_OF(X509_EXTENSION) *x);

// X509v3_get_ext_by_NID returns the index of the first extension in |x| with
// type |nid|, or a negative number if not found. If found, callers can use
// |X509v3_get_ext| to look up the extension by index.
//
// If |lastpos| is non-negative, it begins searching at |lastpos| + 1. Callers
// can thus loop over all matching extensions by first passing -1 and then
// passing the previously-returned value until no match is returned.
OPENSSL_EXPORT int X509v3_get_ext_by_NID(const STACK_OF(X509_EXTENSION) *x,
                                         int nid, int lastpos);

// X509v3_get_ext_by_OBJ behaves like |X509v3_get_ext_by_NID| but looks for
// extensions matching |obj|.
OPENSSL_EXPORT int X509v3_get_ext_by_OBJ(const STACK_OF(X509_EXTENSION) *x,
                                         const ASN1_OBJECT *obj, int lastpos);

// X509v3_get_ext_by_critical returns the index of the first extension in |x|
// whose critical bit matches |crit|, or a negative number if no such extension
// was found.
//
// If |lastpos| is non-negative, it begins searching at |lastpos| + 1. Callers
// can thus loop over all matching extensions by first passing -1 and then
// passing the previously-returned value until no match is returned.
OPENSSL_EXPORT int X509v3_get_ext_by_critical(const STACK_OF(X509_EXTENSION) *x,
                                              int crit, int lastpos);

// X509v3_get_ext returns the extension in |x| at index |loc|, or NULL if |loc|
// is out of bounds. This function returns a non-const pointer for OpenSSL
// compatibility, but callers should not mutate the result.
OPENSSL_EXPORT X509_EXTENSION *X509v3_get_ext(const STACK_OF(X509_EXTENSION) *x,
                                              int loc);

// X509v3_delete_ext removes the extension in |x| at index |loc| and returns the
// removed extension, or NULL if |loc| was out of bounds. If an extension was
// returned, the caller must release it with |X509_EXTENSION_free|.
OPENSSL_EXPORT X509_EXTENSION *X509v3_delete_ext(STACK_OF(X509_EXTENSION) *x,
                                                 int loc);

// X509v3_add_ext adds a copy of |ex| to the extension list in |*x|. If |*x| is
// NULL, it allocates a new |STACK_OF(X509_EXTENSION)| to hold the copy and sets
// |*x| to the new list. It returns |*x| on success and NULL on error. The
// caller retains ownership of |ex| and can release it independently of |*x|.
//
// The new extension is inserted at index |loc|, shifting extensions to the
// right. If |loc| is -1 or out of bounds, the new extension is appended to the
// list.
OPENSSL_EXPORT STACK_OF(X509_EXTENSION) *X509v3_add_ext(
    STACK_OF(X509_EXTENSION) **x, const X509_EXTENSION *ex, int loc);


// Built-in extensions.
//
// Several functions in the library encode and decode extension values into a
// C structure to that extension. The following extensions are supported:
//
// - |NID_authority_key_identifier| with type |AUTHORITY_KEYID|
// - |NID_basic_constraints| with type |BASIC_CONSTRAINTS|
// - |NID_certificate_issuer| with type |GENERAL_NAMES|
// - |NID_certificate_policies| with type |CERTIFICATEPOLICIES|
// - |NID_crl_distribution_points| with type |CRL_DIST_POINTS|
// - |NID_crl_number| with type |ASN1_INTEGER|
// - |NID_crl_reason| with type |ASN1_ENUMERATED|
// - |NID_delta_crl| with type |ASN1_INTEGER|
// - |NID_ext_key_usage| with type |EXTENDED_KEY_USAGE|
// - |NID_freshest_crl| with type |ISSUING_DIST_POINT|
// - |NID_id_pkix_OCSP_noCheck| with type |ASN1_NULL|
// - |NID_info_access| with type |AUTHORITY_INFO_ACCESS|
// - |NID_inhibit_any_policy| with type |ASN1_INTEGER|
// - |NID_invalidity_date| with type |ASN1_GENERALIZEDTIME|
// - |NID_issuer_alt_name| with type |GENERAL_NAMES|
// - |NID_issuing_distribution_point| with type |ISSUING_DIST_POINT|
// - |NID_key_usage| with type |ASN1_BIT_STRING|
// - |NID_name_constraints| with type |NAME_CONSTRAINTS|
// - |NID_netscape_base_url| with type |ASN1_IA5STRING|
// - |NID_netscape_ca_policy_url| with type |ASN1_IA5STRING|
// - |NID_netscape_ca_revocation_url| with type |ASN1_IA5STRING|
// - |NID_netscape_cert_type| with type |ASN1_BIT_STRING|
// - |NID_netscape_comment| with type |ASN1_IA5STRING|
// - |NID_netscape_renewal_url| with type |ASN1_IA5STRING|
// - |NID_netscape_revocation_url| with type |ASN1_IA5STRING|
// - |NID_netscape_ssl_server_name| with type |ASN1_IA5STRING|
// - |NID_policy_constraints| with type |POLICY_CONSTRAINTS|
// - |NID_policy_mappings| with type |POLICY_MAPPINGS|
// - |NID_sinfo_access| with type |AUTHORITY_INFO_ACCESS|
// - |NID_subject_alt_name| with type |GENERAL_NAMES|
// - |NID_subject_key_identifier| with type |ASN1_OCTET_STRING|
//
// If an extension does not appear in this list, e.g. for a custom extension,
// callers can instead use functions such as |X509_get_ext_by_OBJ|,
// |X509_EXTENSION_get_data|, and |X509_EXTENSION_create_by_OBJ| to inspect or
// create extensions directly. Although the |X509V3_EXT_METHOD| mechanism allows
// registering custom extensions, doing so is deprecated and may result in
// threading or memory errors.

// X509V3_EXT_d2i decodes |ext| and returns a pointer to a newly-allocated
// structure, with type dependent on the type of the extension. It returns NULL
// if |ext| is an unsupported extension or if there was a syntax error in the
// extension. The caller should cast the return value to the expected type and
// free the structure when done.
//
// WARNING: Casting the return value to the wrong type is a potentially
// exploitable memory error, so callers must not use this function before
// checking |ext| is of a known type. See the list at the top of this section
// for the correct types.
OPENSSL_EXPORT void *X509V3_EXT_d2i(const X509_EXTENSION *ext);

// X509V3_get_d2i finds and decodes the extension in |extensions| of type |nid|.
// If found, it decodes it and returns a newly-allocated structure, with type
// dependent on |nid|. If the extension is not found or on error, it returns
// NULL. The caller may distinguish these cases using the |out_critical| value.
//
// If |out_critical| is not NULL, this function sets |*out_critical| to one if
// the extension is found and critical, zero if it is found and not critical, -1
// if it is not found, and -2 if there is an invalid duplicate extension. Note
// this function may set |*out_critical| to one or zero and still return NULL if
// the extension is found but has a syntax error.
//
// If |out_idx| is not NULL, this function looks for the first occurrence of the
// extension after |*out_idx|. It then sets |*out_idx| to the index of the
// extension, or -1 if not found. If |out_idx| is non-NULL, duplicate extensions
// are not treated as an error. Callers, however, should not rely on this
// behavior as it may be removed in the future. Duplicate extensions are
// forbidden in RFC 5280.
//
// WARNING: This function is difficult to use correctly. Callers should pass a
// non-NULL |out_critical| and check both the return value and |*out_critical|
// to handle errors. If the return value is NULL and |*out_critical| is not -1,
// there was an error. Otherwise, the function succeeded and but may return NULL
// for a missing extension. Callers should pass NULL to |out_idx| so that
// duplicate extensions are handled correctly.
//
// Additionally, casting the return value to the wrong type is a potentially
// exploitable memory error, so callers must ensure the cast and |nid| match.
// See the list at the top of this section for the correct types.
OPENSSL_EXPORT void *X509V3_get_d2i(const STACK_OF(X509_EXTENSION) *extensions,
                                    int nid, int *out_critical, int *out_idx);

// X509V3_EXT_free casts |ext_data| into the type that corresponds to |nid| and
// releases memory associated with it. It returns one on success and zero if
// |nid| is not a known extension.
//
// WARNING: Casting |ext_data| to the wrong type is a potentially exploitable
// memory error, so callers must ensure |ext_data|'s type matches |nid|. See the
// list at the top of this section for the correct types.
//
// TODO(davidben): OpenSSL upstream no longer exposes this function. Remove it?
OPENSSL_EXPORT int X509V3_EXT_free(int nid, void *ext_data);

// X509V3_EXT_i2d casts |ext_struc| into the type that corresponds to
// |ext_nid|, serializes it, and returns a newly-allocated |X509_EXTENSION|
// object containing the serialization, or NULL on error. The |X509_EXTENSION|
// has OID |ext_nid| and is critical if |crit| is one.
//
// WARNING: Casting |ext_struc| to the wrong type is a potentially exploitable
// memory error, so callers must ensure |ext_struct|'s type matches |ext_nid|.
// See the list at the top of this section for the correct types.
OPENSSL_EXPORT X509_EXTENSION *X509V3_EXT_i2d(int ext_nid, int crit,
                                              void *ext_struc);

// The following constants control the behavior of |X509V3_add1_i2d| and related
// functions.

// X509V3_ADD_OP_MASK can be ANDed with the flags to determine how duplicate
// extensions are processed.
#define X509V3_ADD_OP_MASK 0xfL

// X509V3_ADD_DEFAULT causes the function to fail if the extension was already
// present.
#define X509V3_ADD_DEFAULT 0L

// X509V3_ADD_APPEND causes the function to unconditionally appended the new
// extension to to the extensions list, even if there is a duplicate.
#define X509V3_ADD_APPEND 1L

// X509V3_ADD_REPLACE causes the function to replace the existing extension, or
// append if it is not present.
#define X509V3_ADD_REPLACE 2L

// X509V3_ADD_REPLACE_EXISTING causes the function to replace the existing
// extension and fail if it is not present.
#define X509V3_ADD_REPLACE_EXISTING 3L

// X509V3_ADD_KEEP_EXISTING causes the function to succeed without replacing the
// extension if already present.
#define X509V3_ADD_KEEP_EXISTING 4L

// X509V3_ADD_DELETE causes the function to remove the matching extension. No
// new extension is added. If there is no matching extension, the function
// fails. The |value| parameter is ignored in this mode.
#define X509V3_ADD_DELETE 5L

// X509V3_ADD_SILENT may be ORed into one of the values above to indicate the
// function should not add to the error queue on duplicate or missing extension.
// The function will continue to return zero in those cases, and it will
// continue to return -1 and add to the error queue on other errors.
#define X509V3_ADD_SILENT 0x10

// X509V3_add1_i2d casts |value| to the type that corresponds to |nid|,
// serializes it, and appends it to the extension list in |*x|. If |*x| is NULL,
// it will set |*x| to a newly-allocated |STACK_OF(X509_EXTENSION)| as needed.
// The |crit| parameter determines whether the new extension is critical.
// |flags| may be some combination of the |X509V3_ADD_*| constants to control
// the function's behavior on duplicate extension.
//
// This function returns one on success, zero if the operation failed due to a
// missing or duplicate extension, and -1 on other errors.
//
// WARNING: Casting |value| to the wrong type is a potentially exploitable
// memory error, so callers must ensure |value|'s type matches |nid|. See the
// list at the top of this section for the correct types.
OPENSSL_EXPORT int X509V3_add1_i2d(STACK_OF(X509_EXTENSION) **x, int nid,
                                   void *value, int crit, unsigned long flags);


// General names.
//
// A |GENERAL_NAME| represents an X.509 GeneralName structure, defined in RFC
// 5280, Section 4.2.1.6. General names are distinct from names (|X509_NAME|). A
// general name is a CHOICE type which may contain one of several name types,
// most commonly a DNS name or an IP address. General names most commonly appear
// in the subject alternative name (SAN) extension, though they are also used in
// other extensions.
//
// Many extensions contain a SEQUENCE OF GeneralName, or GeneralNames, so
// |STACK_OF(GENERAL_NAME)| is defined and aliased to |GENERAL_NAMES|.

typedef struct otherName_st {
  ASN1_OBJECT *type_id;
  ASN1_TYPE *value;
} OTHERNAME;

typedef struct EDIPartyName_st {
  ASN1_STRING *nameAssigner;
  ASN1_STRING *partyName;
} EDIPARTYNAME;

// GEN_* are constants for the |type| field of |GENERAL_NAME|, defined below.
#define GEN_OTHERNAME 0
#define GEN_EMAIL 1
#define GEN_DNS 2
#define GEN_X400 3
#define GEN_DIRNAME 4
#define GEN_EDIPARTY 5
#define GEN_URI 6
#define GEN_IPADD 7
#define GEN_RID 8

// A GENERAL_NAME_st, aka |GENERAL_NAME|, represents an X.509 GeneralName. The
// |type| field determines which member of |d| is active. A |GENERAL_NAME| may
// also be empty, in which case |type| is -1 and |d| is NULL. Empty
// |GENERAL_NAME|s are invalid and will never be returned from the parser, but
// may be created temporarily, e.g. by |GENERAL_NAME_new|.
struct GENERAL_NAME_st {
  int type;
  union {
    char *ptr;
    OTHERNAME *otherName;
    ASN1_IA5STRING *rfc822Name;
    ASN1_IA5STRING *dNSName;
    ASN1_STRING *x400Address;
    X509_NAME *directoryName;
    EDIPARTYNAME *ediPartyName;
    ASN1_IA5STRING *uniformResourceIdentifier;
    ASN1_OCTET_STRING *iPAddress;
    ASN1_OBJECT *registeredID;

    // Old names
    ASN1_OCTET_STRING *ip;  // iPAddress
    X509_NAME *dirn;        // dirn
    ASN1_IA5STRING *ia5;    // rfc822Name, dNSName, uniformResourceIdentifier
    ASN1_OBJECT *rid;       // registeredID
  } d;
} /* GENERAL_NAME */;

// GENERAL_NAME_new returns a new, empty |GENERAL_NAME|, or NULL on error.
OPENSSL_EXPORT GENERAL_NAME *GENERAL_NAME_new(void);

// GENERAL_NAME_free releases memory associated with |gen|.
OPENSSL_EXPORT void GENERAL_NAME_free(GENERAL_NAME *gen);

// d2i_GENERAL_NAME parses up to |len| bytes from |*inp| as a DER-encoded X.509
// GeneralName (RFC 5280), as described in |d2i_SAMPLE|.
OPENSSL_EXPORT GENERAL_NAME *d2i_GENERAL_NAME(GENERAL_NAME **out,
                                              const uint8_t **inp, long len);

// i2d_GENERAL_NAME marshals |in| as a DER-encoded X.509 GeneralName (RFC 5280),
// as described in |i2d_SAMPLE|.
//
// TODO(https://crbug.com/boringssl/407): This function should be const and
// thread-safe but is currently neither in some cases, notably if |in| is an
// directoryName and the |X509_NAME| has been modified.
OPENSSL_EXPORT int i2d_GENERAL_NAME(GENERAL_NAME *in, uint8_t **outp);

// GENERAL_NAME_dup returns a newly-allocated copy of |gen|, or NULL on error.
// This function works by serializing the structure, so it will fail if |gen| is
// empty.
//
// TODO(https://crbug.com/boringssl/407): This function should be const and
// thread-safe but is currently neither in some cases, notably if |gen| is an
// directoryName and the |X509_NAME| has been modified.
OPENSSL_EXPORT GENERAL_NAME *GENERAL_NAME_dup(GENERAL_NAME *gen);

// GENERAL_NAMES_new returns a new, empty |GENERAL_NAMES|, or NULL on error.
OPENSSL_EXPORT GENERAL_NAMES *GENERAL_NAMES_new(void);

// GENERAL_NAMES_free releases memory associated with |gens|.
OPENSSL_EXPORT void GENERAL_NAMES_free(GENERAL_NAMES *gens);

// d2i_GENERAL_NAMES parses up to |len| bytes from |*inp| as a DER-encoded
// SEQUENCE OF GeneralName, as described in |d2i_SAMPLE|.
OPENSSL_EXPORT GENERAL_NAMES *d2i_GENERAL_NAMES(GENERAL_NAMES **out,
                                                const uint8_t **inp, long len);

// i2d_GENERAL_NAMES marshals |in| as a DER-encoded SEQUENCE OF GeneralName, as
// described in |i2d_SAMPLE|.
//
// TODO(https://crbug.com/boringssl/407): This function should be const and
// thread-safe but is currently neither in some cases, notably if some element
// of |in| is an directoryName and the |X509_NAME| has been modified.
OPENSSL_EXPORT int i2d_GENERAL_NAMES(GENERAL_NAMES *in, uint8_t **outp);

// OTHERNAME_new returns a new, empty |OTHERNAME|, or NULL on error.
OPENSSL_EXPORT OTHERNAME *OTHERNAME_new(void);

// OTHERNAME_free releases memory associated with |name|.
OPENSSL_EXPORT void OTHERNAME_free(OTHERNAME *name);

// EDIPARTYNAME_new returns a new, empty |EDIPARTYNAME|, or NULL on error.
// EDIPartyName is rarely used in practice, so callers are unlikely to need this
// function.
OPENSSL_EXPORT EDIPARTYNAME *EDIPARTYNAME_new(void);

// EDIPARTYNAME_free releases memory associated with |name|. EDIPartyName is
// rarely used in practice, so callers are unlikely to need this function.
OPENSSL_EXPORT void EDIPARTYNAME_free(EDIPARTYNAME *name);

// GENERAL_NAME_set0_value set |gen|'s type and value to |type| and |value|.
// |type| must be a |GEN_*| constant and |value| must be an object of the
// corresponding type. |gen| takes ownership of |value|, so |value| must have
// been an allocated object.
//
// WARNING: |gen| must be empty (typically as returned from |GENERAL_NAME_new|)
// before calling this function. If |gen| already contained a value, the
// previous contents will be leaked.
OPENSSL_EXPORT void GENERAL_NAME_set0_value(GENERAL_NAME *gen, int type,
                                            void *value);

// GENERAL_NAME_get0_value returns the in-memory representation of |gen|'s
// contents and, |out_type| is not NULL, sets |*out_type| to the type of |gen|,
// which will be a |GEN_*| constant. If |gen| is incomplete, the return value
// will be NULL and the type will be -1.
//
// WARNING: Casting the result of this function to the wrong type is a
// potentially exploitable memory error. Callers must check |gen|'s type, either
// via |*out_type| or checking |gen->type| directly, before inspecting the
// result.
//
// WARNING: This function is not const-correct. The return value should be
// const. Callers shoudl not mutate the returned object.
OPENSSL_EXPORT void *GENERAL_NAME_get0_value(const GENERAL_NAME *gen,
                                             int *out_type);

// GENERAL_NAME_set0_othername sets |gen| to be an OtherName with type |oid| and
// value |value|. On success, it returns one and takes ownership of |oid| and
// |value|, which must be created in a way compatible with |ASN1_OBJECT_free|
// and |ASN1_TYPE_free|, respectively. On allocation failure, it returns zero.
// In the failure case, the caller retains ownership of |oid| and |value| and
// must release them when done.
//
// WARNING: |gen| must be empty (typically as returned from |GENERAL_NAME_new|)
// before calling this function. If |gen| already contained a value, the
// previously contents will be leaked.
OPENSSL_EXPORT int GENERAL_NAME_set0_othername(GENERAL_NAME *gen,
                                               ASN1_OBJECT *oid,
                                               ASN1_TYPE *value);

// GENERAL_NAME_get0_otherName, if |gen| is an OtherName, sets |*out_oid| and
// |*out_value| to the OtherName's type-id and value, respectively, and returns
// one. If |gen| is not an OtherName, it returns zero and leaves |*out_oid| and
// |*out_value| unmodified. Either of |out_oid| or |out_value| may be NULL to
// ignore the value.
//
// WARNING: This function is not const-correct. |out_oid| and |out_value| are
// not const, but callers should not mutate the resulting objects.
OPENSSL_EXPORT int GENERAL_NAME_get0_otherName(const GENERAL_NAME *gen,
                                               ASN1_OBJECT **out_oid,
                                               ASN1_TYPE **out_value);


// Algorithm identifiers.
//
// An |X509_ALGOR| represents an AlgorithmIdentifier structure, used in X.509
// to represent signature algorithms and public key algorithms.

DEFINE_STACK_OF(X509_ALGOR)

// X509_ALGOR is an |ASN1_ITEM| whose ASN.1 type is AlgorithmIdentifier and C
// type is |X509_ALGOR*|.
DECLARE_ASN1_ITEM(X509_ALGOR)

// X509_ALGOR_new returns a newly-allocated, empty |X509_ALGOR| object, or NULL
// on error.
OPENSSL_EXPORT X509_ALGOR *X509_ALGOR_new(void);

// X509_ALGOR_dup returns a newly-allocated copy of |alg|, or NULL on error.
// This function works by serializing the structure, so if |alg| is incomplete,
// it may fail.
OPENSSL_EXPORT X509_ALGOR *X509_ALGOR_dup(const X509_ALGOR *alg);

// X509_ALGOR_free releases memory associated with |alg|.
OPENSSL_EXPORT void X509_ALGOR_free(X509_ALGOR *alg);

// d2i_X509_ALGOR parses up to |len| bytes from |*inp| as a DER-encoded
// AlgorithmIdentifier, as described in |d2i_SAMPLE_with_reuse|.
OPENSSL_EXPORT X509_ALGOR *d2i_X509_ALGOR(X509_ALGOR **out, const uint8_t **inp,
                                          long len);

// i2d_X509_ALGOR marshals |alg| as a DER-encoded AlgorithmIdentifier, as
// described in |i2d_SAMPLE|.
OPENSSL_EXPORT int i2d_X509_ALGOR(const X509_ALGOR *alg, uint8_t **outp);

// X509_ALGOR_set0 sets |alg| to an AlgorithmIdentifier with algorithm |obj| and
// parameter determined by |param_type| and |param_value|. It returns one on
// success and zero on error. This function takes ownership of |obj| and
// |param_value| on success.
//
// If |param_type| is |V_ASN1_UNDEF|, the parameter is omitted. If |param_type|
// is zero, the parameter is left unchanged. Otherwise, |param_type| and
// |param_value| are interpreted as in |ASN1_TYPE_set|.
//
// Note omitting the parameter (|V_ASN1_UNDEF|) and encoding an explicit NULL
// value (|V_ASN1_NULL|) are different. Some algorithms require one and some the
// other. Consult the relevant specification before calling this function. The
// correct parameter for an RSASSA-PKCS1-v1_5 signature is |V_ASN1_NULL|. The
// correct one for an ECDSA or Ed25519 signature is |V_ASN1_UNDEF|.
OPENSSL_EXPORT int X509_ALGOR_set0(X509_ALGOR *alg, ASN1_OBJECT *obj,
                                   int param_type, void *param_value);

// X509_ALGOR_get0 sets |*out_obj| to the |alg|'s algorithm. If |alg|'s
// parameter is omitted, it sets |*out_param_type| and |*out_param_value| to
// |V_ASN1_UNDEF| and NULL. Otherwise, it sets |*out_param_type| and
// |*out_param_value| to the parameter, using the same representation as
// |ASN1_TYPE_set0|. See |ASN1_TYPE_set0| and |ASN1_TYPE| for details.
//
// Callers that require the parameter in serialized form should, after checking
// for |V_ASN1_UNDEF|, use |ASN1_TYPE_set1| and |d2i_ASN1_TYPE|, rather than
// inspecting |*out_param_value|.
//
// Each of |out_obj|, |out_param_type|, and |out_param_value| may be NULL to
// ignore the output. If |out_param_type| is NULL, |out_param_value| is ignored.
//
// WARNING: If |*out_param_type| is set to |V_ASN1_UNDEF|, OpenSSL and older
// revisions of BoringSSL leave |*out_param_value| unset rather than setting it
// to NULL. Callers that support both OpenSSL and BoringSSL should not assume
// |*out_param_value| is uniformly initialized.
OPENSSL_EXPORT void X509_ALGOR_get0(const ASN1_OBJECT **out_obj,
                                    int *out_param_type,
                                    const void **out_param_value,
                                    const X509_ALGOR *alg);

// X509_ALGOR_set_md sets |alg| to the hash function |md|. Note this
// AlgorithmIdentifier represents the hash function itself, not a signature
// algorithm that uses |md|. It returns one on success and zero on error.
//
// Due to historical specification mistakes (see Section 2.1 of RFC 4055), the
// parameters field is sometimes omitted and sometimes a NULL value. When used
// in RSASSA-PSS and RSAES-OAEP, it should be a NULL value. In other contexts,
// the parameters should be omitted. This function assumes the caller is
// constructing a RSASSA-PSS or RSAES-OAEP AlgorithmIdentifier and includes a
// NULL parameter. This differs from OpenSSL's behavior.
//
// TODO(davidben): Rename this function, or perhaps just add a bespoke API for
// constructing PSS and move on.
OPENSSL_EXPORT int X509_ALGOR_set_md(X509_ALGOR *alg, const EVP_MD *md);

// X509_ALGOR_cmp returns zero if |a| and |b| are equal, and some non-zero value
// otherwise. Note this function can only be used for equality checks, not an
// ordering.
OPENSSL_EXPORT int X509_ALGOR_cmp(const X509_ALGOR *a, const X509_ALGOR *b);


// Attributes.
//
// Unlike certificates and CRLs, CSRs use a separate Attribute structure (RFC
// 2985, RFC 2986) for extensibility. This is represented by the library as
// |X509_ATTRIBUTE|.

DEFINE_STACK_OF(X509_ATTRIBUTE)

// X509_ATTRIBUTE is an |ASN1_ITEM| whose ASN.1 type is Attribute (RFC 2986) and
// C type is |X509_ATTRIBUTE*|.
DECLARE_ASN1_ITEM(X509_ATTRIBUTE)

// X509_ATTRIBUTE_new returns a newly-allocated, empty |X509_ATTRIBUTE| object,
// or NULL on error. |X509_ATTRIBUTE_set1_*| may be used to finish initializing
// it.
OPENSSL_EXPORT X509_ATTRIBUTE *X509_ATTRIBUTE_new(void);

// X509_ATTRIBUTE_dup returns a newly-allocated copy of |attr|, or NULL on
// error. This function works by serializing the structure, so if |attr| is
// incomplete, it may fail.
OPENSSL_EXPORT X509_ATTRIBUTE *X509_ATTRIBUTE_dup(const X509_ATTRIBUTE *attr);

// X509_ATTRIBUTE_free releases memory associated with |attr|.
OPENSSL_EXPORT void X509_ATTRIBUTE_free(X509_ATTRIBUTE *attr);

// d2i_X509_ATTRIBUTE parses up to |len| bytes from |*inp| as a DER-encoded
// Attribute (RFC 2986), as described in |d2i_SAMPLE|.
OPENSSL_EXPORT X509_ATTRIBUTE *d2i_X509_ATTRIBUTE(X509_ATTRIBUTE **out,
                                                  const uint8_t **inp,
                                                  long len);

// i2d_X509_ATTRIBUTE marshals |alg| as a DER-encoded Attribute (RFC 2986), as
// described in |i2d_SAMPLE|.
OPENSSL_EXPORT int i2d_X509_ATTRIBUTE(const X509_ATTRIBUTE *alg,
                                      uint8_t **outp);

// X509_ATTRIBUTE_create returns a newly-allocated |X509_ATTRIBUTE|, or NULL on
// error. The attribute has type |nid| and contains a single value determined by
// |attrtype| and |value|, which are interpreted as in |ASN1_TYPE_set|. Note
// this function takes ownership of |value|.
OPENSSL_EXPORT X509_ATTRIBUTE *X509_ATTRIBUTE_create(int nid, int attrtype,
                                                     void *value);

// X509_ATTRIBUTE_create_by_NID returns a newly-allocated |X509_ATTRIBUTE| of
// type |nid|, or NULL on error. The value is determined as in
// |X509_ATTRIBUTE_set1_data|.
//
// If |attr| is non-NULL, the resulting |X509_ATTRIBUTE| is also written to
// |*attr|. If |*attr| was non-NULL when the function was called, |*attr| is
// reused instead of creating a new object.
//
// WARNING: The interpretation of |attrtype|, |data|, and |len| is complex and
// error-prone. See |X509_ATTRIBUTE_set1_data| for details.
//
// WARNING: The object reuse form is deprecated and may be removed in the
// future. It also currently incorrectly appends to the reused object's value
// set rather than overwriting it.
OPENSSL_EXPORT X509_ATTRIBUTE *X509_ATTRIBUTE_create_by_NID(
    X509_ATTRIBUTE **attr, int nid, int attrtype, const void *data, int len);

// X509_ATTRIBUTE_create_by_OBJ behaves like |X509_ATTRIBUTE_create_by_NID|
// except the attribute's type is determined by |obj|.
OPENSSL_EXPORT X509_ATTRIBUTE *X509_ATTRIBUTE_create_by_OBJ(
    X509_ATTRIBUTE **attr, const ASN1_OBJECT *obj, int attrtype,
    const void *data, int len);

// X509_ATTRIBUTE_create_by_txt behaves like |X509_ATTRIBUTE_create_by_NID|
// except the attribute's type is determined by calling |OBJ_txt2obj| with
// |attrname|.
OPENSSL_EXPORT X509_ATTRIBUTE *X509_ATTRIBUTE_create_by_txt(
    X509_ATTRIBUTE **attr, const char *attrname, int type,
    const unsigned char *bytes, int len);

// X509_ATTRIBUTE_set1_object sets |attr|'s type to |obj|. It returns one on
// success and zero on error.
OPENSSL_EXPORT int X509_ATTRIBUTE_set1_object(X509_ATTRIBUTE *attr,
                                              const ASN1_OBJECT *obj);

// X509_ATTRIBUTE_set1_data appends a value to |attr|'s value set and returns
// one on success or zero on error. The value is determined as follows:
//
// If |attrtype| is zero, this function returns one and does nothing. This form
// may be used when calling |X509_ATTRIBUTE_create_by_*| to create an attribute
// with an empty value set. Such attributes are invalid, but OpenSSL supports
// creating them.
//
// Otherwise, if |attrtype| is a |MBSTRING_*| constant, the value is an ASN.1
// string. The string is determined by decoding |len| bytes from |data| in the
// encoding specified by |attrtype|, and then re-encoding it in a form
// appropriate for |attr|'s type. If |len| is -1, |strlen(data)| is used
// instead. See |ASN1_STRING_set_by_NID| for details.
//
// Otherwise, if |len| is not -1, the value is an ASN.1 string. |attrtype| is an
// |ASN1_STRING| type value and the |len| bytes from |data| are copied as the
// type-specific representation of |ASN1_STRING|. See |ASN1_STRING| for details.
//
// Otherwise, if |len| is -1, the value is constructed by passing |attrtype| and
// |data| to |ASN1_TYPE_set1|. That is, |attrtype| is an |ASN1_TYPE| type value,
// and |data| is cast to the corresponding pointer type.
//
// WARNING: Despite the name, this function appends to |attr|'s value set,
// rather than overwriting it. To overwrite the value set, create a new
// |X509_ATTRIBUTE| with |X509_ATTRIBUTE_new|.
//
// WARNING: If using the |MBSTRING_*| form, pass a length rather than relying on
// |strlen|. In particular, |strlen| will not behave correctly if the input is
// |MBSTRING_BMP| or |MBSTRING_UNIV|.
//
// WARNING: This function currently misinterprets |V_ASN1_OTHER| as an
// |MBSTRING_*| constant. This matches OpenSSL but means it is impossible to
// construct a value with a non-universal tag.
OPENSSL_EXPORT int X509_ATTRIBUTE_set1_data(X509_ATTRIBUTE *attr, int attrtype,
                                            const void *data, int len);

// X509_ATTRIBUTE_get0_data returns the |idx|th value of |attr| in a
// type-specific representation to |attrtype|, or NULL if out of bounds or the
// type does not match. |attrtype| is one of the type values in |ASN1_TYPE|. On
// match, the return value uses the same representation as |ASN1_TYPE_set0|. See
// |ASN1_TYPE| for details.
OPENSSL_EXPORT void *X509_ATTRIBUTE_get0_data(X509_ATTRIBUTE *attr, int idx,
                                              int attrtype, void *unused);

// X509_ATTRIBUTE_count returns the number of values in |attr| or 0 if |attr|
// is NULL.
OPENSSL_EXPORT int X509_ATTRIBUTE_count(const X509_ATTRIBUTE *attr);

// X509_ATTRIBUTE_get0_object returns the type of |attr|.
OPENSSL_EXPORT ASN1_OBJECT *X509_ATTRIBUTE_get0_object(X509_ATTRIBUTE *attr);

// X509_ATTRIBUTE_get0_type returns the |idx|th value in |attr|, or NULL if out
// of bounds. Note this function returns one of |attr|'s values, not the type.
OPENSSL_EXPORT ASN1_TYPE *X509_ATTRIBUTE_get0_type(X509_ATTRIBUTE *attr,
                                                   int idx);


// Certificate stores.
//
// An |X509_STORE| contains trusted certificates, CRLs, and verification
// parameters that are shared between multiple certificate verifications.
//
// Certificates in an |X509_STORE| are referred to as "trusted certificates",
// but an individual certificate verification may not necessarily treat every
// trusted certificate as a trust anchor. See |X509_VERIFY_PARAM_set_trust| for
// details.
//
// WARNING: Although a trusted certificate which fails the
// |X509_VERIFY_PARAM_set_trust| check is functionally an untrusted
// intermediate certificate, callers should not rely on this to configure
// untrusted intermediates in an |X509_STORE|. The trust check is complex, so
// this risks inadvertently treating it as a trust anchor. Instead, configure
// untrusted intermediates with the |chain| parameter of |X509_STORE_CTX_init|.
//
// Certificates in |X509_STORE| may be specified in several ways:
// - Added by |X509_STORE_add_cert|.
// - Returned by an |X509_LOOKUP| added by |X509_STORE_add_lookup|.
//
// |X509_STORE|s are reference-counted and may be shared by certificate
// verifications running concurrently on multiple threads. However, an
// |X509_STORE|'s verification parameters may not be modified concurrently with
// certificate verification or other operations. Unless otherwise documented,
// functions which take const pointer may be used concurrently, while
// functions which take a non-const pointer may not. Callers that wish to modify
// verification parameters in a shared |X509_STORE| should instead modify
// |X509_STORE_CTX|s individually.

// X509_STORE_new returns a newly-allocated |X509_STORE|, or NULL on error.
OPENSSL_EXPORT X509_STORE *X509_STORE_new(void);

// X509_STORE_up_ref adds one to the reference count of |store| and returns one.
// Although |store| is not const, this function's use of |store| is thread-safe.
OPENSSL_EXPORT int X509_STORE_up_ref(X509_STORE *store);

// X509_STORE_free releases memory associated with |store|.
OPENSSL_EXPORT void X509_STORE_free(X509_STORE *store);

// X509_STORE_add_cert adds |x509| to |store| as a trusted certificate. It
// returns one on success and zero on error. This function internally increments
// |x509|'s reference count, so the caller retains ownership of |x509|.
//
// Certificates configured by this function are still subject to the checks
// described in |X509_VERIFY_PARAM_set_trust|.
//
// Although |store| is not const, this function's use of |store| is thread-safe.
// However, if this function is called concurrently with |X509_verify_cert|, it
// is a race condition whether |x509| is available for issuer lookups.
// Moreover, the result may differ for each issuer lookup performed by a single
// |X509_verify_cert| call.
OPENSSL_EXPORT int X509_STORE_add_cert(X509_STORE *store, X509 *x509);

// X509_STORE_add_crl adds |crl| to |store|. It returns one on success and zero
// on error. This function internally increments |crl|'s reference count, so the
// caller retains ownership of |crl|. CRLs added in this way are candidates for
// CRL lookup when |X509_V_FLAG_CRL_CHECK| is set.
//
// Although |store| is not const, this function's use of |store| is thread-safe.
// However, if this function is called concurrently with |X509_verify_cert|, it
// is a race condition whether |crl| is available for CRL checks. Moreover, the
// result may differ for each CRL check performed by a single
// |X509_verify_cert| call.
//
// Note there are no supported APIs to remove CRLs from |store| once inserted.
// To vary the set of CRLs over time, callers should either create a new
// |X509_STORE| or configure CRLs on a per-verification basis with
// |X509_STORE_CTX_set0_crls|.
OPENSSL_EXPORT int X509_STORE_add_crl(X509_STORE *store, X509_CRL *crl);

// X509_STORE_get0_param returns |store|'s verification parameters. This object
// is mutable and may be modified by the caller. For an individual certificate
// verification operation, |X509_STORE_CTX_init| initializes the
// |X509_STORE_CTX|'s parameters with these parameters.
//
// WARNING: |X509_STORE_CTX_init| applies some default parameters (as in
// |X509_VERIFY_PARAM_inherit|) after copying |store|'s parameters. This means
// it is impossible to leave some parameters unset at |store|. They must be
// explicitly unset after creating the |X509_STORE_CTX|.
//
// As of writing these late defaults are a depth limit (see
// |X509_VERIFY_PARAM_set_depth|) and the |X509_V_FLAG_TRUSTED_FIRST| flag. This
// warning does not apply if the parameters were set in |store|.
//
// TODO(crbug.com/boringssl/441): This behavior is very surprising. Can we
// remove this notion of late defaults? The unsettable value at |X509_STORE| is
// -1, which rejects everything but explicitly-trusted self-signed certificates.
// |X509_V_FLAG_TRUSTED_FIRST| is mostly a workaround for poor path-building.
OPENSSL_EXPORT X509_VERIFY_PARAM *X509_STORE_get0_param(X509_STORE *store);

// X509_STORE_set1_param copies verification parameters from |param| as in
// |X509_VERIFY_PARAM_set1|. It returns one on success and zero on error.
OPENSSL_EXPORT int X509_STORE_set1_param(X509_STORE *store,
                                         const X509_VERIFY_PARAM *param);

// X509_STORE_set_flags enables all values in |flags| in |store|'s verification
// flags. |flags| should be a combination of |X509_V_FLAG_*| constants.
//
// WARNING: These flags will be combined with default flags when copied to an
// |X509_STORE_CTX|. This means it is impossible to unset those defaults from
// the |X509_STORE|. See discussion in |X509_STORE_get0_param|.
OPENSSL_EXPORT int X509_STORE_set_flags(X509_STORE *store, unsigned long flags);

// X509_STORE_set_depth configures |store| to, by default, limit certificate
// chains to |depth| intermediate certificates. This count excludes both the
// target certificate and the trust anchor (root certificate).
OPENSSL_EXPORT int X509_STORE_set_depth(X509_STORE *store, int depth);

// X509_STORE_set_purpose configures the purpose check for |store|. See
// |X509_VERIFY_PARAM_set_purpose| for details.
OPENSSL_EXPORT int X509_STORE_set_purpose(X509_STORE *store, int purpose);

// X509_STORE_set_trust configures the trust check for |store|. See
// |X509_VERIFY_PARAM_set_trust| for details.
OPENSSL_EXPORT int X509_STORE_set_trust(X509_STORE *store, int trust);

// The following constants indicate the type of an |X509_OBJECT|.
#define X509_LU_NONE 0
#define X509_LU_X509 1
#define X509_LU_CRL 2
#define X509_LU_PKEY 3

DEFINE_STACK_OF(X509_OBJECT)

// X509_OBJECT_new returns a newly-allocated, empty |X509_OBJECT| or NULL on
// error.
OPENSSL_EXPORT X509_OBJECT *X509_OBJECT_new(void);

// X509_OBJECT_free releases memory associated with |obj|.
OPENSSL_EXPORT void X509_OBJECT_free(X509_OBJECT *obj);

// X509_OBJECT_get_type returns the type of |obj|, which will be one of the
// |X509_LU_*| constants.
OPENSSL_EXPORT int X509_OBJECT_get_type(const X509_OBJECT *obj);

// X509_OBJECT_get0_X509 returns |obj| as a certificate, or NULL if |obj| is not
// a certificate.
OPENSSL_EXPORT X509 *X509_OBJECT_get0_X509(const X509_OBJECT *obj);

typedef STACK_OF(X509_CRL) *(*X509_STORE_CTX_lookup_crls_fn)(
    X509_STORE_CTX *ctx, X509_NAME *nm);

OPENSSL_EXPORT X509_STORE_CTX_lookup_crls_fn
X509_STORE_get_lookup_crls(X509_STORE *ctx);

OPENSSL_EXPORT void X509_STORE_set_lookup_crls(
    X509_STORE *ctx, X509_STORE_CTX_lookup_crls_fn lookup_crls);

#define X509_STORE_set_lookup_crls_cb(ctx, func) \
  X509_STORE_set_lookup_crls((ctx), (func))

// Certificate verification.
//
// An |X509_STORE_CTX| object represents a single certificate verification
// operation. To verify a certificate chain, callers construct an
// |X509_STORE_CTX|, initialize it with |X509_STORE_CTX_init|, configure extra
// parameters with |X509_STORE_CTX_get0_param|, and call |X509_verify_cert|.

// X509_STORE_CTX_new returns a newly-allocated, empty |X509_STORE_CTX|, or NULL
// on error.
OPENSSL_EXPORT X509_STORE_CTX *X509_STORE_CTX_new(void);

// X509_STORE_CTX_free releases memory associated with |ctx|.
OPENSSL_EXPORT void X509_STORE_CTX_free(X509_STORE_CTX *ctx);

// X509_STORE_CTX_init initializes |ctx| to verify |x509|, using trusted
// certificates and parameters in |store|. It returns one on success and zero on
// error. |chain| is a list of untrusted intermediate certificates to use in
// verification.
//
// |ctx| stores pointers to |store|, |x509|, and |chain|. Each of these objects
// must outlive |ctx| and may not be mutated for the duration of the certificate
// verification.
OPENSSL_EXPORT int X509_STORE_CTX_init(X509_STORE_CTX *ctx, X509_STORE *store,
                                       X509 *x509, STACK_OF(X509) *chain);

// X509_verify_cert attempts to discover and validate a certificate chain based
// on parameters in |ctx|. |ctx| usually includes a target certificate to be
// verified, a set of certificates serving as trust anchors, a list of
// non-trusted certificates that may be helpful for chain construction, flags,
// and various other optional components such as callback functions. |ctx|
// must have been initialized with |X509_STORE_CTX_init|. A certificate chain
// is built up starting from the target certificate and ending
// in a trust anchor. The chain is built up iteratively, looking up in turn a
// certificate with suitable key usage that matches as an issuer of the current
// "subject" certificate.
//
// NOTE:
//   1. Applications rarely call this function directly, but it is used
//      internally for certificate validation.
//   2. |X509_verify_cert| and other related functions call
//      |sk_X509_OBJECT_sort| internally, which rearranges the certificate
//      ordering. There will be cases where two certs have an identical
//      |subject| and |X509_OBJECT_cmp| will return 0, but one is a valid cert
//      and the other is invalid.
//      Due to https://github.com/openssl/openssl/issues/18708, certificate
//      verification could fail if an invalid cert is checked before the valid
//      cert. What we do with sorting behavior when certs are identical is
//      considered "unstable" and certain sorting expectations shouldn't be
//      depended on.
OPENSSL_EXPORT int X509_verify_cert(X509_STORE_CTX *ctx);

// X509_STORE_CTX_get0_chain, after a successful |X509_verify_cert| call,
// returns the verified certificate chain. The chain begins with the leaf and
// ends with trust anchor.
//
// At other points, such as after a failed verification or during the deprecated
// verification callback, it returns the partial chain built so far. Callers
// should avoid relying on this as this exposes unstable library implementation
// details.
OPENSSL_EXPORT STACK_OF(X509) *X509_STORE_CTX_get0_chain(X509_STORE_CTX *ctx);

// X509_STORE_CTX_get1_chain behaves like |X509_STORE_CTX_get0_chain| but
// returns a newly-allocated |STACK_OF(X509)| containing the completed chain,
// with each certificate's reference count incremented. Callers must free the
// result with |sk_X509_pop_free| and |X509_free| when done.
OPENSSL_EXPORT STACK_OF(X509) *X509_STORE_CTX_get1_chain(X509_STORE_CTX *ctx);

OPENSSL_EXPORT void X509_STORE_CTX_set_cert(X509_STORE_CTX *c, X509 *x);


// The following values are possible outputs of |X509_STORE_CTX_get_error|.
#define X509_V_OK 0
#define X509_V_ERR_UNSPECIFIED 1
#define X509_V_ERR_UNABLE_TO_GET_ISSUER_CERT 2
#define X509_V_ERR_UNABLE_TO_GET_CRL 3
#define X509_V_ERR_UNABLE_TO_DECRYPT_CERT_SIGNATURE 4
#define X509_V_ERR_UNABLE_TO_DECRYPT_CRL_SIGNATURE 5
#define X509_V_ERR_UNABLE_TO_DECODE_ISSUER_PUBLIC_KEY 6
#define X509_V_ERR_CERT_SIGNATURE_FAILURE 7
#define X509_V_ERR_CRL_SIGNATURE_FAILURE 8
#define X509_V_ERR_CERT_NOT_YET_VALID 9
#define X509_V_ERR_CERT_HAS_EXPIRED 10
#define X509_V_ERR_CRL_NOT_YET_VALID 11
#define X509_V_ERR_CRL_HAS_EXPIRED 12
#define X509_V_ERR_ERROR_IN_CERT_NOT_BEFORE_FIELD 13
#define X509_V_ERR_ERROR_IN_CERT_NOT_AFTER_FIELD 14
#define X509_V_ERR_ERROR_IN_CRL_LAST_UPDATE_FIELD 15
#define X509_V_ERR_ERROR_IN_CRL_NEXT_UPDATE_FIELD 16
#define X509_V_ERR_OUT_OF_MEM 17
#define X509_V_ERR_DEPTH_ZERO_SELF_SIGNED_CERT 18
#define X509_V_ERR_SELF_SIGNED_CERT_IN_CHAIN 19
#define X509_V_ERR_UNABLE_TO_GET_ISSUER_CERT_LOCALLY 20
#define X509_V_ERR_UNABLE_TO_VERIFY_LEAF_SIGNATURE 21
#define X509_V_ERR_CERT_CHAIN_TOO_LONG 22
#define X509_V_ERR_CERT_REVOKED 23
#define X509_V_ERR_INVALID_CA 24
#define X509_V_ERR_PATH_LENGTH_EXCEEDED 25
#define X509_V_ERR_INVALID_PURPOSE 26
#define X509_V_ERR_CERT_UNTRUSTED 27
#define X509_V_ERR_CERT_REJECTED 28
#define X509_V_ERR_SUBJECT_ISSUER_MISMATCH 29
#define X509_V_ERR_AKID_SKID_MISMATCH 30
#define X509_V_ERR_AKID_ISSUER_SERIAL_MISMATCH 31
#define X509_V_ERR_KEYUSAGE_NO_CERTSIGN 32
#define X509_V_ERR_UNABLE_TO_GET_CRL_ISSUER 33
#define X509_V_ERR_UNHANDLED_CRITICAL_EXTENSION 34
#define X509_V_ERR_KEYUSAGE_NO_CRL_SIGN 35
#define X509_V_ERR_UNHANDLED_CRITICAL_CRL_EXTENSION 36
#define X509_V_ERR_INVALID_NON_CA 37
#define X509_V_ERR_PROXY_PATH_LENGTH_EXCEEDED 38
#define X509_V_ERR_KEYUSAGE_NO_DIGITAL_SIGNATURE 39
#define X509_V_ERR_PROXY_CERTIFICATES_NOT_ALLOWED 40
#define X509_V_ERR_INVALID_EXTENSION 41
#define X509_V_ERR_INVALID_POLICY_EXTENSION 42
#define X509_V_ERR_NO_EXPLICIT_POLICY 43
#define X509_V_ERR_DIFFERENT_CRL_SCOPE 44
#define X509_V_ERR_UNSUPPORTED_EXTENSION_FEATURE 45
#define X509_V_ERR_UNNESTED_RESOURCE 46
#define X509_V_ERR_PERMITTED_VIOLATION 47
#define X509_V_ERR_EXCLUDED_VIOLATION 48
#define X509_V_ERR_SUBTREE_MINMAX 49
#define X509_V_ERR_APPLICATION_VERIFICATION 50
#define X509_V_ERR_UNSUPPORTED_CONSTRAINT_TYPE 51
#define X509_V_ERR_UNSUPPORTED_CONSTRAINT_SYNTAX 52
#define X509_V_ERR_UNSUPPORTED_NAME_SYNTAX 53
#define X509_V_ERR_CRL_PATH_VALIDATION_ERROR 54
#define X509_V_ERR_HOSTNAME_MISMATCH 62
#define X509_V_ERR_EMAIL_MISMATCH 63
#define X509_V_ERR_IP_ADDRESS_MISMATCH 64
#define X509_V_ERR_INVALID_CALL 65
#define X509_V_ERR_STORE_LOOKUP 66
#define X509_V_ERR_NAME_CONSTRAINTS_WITHOUT_SANS 67
// The following error codes are related to security levels in OpenSSL and are
// unused in AWS-LC. See |SSL_CTX_set_security_level|.
#define X509_V_ERR_EE_KEY_TOO_SMALL 68
#define X509_V_ERR_CA_KEY_TOO_SMALL 69
#define X509_V_ERR_CA_MD_TOO_WEAK 70
#define X509_V_UNABLE_TO_GET_CERTS_PUBLIC_KEY 71
#define X509_V_ERR_EC_KEY_EXPLICIT_PARAMS 72

// X509_STORE_CTX_get_error, after |X509_verify_cert| returns, returns
// |X509_V_OK| if verification succeeded or an |X509_V_ERR_*| describing why
// verification failed. This will be consistent with |X509_verify_cert|'s return
// value, unless the caller used the deprecated verification callback (see
// |X509_STORE_CTX_set_verify_cb|) in a way that breaks |ctx|'s invariants.
//
// If called during the deprecated verification callback when |ok| is zero, it
// returns the current error under consideration.
OPENSSL_EXPORT int X509_STORE_CTX_get_error(X509_STORE_CTX *ctx);

// X509_STORE_CTX_set_error sets |ctx|'s error to |err|, which should be
// |X509_V_OK| or an |X509_V_ERR_*| constant. It is not expected to be called in
// typical |X509_STORE_CTX| usage, but may be used in callback APIs where
// applications synthesize |X509_STORE_CTX| error conditions. See also
// |X509_STORE_CTX_set_verify_cb| and |SSL_CTX_set_cert_verify_callback|.
OPENSSL_EXPORT void X509_STORE_CTX_set_error(X509_STORE_CTX *ctx, int err);

// X509_verify_cert_error_string returns |err| as a human-readable string, where
// |err| should be one of the |X509_V_*| values. If |err| is unknown, it returns
// a default description.
OPENSSL_EXPORT const char *X509_verify_cert_error_string(long err);

// X509_STORE_CTX_get_error_depth returns the depth at which the error returned
// by |X509_STORE_CTX_get_error| occured. This is zero-indexed integer into the
// certificate chain. Zero indicates the target certificate, one its issuer, and
// so on.
OPENSSL_EXPORT int X509_STORE_CTX_get_error_depth(X509_STORE_CTX *ctx);

// X509_STORE_CTX_get_current_cert returns the certificate which caused the
// error returned by |X509_STORE_CTX_get_error|.
OPENSSL_EXPORT X509 *X509_STORE_CTX_get_current_cert(X509_STORE_CTX *ctx);

OPENSSL_EXPORT X509 *X509_STORE_CTX_get0_current_issuer(X509_STORE_CTX *ctx);

// X509_STORE_CTX_get0_current_crl returns the CRL which caused the error
// returned by |X509_STORE_CTX_get_error|.
OPENSSL_EXPORT X509_CRL *X509_STORE_CTX_get0_current_crl(X509_STORE_CTX *ctx);

// X509_STORE_CTX_get0_store returns the |X509_STORE| that |ctx| uses.
OPENSSL_EXPORT X509_STORE *X509_STORE_CTX_get0_store(X509_STORE_CTX *ctx);

// X509_STORE_CTX_get0_cert returns the leaf certificate that |ctx| is
// verifying.
OPENSSL_EXPORT X509 *X509_STORE_CTX_get0_cert(X509_STORE_CTX *ctx);

// X509_STORE_CTX_get0_untrusted returns the stack of untrusted intermediates
// used by |ctx| for certificate verification.
OPENSSL_EXPORT STACK_OF(X509) *X509_STORE_CTX_get0_untrusted(
    X509_STORE_CTX *ctx);

// X509_STORE_CTX_set0_trusted_stack configures |ctx| to trust the certificates
// in |sk|. |sk| must remain valid for the duration of |ctx|. Calling this
// function causes |ctx| to ignore any certificates configured in the
// |X509_STORE|. Certificates in |sk| are still subject to the check described
// in |X509_VERIFY_PARAM_set_trust|.
//
// WARNING: This function differs from most |set0| functions in that it does not
// take ownership of its input. The caller is required to ensure the lifetimes
// are consistent.
OPENSSL_EXPORT void X509_STORE_CTX_set0_trusted_stack(X509_STORE_CTX *ctx,
                                                      STACK_OF(X509) *sk);

// X509_STORE_CTX_set0_crls configures |ctx| to consider the CRLs in |sk| as
// candidates for CRL lookup. |sk| must remain valid for the duration of |ctx|.
// These CRLs are considered in addition to CRLs found in |X509_STORE|.
//
// WARNING: This function differs from most |set0| functions in that it does not
// take ownership of its input. The caller is required to ensure the lifetimes
// are consistent.
OPENSSL_EXPORT void X509_STORE_CTX_set0_crls(X509_STORE_CTX *ctx,
                                             STACK_OF(X509_CRL) *sk);

// X509_STORE_CTX_set_default looks up the set of parameters named |name| and
// applies those default verification parameters for |ctx|. As in
// |X509_VERIFY_PARAM_inherit|, only unset parameters are changed. This function
// returns one on success and zero on error.
//
// The supported values of |name| are:
// - "default" is an internal value which configures some late defaults. See the
//   discussion in |X509_STORE_get0_param|.
// - "pkcs7" configures default trust and purpose checks for PKCS#7 signatures.
// - "smime_sign" configures trust and purpose checks for S/MIME signatures.
// - "ssl_client" configures trust and purpose checks for TLS clients.
// - "ssl_server" configures trust and purpose checks for TLS servers.
//
// TODO(crbug.com/boringssl/441): Make "default" a no-op.
OPENSSL_EXPORT int X509_STORE_CTX_set_default(X509_STORE_CTX *ctx,
                                              const char *name);

// X509_STORE_CTX_get0_param returns |ctx|'s verification parameters. This
// object is mutable and may be modified by the caller.
OPENSSL_EXPORT X509_VERIFY_PARAM *X509_STORE_CTX_get0_param(
    X509_STORE_CTX *ctx);

// X509_STORE_CTX_set0_param returns |ctx|'s verification parameters to |param|
// and takes ownership of |param|. After this function returns, the caller
// should not free |param|.
//
// WARNING: This function discards any values which were previously applied in
// |ctx|, including the "default" parameters applied late in
// |X509_STORE_CTX_init|. These late defaults are not applied to parameters
// created standalone by |X509_VERIFY_PARAM_new|.
//
// TODO(crbug.com/boringssl/441): This behavior is very surprising. Should we
// re-apply the late defaults in |param|, or somehow avoid this notion of late
// defaults altogether?
OPENSSL_EXPORT void X509_STORE_CTX_set0_param(X509_STORE_CTX *ctx,
                                              X509_VERIFY_PARAM *param);

// X509_STORE_CTX_set_flags enables all values in |flags| in |ctx|'s
// verification flags. |flags| should be a combination of |X509_V_FLAG_*|
// constants.
OPENSSL_EXPORT void X509_STORE_CTX_set_flags(X509_STORE_CTX *ctx,
                                             unsigned long flags);

// X509_STORE_CTX_set_time configures certificate verification to use |t|
// instead of the current time. |flags| is ignored and should be zero.
OPENSSL_EXPORT void X509_STORE_CTX_set_time(X509_STORE_CTX *ctx,
                                            unsigned long flags, time_t t);

// X509_STORE_CTX_set_time_posix configures certificate verification to use |t|
// instead of the current time. |t| is interpreted as a POSIX timestamp in
// seconds. |flags| is ignored and should be zero.
OPENSSL_EXPORT void X509_STORE_CTX_set_time_posix(X509_STORE_CTX *ctx,
                                                  unsigned long flags,
                                                  int64_t t);

// X509_STORE_CTX_set_depth configures |ctx| to, by default, limit certificate
// chains to |depth| intermediate certificates. This count excludes both the
// target certificate and the trust anchor (root certificate).
OPENSSL_EXPORT void X509_STORE_CTX_set_depth(X509_STORE_CTX *ctx, int depth);

// X509_STORE_CTX_set_purpose simultaneously configures |ctx|'s purpose and
// trust checks, if unset. It returns one on success and zero if |purpose| is
// not a valid purpose value. |purpose| should be an |X509_PURPOSE_*| constant.
// If so, it configures |ctx| with a purpose check of |purpose| and a trust
// check of |purpose|'s corresponding trust value. If either the purpose or
// trust check had already been specified for |ctx|, that corresponding
// modification is silently dropped.
//
// See |X509_VERIFY_PARAM_set_purpose| and |X509_VERIFY_PARAM_set_trust| for
// details on the purpose and trust checks, respectively.
//
// If |purpose| is |X509_PURPOSE_ANY|, this function returns an error because it
// has no corresponding |X509_TRUST_*| value. It is not possible to set
// |X509_PURPOSE_ANY| with this function, only |X509_VERIFY_PARAM_set_purpose|.
//
// WARNING: Unlike similarly named functions in this header, this function
// silently does not behave the same as |X509_VERIFY_PARAM_set_purpose|. Callers
// may use |X509_VERIFY_PARAM_set_purpose| with |X509_STORE_CTX_get0_param| to
// avoid this difference.
OPENSSL_EXPORT int X509_STORE_CTX_set_purpose(X509_STORE_CTX *ctx, int purpose);

// X509_STORE_CTX_set_trust configures |ctx|'s trust check, if unset. It returns
// one on success and zero if |trust| is not a valid trust value. |trust| should
// be an |X509_TRUST_*| constant. If so, it configures |ctx| with a trust check
// of |trust|. If the trust check had already been specified for |ctx|, it
// silently does nothing.
//
// See |X509_VERIFY_PARAM_set_trust| for details on the purpose and trust check.
//
// WARNING: Unlike similarly named functions in this header, this function
// does not behave the same as |X509_VERIFY_PARAM_set_trust|. Callers may use
// |X509_VERIFY_PARAM_set_trust| with |X509_STORE_CTX_get0_param| to avoid this
// difference.
OPENSSL_EXPORT int X509_STORE_CTX_set_trust(X509_STORE_CTX *ctx, int trust);

// X509_STORE_CTX_add_custom_crit_oid adds |oid| to the list of "known" critical
// extension OIDs in |ctx|. Typical OpenSSL/AWS-LC behavior returns an error if
// there are any unknown critical extensions present within the certificates
// being validated. This function lets users specify custom OIDs of any critical
// extensions that are within the certificates being validated, that they wish
// to allow.
//
// To properly consume this feature, the callback mechanism with
// |X509_STORE_CTX_set_verify_crit_oids| must be set. See its specific
// documentation for more details.
OPENSSL_EXPORT int X509_STORE_CTX_add_custom_crit_oid(X509_STORE_CTX *ctx,
                                                      ASN1_OBJECT *oid);

// X509_STORE_CTX_verify_crit_oids is the callback signature for
// |X509_STORE_CTX_set_verify_crit_oids|. |ctx| is the context being used,
// |x509| represents the current certificate being validated, and |oids|
// is a stack of |ASN1_OBJECT|s representing unknown critical extension
// OIDs that were found in |x509| and match those previously registered via
// |X509_STORE_CTX_add_custom_crit_oid|.
typedef int (*X509_STORE_CTX_verify_crit_oids_cb)(X509_STORE_CTX *ctx,
                                                  X509 *x509,
                                                  STACK_OF(ASN1_OBJECT) *oids);

// X509_STORE_CTX_set_verify_crit_oids sets the |verify_crit_oids| callback
// function for |ctx|. Consumers should be performing additional validation
// against the custom extension oids after or during the handshake with
// |X509_STORE_CTX_set_verify_crit_oids|. This callback forces users to validate
// their custom OIDs when processing unknown custom critical extensions.
OPENSSL_EXPORT void X509_STORE_CTX_set_verify_crit_oids(
    X509_STORE_CTX *ctx,
    X509_STORE_CTX_verify_crit_oids_cb verify_custom_crit_oids);

// Verification parameters
//
// An |X509_VERIFY_PARAM| contains a set of parameters for certificate
// verification.

// X509_VERIFY_PARAM_new returns a newly-allocated |X509_VERIFY_PARAM|, or NULL
// on error.
OPENSSL_EXPORT X509_VERIFY_PARAM *X509_VERIFY_PARAM_new(void);

// X509_VERIFY_PARAM_free releases memory associated with |param|.
OPENSSL_EXPORT void X509_VERIFY_PARAM_free(X509_VERIFY_PARAM *param);

// X509_VERIFY_PARAM_inherit applies |from| as the default values for |to|. That
// is, for each parameter that is unset in |to|, it copies the value in |from|.
// This function returns one on success and zero on error.
OPENSSL_EXPORT int X509_VERIFY_PARAM_inherit(X509_VERIFY_PARAM *to,
                                             const X509_VERIFY_PARAM *from);

// X509_VERIFY_PARAM_set1 copies parameters from |from| to |to|. If a parameter
// is unset in |from|, the existing value in |to| is preserved. This function
// returns one on success and zero on error.
OPENSSL_EXPORT int X509_VERIFY_PARAM_set1(X509_VERIFY_PARAM *to,
                                          const X509_VERIFY_PARAM *from);

// X509_V_FLAG_* are flags for |X509_VERIFY_PARAM_set_flags| and
// |X509_VERIFY_PARAM_clear_flags|.

// X509_V_FLAG_CB_ISSUER_CHECK causes the deprecated verify callback (see
// |X509_STORE_CTX_set_verify_cb|) to be called for errors while matching
// subject and issuer certificates.
#define X509_V_FLAG_CB_ISSUER_CHECK 0x1
// X509_V_FLAG_USE_CHECK_TIME is an internal flag used to track whether
// |X509_STORE_CTX_set_time| has been used. If cleared, the system time is
// restored.
#define X509_V_FLAG_USE_CHECK_TIME 0x2
// X509_V_FLAG_CRL_CHECK enables CRL lookup and checking for the leaf.
#define X509_V_FLAG_CRL_CHECK 0x4
// X509_V_FLAG_CRL_CHECK_ALL enables CRL lookup and checking for the entire
// certificate chain. |X509_V_FLAG_CRL_CHECK| must be set for this flag to take
// effect.
#define X509_V_FLAG_CRL_CHECK_ALL 0x8
// X509_V_FLAG_IGNORE_CRITICAL ignores unhandled critical extensions. Do not use
// this option. Critical extensions ensure the verifier does not bypass
// unrecognized security restrictions in certificates.
#define X509_V_FLAG_IGNORE_CRITICAL 0x10
// X509_V_FLAG_X509_STRICT does nothing. Its functionality has been enabled by
// default.
#define X509_V_FLAG_X509_STRICT 0x00
// X509_V_FLAG_ALLOW_PROXY_CERTS does nothing. Proxy certificate support has
// been removed.
#define X509_V_FLAG_ALLOW_PROXY_CERTS 0x40
// X509_V_FLAG_POLICY_CHECK does nothing. Policy checking is always enabled.
#define X509_V_FLAG_POLICY_CHECK 0x80
// X509_V_FLAG_EXPLICIT_POLICY requires some policy OID to be asserted by the
// final certificate chain. See initial-explicit-policy from RFC 5280,
// section 6.1.1.
#define X509_V_FLAG_EXPLICIT_POLICY 0x100
// X509_V_FLAG_INHIBIT_ANY inhibits the anyPolicy OID. See
// initial-any-policy-inhibit from RFC 5280, section 6.1.1.
#define X509_V_FLAG_INHIBIT_ANY 0x200
// X509_V_FLAG_INHIBIT_MAP inhibits policy mapping. See
// initial-policy-mapping-inhibit from RFC 5280, section 6.1.1.
#define X509_V_FLAG_INHIBIT_MAP 0x400
// X509_V_FLAG_NOTIFY_POLICY does nothing. Its functionality has been removed.
#define X509_V_FLAG_NOTIFY_POLICY 0x800
// X509_V_FLAG_EXTENDED_CRL_SUPPORT causes all verifications to fail. Extended
// CRL features have been removed.
#define X509_V_FLAG_EXTENDED_CRL_SUPPORT 0x1000
// X509_V_FLAG_USE_DELTAS causes all verifications to fail. Delta CRL support
// has been removed.
#define X509_V_FLAG_USE_DELTAS 0x2000
// X509_V_FLAG_CHECK_SS_SIGNATURE checks the redundant signature on self-signed
// trust anchors. This check provides no security benefit and only wastes CPU.
#define X509_V_FLAG_CHECK_SS_SIGNATURE 0x4000
// X509_V_FLAG_TRUSTED_FIRST, during path-building, checks for a match in the
// trust store before considering an untrusted intermediate. This flag is
// enabled by default.
#define X509_V_FLAG_TRUSTED_FIRST 0x8000
// X509_V_FLAG_PARTIAL_CHAIN treats all trusted certificates as trust anchors,
// independent of the |X509_VERIFY_PARAM_set_trust| setting.
#define X509_V_FLAG_PARTIAL_CHAIN 0x80000
// X509_V_FLAG_NO_ALT_CHAINS disables building alternative chains if the initial
// one was rejected.
#define X509_V_FLAG_NO_ALT_CHAINS 0x100000
// X509_V_FLAG_NO_CHECK_TIME disables all time checks in certificate
// verification.
#define X509_V_FLAG_NO_CHECK_TIME 0x200000

// X509_VERIFY_PARAM_set_flags enables all values in |flags| in |param|'s
// verification flags and returns one. |flags| should be a combination of
// |X509_V_FLAG_*| constants.
OPENSSL_EXPORT int X509_VERIFY_PARAM_set_flags(X509_VERIFY_PARAM *param,
                                               unsigned long flags);

// X509_VERIFY_PARAM_clear_flags disables all values in |flags| in |param|'s
// verification flags and returns one. |flags| should be a combination of
// |X509_V_FLAG_*| constants.
OPENSSL_EXPORT int X509_VERIFY_PARAM_clear_flags(X509_VERIFY_PARAM *param,
                                                 unsigned long flags);

// X509_VERIFY_PARAM_get_flags returns |param|'s verification flags.
OPENSSL_EXPORT unsigned long X509_VERIFY_PARAM_get_flags(
    const X509_VERIFY_PARAM *param);

// X509_VERIFY_PARAM_set_depth configures |param| to limit certificate chains to
// |depth| intermediate certificates. This count excludes both the target
// certificate and the trust anchor (root certificate).
OPENSSL_EXPORT void X509_VERIFY_PARAM_set_depth(X509_VERIFY_PARAM *param,
                                                int depth);

// X509_VERIFY_PARAM_get_depth returns the maximum depth configured in |param|.
// See |X509_VERIFY_PARAM_set_depth|.
OPENSSL_EXPORT int X509_VERIFY_PARAM_get_depth(const X509_VERIFY_PARAM *param);

// X509_VERIFY_PARAM_set_time configures certificate verification to use |t|
// instead of the current time.
OPENSSL_EXPORT void X509_VERIFY_PARAM_set_time(X509_VERIFY_PARAM *param,
                                               time_t t);

// X509_VERIFY_PARAM_set_time_posix configures certificate verification to use
// |t| instead of the current time. |t| is interpreted as a POSIX timestamp in
// seconds.
OPENSSL_EXPORT void X509_VERIFY_PARAM_set_time_posix(X509_VERIFY_PARAM *param,
                                                     int64_t t);

// X509_VERIFY_PARAM_add0_policy adds |policy| to the user-initial-policy-set
// (see Section 6.1.1 of RFC 5280). On success, it takes ownership of
// |policy| and returns one. Otherwise, it returns zero and the caller retains
// owneship of |policy|.
OPENSSL_EXPORT int X509_VERIFY_PARAM_add0_policy(X509_VERIFY_PARAM *param,
                                                 ASN1_OBJECT *policy);

// X509_VERIFY_PARAM_set1_policies sets the user-initial-policy-set (see
// Section 6.1.1 of RFC 5280) to a copy of |policies|. It returns one on success
// and zero on error.
OPENSSL_EXPORT int X509_VERIFY_PARAM_set1_policies(
    X509_VERIFY_PARAM *param, const STACK_OF(ASN1_OBJECT) *policies);

// X509_VERIFY_PARAM_set1_host configures |param| to check for the DNS name
// specified by |name| and clears any previously specified hostname. If |name|
// is NULL or empty, the list of hostnames is cleared and name checks are not
// performed on the peer certificate. It returns one on success and zero on
// error.
// |namelen| should be set to the length of |name|. It may be zero if |name| is
// NUL-terminated, but this is only maintained for backwards compatibility with
// OpenSSL.
//
// By default, both subject alternative names and the subject's common name
// attribute are checked. The latter has long been deprecated, so callers should
// call |X509_VERIFY_PARAM_set_hostflags| with
// |X509_CHECK_FLAG_NEVER_CHECK_SUBJECT| to use the standard behavior.
// https://crbug.com/boringssl/464 tracks fixing the default.
OPENSSL_EXPORT int X509_VERIFY_PARAM_set1_host(X509_VERIFY_PARAM *param,
                                               const char *name,
                                               size_t name_len);

// X509_VERIFY_PARAM_add1_host adds |name| to the list of names checked by
// |param|. If any configured DNS name matches the certificate, verification
// succeeds. Any previous names set via |X509_VERIFY_PARAM_set1_host| or
// |X509_VERIFY_PARAM_add1_host| are retained, no change is made if |name| is
// NULL or empty. When multiple names are configured, the peer is considered
// verified when any name matches. It returns one on success and zero on error.
// |namelen| should be set to the length of |name|. It may be zero if |name| is
// NUL-terminated, but this is only maintained for backwards compatibility with
// OpenSSL.
//
// By default, both subject alternative names and the subject's common name
// attribute are checked. The latter has long been deprecated, so callers should
// call |X509_VERIFY_PARAM_set_hostflags| with
// |X509_CHECK_FLAG_NEVER_CHECK_SUBJECT| to use the standard behavior.
// https://crbug.com/boringssl/464 tracks fixing the default.
OPENSSL_EXPORT int X509_VERIFY_PARAM_add1_host(X509_VERIFY_PARAM *param,
                                               const char *name,
                                               size_t name_len);

// X509_CHECK_FLAG_ALWAYS_CHECK_SUBJECT enables always checking the subject name
// for host match even if subject alt names are present.
#define X509_CHECK_FLAG_ALWAYS_CHECK_SUBJECT 0x1

// X509_CHECK_FLAG_NO_WILDCARDS disables wildcard matching for DNS names.
#define X509_CHECK_FLAG_NO_WILDCARDS 0x2

// X509_CHECK_FLAG_SINGLE_LABEL_SUBDOMAINS constrains host name patterns passed
// to |X509_check_host| starting with '.' to only match a single label /
// subdomain.
//
// For example, by default the host name '.example.com' would match a
// certificate DNS name like 'www.example.com' and 'www.foo.example.com'.
// Setting this flag would result in the same host name only matching
// 'www.example.com' but not 'www.foo.example.com'.
#define X509_CHECK_FLAG_SINGLE_LABEL_SUBDOMAINS 0x10

// X509_CHECK_FLAG_NEVER_CHECK_SUBJECT disables the subject fallback, normally
// enabled when subjectAltNames is missing.
#define X509_CHECK_FLAG_NEVER_CHECK_SUBJECT 0x20

// X509_VERIFY_PARAM_set_hostflags sets the name-checking flags on |param| to
// |flags|. |flags| should be a combination of |X509_CHECK_FLAG_*| constants.
OPENSSL_EXPORT void X509_VERIFY_PARAM_set_hostflags(X509_VERIFY_PARAM *param,
                                                    unsigned int flags);

// X509_VERIFY_PARAM_get_hostflags returns |param|'s name-checking flags.
OPENSSL_EXPORT unsigned int X509_VERIFY_PARAM_get_hostflags(
    const X509_VERIFY_PARAM *param);


// X509_VERIFY_PARAM_set1_email configures |param| to check for the email
// address specified by |email|. It returns one on success and zero on error.
// |emaillen| should be set to the length of |email|. It may be zero if |email|
// is NUL-terminated, but this is only maintained for backwards compatibility
// with OpenSSL.
//
// By default, both subject alternative names and the subject's email address
// attribute are checked. The |X509_CHECK_FLAG_NEVER_CHECK_SUBJECT| flag may be
// used to change this behavior.
OPENSSL_EXPORT int X509_VERIFY_PARAM_set1_email(X509_VERIFY_PARAM *param,
                                                const char *email,
                                                size_t email_len);

// X509_VERIFY_PARAM_set1_ip configures |param| to check for the IP address
// specified by |ip|. It returns one on success and zero on error. The IP
// address is specified in its binary representation. |ip_len| must be 4 for an
// IPv4 address and 16 for an IPv6 address.
// NOTE:|iplen| MUST be set to the length of |ip|.
OPENSSL_EXPORT int X509_VERIFY_PARAM_set1_ip(X509_VERIFY_PARAM *param,
                                             const uint8_t *ip, size_t ip_len);

// X509_VERIFY_PARAM_set1_ip_asc decodes |ipasc| as the ASCII representation of
// an IPv4 or IPv6 address, and configures |param| to check for it. It returns
// one on success and zero on error.
// |ipasc| MUST be a NUL-terminal ASCII string: dotted decimal quad for IPv4 and
// colon-separated hexadecimal for IPv6. The condensed "::" notation is
// supported for IPv6 addresses.
OPENSSL_EXPORT int X509_VERIFY_PARAM_set1_ip_asc(X509_VERIFY_PARAM *param,
                                                 const char *ipasc);

// X509_PURPOSE_SSL_CLIENT validates TLS client certificates. It checks for the
// id-kp-clientAuth EKU and one of digitalSignature or keyAgreement key usages.
// The TLS library is expected to check for the key usage specific to the
// negotiated TLS parameters.
#define X509_PURPOSE_SSL_CLIENT 1
// X509_PURPOSE_SSL_SERVER validates TLS server certificates. It checks for the
// id-kp-clientAuth EKU and one of digitalSignature, keyAgreement, or
// keyEncipherment key usages. The TLS library is expected to check for the key
// usage specific to the negotiated TLS parameters.
#define X509_PURPOSE_SSL_SERVER 2
// X509_PURPOSE_NS_SSL_SERVER is a legacy mode. It behaves like
// |X509_PURPOSE_SSL_SERVER|, but only accepts the keyEncipherment key usage,
// used by SSL 2.0 and RSA key exchange. Do not use this.
#define X509_PURPOSE_NS_SSL_SERVER 3
// X509_PURPOSE_SMIME_SIGN validates S/MIME signing certificates. It checks for
// the id-kp-emailProtection EKU and one of digitalSignature or nonRepudiation
// key usages.
#define X509_PURPOSE_SMIME_SIGN 4
// X509_PURPOSE_SMIME_ENCRYPT validates S/MIME encryption certificates. It
// checks for the id-kp-emailProtection EKU and keyEncipherment key usage.
#define X509_PURPOSE_SMIME_ENCRYPT 5
// X509_PURPOSE_CRL_SIGN validates indirect CRL signers. It checks for the
// cRLSign key usage. BoringSSL does not support indirect CRLs and does not use
// this mode.
#define X509_PURPOSE_CRL_SIGN 6
// X509_PURPOSE_ANY performs no EKU or key usage checks. Such checks are the
// responsibility of the caller.
#define X509_PURPOSE_ANY 7
// X509_PURPOSE_OCSP_HELPER performs no EKU or key usage checks. It was
// historically used in OpenSSL's OCSP implementation, which left those checks
// to the OCSP implementation itself.
#define X509_PURPOSE_OCSP_HELPER 8
// X509_PURPOSE_TIMESTAMP_SIGN validates Time Stamping Authority (RFC 3161)
// certificates. It checks for the id-kp-timeStamping EKU and one of
// digitalSignature or nonRepudiation key usages. It additionally checks that
// the EKU extension is critical and that no other EKUs or key usages are
// asserted.
#define X509_PURPOSE_TIMESTAMP_SIGN 9

// X509_VERIFY_PARAM_set_purpose configures |param| to validate certificates for
// a specified purpose. It returns one on success and zero if |purpose| is not a
// valid purpose type. |purpose| should be one of the |X509_PURPOSE_*| values.
//
// This option controls checking the extended key usage (EKU) and key usage
// extensions. These extensions specify how a certificate's public key may be
// used and are important to avoid cross-protocol attacks, particularly in PKIs
// that may issue certificates for multiple protocols, or for protocols that use
// keys in multiple ways. If not configured, these security checks are the
// caller's responsibility.
//
// This library applies the EKU checks to all untrusted intermediates. Although
// not defined in RFC 5280, this matches widely-deployed practice. It also does
// not accept anyExtendedKeyUsage.
//
// Many purpose values have a corresponding trust value, which is not configured
// by this function.  See |X509_VERIFY_PARAM_set_trust| for details. Callers
// that wish to configure both should either call both functions, or use
// |X509_STORE_CTX_set_purpose|.
//
// It is currently not possible to configure custom EKU OIDs or key usage bits.
// Contact the BoringSSL maintainers if your application needs to do so. OpenSSL
// had an |X509_PURPOSE_add| API, but it was not thread-safe and relied on
// global mutable state, so we removed it.
//
// TODO(davidben): This function additionally configures checking the legacy
// Netscape certificate type extension. Remove this.
OPENSSL_EXPORT int X509_VERIFY_PARAM_set_purpose(X509_VERIFY_PARAM *param,
                                                 int purpose);

// X509_TRUST_COMPAT evaluates trust using only the self-signed fallback. Trust
// and distrust OIDs are ignored.
#define X509_TRUST_COMPAT 1
// X509_TRUST_SSL_CLIENT evaluates trust with the |NID_client_auth| OID, for
// validating TLS client certificates.
#define X509_TRUST_SSL_CLIENT 2
// X509_TRUST_SSL_SERVER evaluates trust with the |NID_server_auth| OID, for
// validating TLS server certificates.
#define X509_TRUST_SSL_SERVER 3
// X509_TRUST_EMAIL evaluates trust with the |NID_email_protect| OID, for
// validating S/MIME email certificates.
#define X509_TRUST_EMAIL 4
// X509_TRUST_OBJECT_SIGN evaluates trust with the |NID_code_sign| OID, for
// validating code signing certificates.
#define X509_TRUST_OBJECT_SIGN 5
// X509_TRUST_OCSP_SIGN does nothing. It's unused in OpenSSL and AWS-LC.
#define X509_TRUST_OCSP_SIGN 6
// X509_TRUST_OCSP_REQUEST does nothing. It's unused in OpenSSL and AWS-LC.
#define X509_TRUST_OCSP_REQUEST 7
// X509_TRUST_TSA evaluates trust with the |NID_time_stamp| OID, for validating
// Time Stamping Authority (RFC 3161) certificates.
#define X509_TRUST_TSA 8

// X509_VERIFY_PARAM_set_trust configures which certificates from |X509_STORE|
// are trust anchors. It returns one on success and zero if |trust| is not a
// valid trust value. |trust| should be one of the |X509_TRUST_*| constants.
// This function allows applications to vary trust anchors when the same set of
// trusted certificates is used in multiple contexts.
//
// Two properties determine whether a certificate is a trust anchor:
//
// - Whether it is trusted or distrusted for some OID, via auxiliary information
//   configured by |X509_add1_trust_object| or |X509_add1_reject_object|.
//
// - Whether it is "self-signed". That is, whether |X509_get_extension_flags|
//   includes |EXFLAG_SS|. The signature itself is not checked.
//
// When this function is called, |trust| determines the OID to check in the
// first case. If the certificate is not explicitly trusted or distrusted for
// any OID, it is trusted if self-signed instead.
//
// If unset, the default behavior is to check for the |NID_anyExtendedKeyUsage|
// OID. If the certificate is not explicitly trusted or distrusted for this OID,
// it is trusted if self-signed instead. Note this slightly differs from the
// above.
//
// If the |X509_V_FLAG_PARTIAL_CHAIN| is set, every certificate from
// |X509_STORE| is a trust anchor, unless it was explicitly distrusted for the
// OID.
//
// It is currently not possible to configure custom trust OIDs. Contact the
// BoringSSL maintainers if your application needs to do so. OpenSSL had an
// |X509_TRUST_add| API, but it was not thread-safe and relied on global mutable
// state, so we removed it.
OPENSSL_EXPORT int X509_VERIFY_PARAM_set_trust(X509_VERIFY_PARAM *param,
                                               int trust);

// X509_VERIFY_PARAM_enable_ec_key_explicit_params enables X.509 subject public
// keys to contain elliptic curve keys with explicit parameters. By default
// AWS-LC rejects validation of certificate chains containing public keys
// with explicit EC parameters. Returns 1 on success, or 0 on failure.
OPENSSL_EXPORT int X509_VERIFY_PARAM_enable_ec_key_explicit_params(
    X509_VERIFY_PARAM *param);

// X509_VERIFY_PARAM_disable_ec_key_explicit_params disables X.509 subject
// public keys to contain elliptic curve keys with explicit parameters. By
// default AWS-LC rejects validation of certificate chains containing public
// keys with explicit EC parameters. Returns 1 on success, or 0 on failure.
OPENSSL_EXPORT int X509_VERIFY_PARAM_disable_ec_key_explicit_params(
    X509_VERIFY_PARAM *param);

// Filesystem-based certificate stores.
//
// An |X509_STORE| may be configured to get its contents from the filesystem.
// This is done by adding |X509_LOOKUP| structures to the |X509_STORE| with
// |X509_STORE_add_lookup| and then configuring the |X509_LOOKUP| with paths.
//
// Most cases can use |X509_STORE_load_locations|, which configures the same
// thing but is simpler to use.

// X509_STORE_load_locations configures |store| to load data from filepaths
// |file| and |dir|. It returns one on success and zero on error. Either of
// |file| or |dir| may be NULL, but at least one must be non-NULL.
//
// If |file| is non-NULL, it loads CRLs and trusted certificates in PEM format
// from the file at |file|, and them to |store|, as in |X509_load_cert_crl_file|
// with |X509_FILETYPE_PEM|.
//
// If |dir| is non-NULL, it configures |store| to load CRLs and trusted
// certificates from the directory at |dir| in PEM format, as in
// |X509_LOOKUP_add_dir| with |X509_FILETYPE_PEM|.
OPENSSL_EXPORT int X509_STORE_load_locations(X509_STORE *store,
                                             const char *file, const char *dir);

// X509_STORE_add_lookup returns an |X509_LOOKUP| associated with |store| with
// type |method|, or NULL on error. The result is owned by |store|, so callers
// are not expected to free it. This may be used with |X509_LOOKUP_add_dir| or
// |X509_LOOKUP_load_file|, depending on |method|, to configure |store|.
//
// A single |X509_LOOKUP| may be configured with multiple paths, and an
// |X509_STORE| only contains one |X509_LOOKUP| of each type, so there is no
// need to call this function multiple times for a single type. Calling it
// multiple times will return the previous |X509_LOOKUP| of that type.
OPENSSL_EXPORT X509_LOOKUP *X509_STORE_add_lookup(
    X509_STORE *store, const X509_LOOKUP_METHOD *method);

// X509_LOOKUP_hash_dir creates |X509_LOOKUP|s that may be used with
// |X509_LOOKUP_add_dir|.
OPENSSL_EXPORT const X509_LOOKUP_METHOD *X509_LOOKUP_hash_dir(void);

// X509_LOOKUP_file creates |X509_LOOKUP|s that may be used with
// |X509_LOOKUP_load_file|.
//
// Although this is modeled as an |X509_LOOKUP|, this function is redundant. It
// has the same effect as loading a certificate or CRL from the filesystem, in
// the caller's desired format, and then adding it with |X509_STORE_add_cert|
// and |X509_STORE_add_crl|.
OPENSSL_EXPORT const X509_LOOKUP_METHOD *X509_LOOKUP_file(void);

// The following constants are used to specify the format of files in an
// |X509_LOOKUP|.
#define X509_FILETYPE_PEM 1
#define X509_FILETYPE_ASN1 2
#define X509_FILETYPE_DEFAULT 3

// X509_LOOKUP_load_file calls |X509_load_cert_crl_file|. |lookup| must have
// been constructed with |X509_LOOKUP_file|.
//
// If |type| is |X509_FILETYPE_DEFAULT|, it ignores |file| and instead uses some
// default system path with |X509_FILETYPE_PEM|. See also
// |X509_STORE_set_default_paths|.
OPENSSL_EXPORT int X509_LOOKUP_load_file(X509_LOOKUP *lookup, const char *file,
                                         int type);

// X509_LOOKUP_add_dir configures |lookup| to load CRLs and trusted certificates
// from the directories in |path|. It returns one on success and zero on error.
// |lookup| must have been constructed with |X509_LOOKUP_hash_dir|.
//
// WARNING: |path| is interpreted as a colon-separated (semicolon-separated on
// Windows) list of paths. It is not possible to configure a path containing the
// separator character. https://crbug.com/boringssl/691 tracks removing this
// behavior.
//
// |type| should be one of the |X509_FILETYPE_*| constants and determines the
// format of the files. If |type| is |X509_FILETYPE_DEFAULT|, |path| is ignored
// and some default system path is used with |X509_FILETYPE_PEM|. See also
// |X509_STORE_set_default_paths|.
//
// Trusted certificates should be named HASH.N and CRLs should be
// named HASH.rN. HASH is |X509_NAME_hash| of the certificate subject and CRL
// issuer, respectively, in hexadecimal. N is in decimal and counts hash
// collisions consecutively, starting from zero. For example, "002c0b4f.0" and
// "002c0b4f.r0".
//
// WARNING: Objects from |path| are loaded on demand, but cached in memory on
// the |X509_STORE|. If a CA is removed from the directory, existing
// |X509_STORE|s will continue to trust it. Cache entries are not evicted for
// the lifetime of the |X509_STORE|.
//
// WARNING: This mechanism is also not well-suited for CRL updates.
// |X509_STORE|s rely on this cache and never load the same CRL file twice. CRL
// updates must use a new file, with an incremented suffix, to be reflected in
// existing |X509_STORE|s. However, this means each CRL update will use
// additional storage and memory. Instead, configure inputs that vary per
// verification, such as CRLs, on each |X509_STORE_CTX| separately, using
// functions like |X509_STORE_CTX_set0_crl|.
OPENSSL_EXPORT int X509_LOOKUP_add_dir(X509_LOOKUP *lookup, const char *path,
                                       int type);

// X509_L_* are commands for |X509_LOOKUP_ctrl|.
#define X509_L_FILE_LOAD 1
#define X509_L_ADD_DIR 2

// X509_LOOKUP_ctrl implements commands on |lookup|. |cmd| specifies the
// command. The other arguments specify the operation in a command-specific way.
// Use |X509_LOOKUP_load_file| or |X509_LOOKUP_add_dir| instead.
OPENSSL_EXPORT int X509_LOOKUP_ctrl(X509_LOOKUP *lookup, int cmd,
                                    const char *argc, long argl, char **ret);

// X509_load_cert_file loads trusted certificates from |file| and adds them to
// |lookup|'s |X509_STORE|. It returns one on success and zero on error.
//
// If |type| is |X509_FILETYPE_ASN1|, it loads a single DER-encoded certificate.
// If |type| is |X509_FILETYPE_PEM|, it loads a sequence of PEM-encoded
// certificates. |type| may not be |X509_FILETYPE_DEFAULT|.
OPENSSL_EXPORT int X509_load_cert_file(X509_LOOKUP *lookup, const char *file,
                                       int type);

// X509_load_crl_file loads CRLs from |file| and add them it to |lookup|'s
// |X509_STORE|. It returns one on success and zero on error.
//
// If |type| is |X509_FILETYPE_ASN1|, it loads a single DER-encoded CRL. If
// |type| is |X509_FILETYPE_PEM|, it loads a sequence of PEM-encoded CRLs.
// |type| may not be |X509_FILETYPE_DEFAULT|.
OPENSSL_EXPORT int X509_load_crl_file(X509_LOOKUP *lookup, const char *file,
                                      int type);

// X509_load_cert_crl_file loads CRLs and trusted certificates from |file| and
// adds them to |lookup|'s |X509_STORE|. It returns one on success and zero on
// error.
//
// If |type| is |X509_FILETYPE_ASN1|, it loads a single DER-encoded certificate.
// This function cannot be used to load a DER-encoded CRL. If |type| is
// |X509_FILETYPE_PEM|, it loads a sequence of PEM-encoded certificates and
// CRLs. |type| may not be |X509_FILETYPE_DEFAULT|.
OPENSSL_EXPORT int X509_load_cert_crl_file(X509_LOOKUP *lookup,
                                           const char *file, int type);

// X509_NAME_hash returns a hash of |name|, or zero on error. This is the new
// hash used by |X509_LOOKUP_add_dir|.
//
// This hash is specific to the |X509_LOOKUP_add_dir| filesystem format and is
// not suitable for general-purpose X.509 name processing. It is very short, so
// there will be hash collisions. It also depends on an OpenSSL-specific
// canonicalization process.
//
// TODO(https://crbug.com/boringssl/407): This should be const and thread-safe
// but currently is neither, notably if |name| was modified from its parsed
// value.
OPENSSL_EXPORT uint32_t X509_NAME_hash(X509_NAME *name);

// X509_NAME_hash_old returns a hash of |name|, or zero on error. This is the
// legacy hash used by |X509_LOOKUP_add_dir|, which is still supported for
// compatibility.
//
// This hash is specific to the |X509_LOOKUP_add_dir| filesystem format and is
// not suitable for general-purpose X.509 name processing. It is very short, so
// there will be hash collisions.
//
// TODO(https://crbug.com/boringssl/407): This should be const and thread-safe
// but currently is neither, notably if |name| was modified from its parsed
// value.
OPENSSL_EXPORT uint32_t X509_NAME_hash_old(X509_NAME *name);

// X509_STORE_set_default_paths configures |store| to read from some "default"
// filesystem paths. It returns one on success and zero on error. The filesystem
// paths are determined by a combination of hardcoded paths and the SSL_CERT_DIR
// and SSL_CERT_FILE environment variables.
//
// Using this function is not recommended. In OpenSSL, these defaults are
// determined by OpenSSL's install prefix. There is no corresponding concept for
// BoringSSL. Future versions of BoringSSL may change or remove this
// functionality.
OPENSSL_EXPORT int X509_STORE_set_default_paths(X509_STORE *store);

// The following functions return filesystem paths used to determine the above
// "default" paths, when the corresponding environment variables are not set.
//
// Using these functions is not recommended. In OpenSSL, these defaults are
// determined by OpenSSL's install prefix. There is no corresponding concept for
// BoringSSL. Future versions of BoringSSL may change or remove this
// functionality.
OPENSSL_EXPORT const char *X509_get_default_cert_area(void);
OPENSSL_EXPORT const char *X509_get_default_cert_dir(void);
OPENSSL_EXPORT const char *X509_get_default_cert_file(void);
OPENSSL_EXPORT const char *X509_get_default_private_dir(void);

// X509_get_default_cert_dir_env returns "SSL_CERT_DIR", an environment variable
// used to determine the above "default" paths.
OPENSSL_EXPORT const char *X509_get_default_cert_dir_env(void);

// X509_get_default_cert_file_env returns "SSL_CERT_FILE", an environment
// variable used to determine the above "default" paths.
OPENSSL_EXPORT const char *X509_get_default_cert_file_env(void);


// SignedPublicKeyAndChallenge structures.
//
// The SignedPublicKeyAndChallenge (SPKAC) is a legacy structure to request
// certificates, primarily in the legacy <keygen> HTML tag. An SPKAC structure
// is represented by a |NETSCAPE_SPKI| structure.
//
// The structure is described in
// https://developer.mozilla.org/en-US/docs/Web/HTML/Element/keygen

// A Netscape_spki_st, or |NETSCAPE_SPKI|, represents a
// SignedPublicKeyAndChallenge structure. Although this structure contains a
// |spkac| field of type |NETSCAPE_SPKAC|, these are misnamed. The SPKAC is the
// entire structure, not the signed portion.
struct Netscape_spki_st {
  NETSCAPE_SPKAC *spkac;
  X509_ALGOR *sig_algor;
  ASN1_BIT_STRING *signature;
} /* NETSCAPE_SPKI */;

// NETSCAPE_SPKI is an |ASN1_ITEM| whose ASN.1 type is
// SignedPublicKeyAndChallenge and C type is |NETSCAPE_SPKI*|.
DECLARE_ASN1_ITEM(NETSCAPE_SPKI)

// NETSCAPE_SPKI_new returns a newly-allocated, empty |NETSCAPE_SPKI| object, or
// NULL on error.
OPENSSL_EXPORT NETSCAPE_SPKI *NETSCAPE_SPKI_new(void);

// NETSCAPE_SPKI_free releases memory associated with |spki|.
OPENSSL_EXPORT void NETSCAPE_SPKI_free(NETSCAPE_SPKI *spki);

// d2i_NETSCAPE_SPKI parses up to |len| bytes from |*inp| as a DER-encoded
// SignedPublicKeyAndChallenge structure, as described in |d2i_SAMPLE|.
OPENSSL_EXPORT NETSCAPE_SPKI *d2i_NETSCAPE_SPKI(NETSCAPE_SPKI **out,
                                                const uint8_t **inp, long len);

// i2d_NETSCAPE_SPKI marshals |spki| as a DER-encoded
// SignedPublicKeyAndChallenge structure, as described in |i2d_SAMPLE|.
OPENSSL_EXPORT int i2d_NETSCAPE_SPKI(const NETSCAPE_SPKI *spki, uint8_t **outp);

// NETSCAPE_SPKI_verify checks that |spki| has a valid signature by |pkey|. It
// returns one if the signature is valid and zero otherwise.
OPENSSL_EXPORT int NETSCAPE_SPKI_verify(NETSCAPE_SPKI *spki, EVP_PKEY *pkey);

// NETSCAPE_SPKI_b64_decode decodes |len| bytes from |str| as a base64-encoded
// SignedPublicKeyAndChallenge structure. It returns a newly-allocated
// |NETSCAPE_SPKI| structure with the result, or NULL on error. If |len| is 0 or
// negative, the length is calculated with |strlen| and |str| must be a
// NUL-terminated C string.
OPENSSL_EXPORT NETSCAPE_SPKI *NETSCAPE_SPKI_b64_decode(const char *str,
                                                       ossl_ssize_t len);

// NETSCAPE_SPKI_b64_encode encodes |spki| as a base64-encoded
// SignedPublicKeyAndChallenge structure. It returns a newly-allocated
// NUL-terminated C string with the result, or NULL on error. The caller must
// release the memory with |OPENSSL_free| when done.
OPENSSL_EXPORT char *NETSCAPE_SPKI_b64_encode(NETSCAPE_SPKI *spki);

// NETSCAPE_SPKI_get_pubkey decodes and returns the public key in |spki| as an
// |EVP_PKEY|, or NULL on error. The caller takes ownership of the resulting
// pointer and must call |EVP_PKEY_free| when done.
OPENSSL_EXPORT EVP_PKEY *NETSCAPE_SPKI_get_pubkey(const NETSCAPE_SPKI *spki);

// NETSCAPE_SPKI_set_pubkey sets |spki|'s public key to |pkey|. It returns one
// on success or zero on error. This function does not take ownership of |pkey|,
// so the caller may continue to manage its lifetime independently of |spki|.
OPENSSL_EXPORT int NETSCAPE_SPKI_set_pubkey(NETSCAPE_SPKI *spki,
                                            EVP_PKEY *pkey);

// NETSCAPE_SPKI_sign signs |spki| with |pkey| and replaces the signature
// algorithm and signature fields. It returns the length of the signature on
// success and zero on error. This function uses digest algorithm |md|, or
// |pkey|'s default if NULL. Other signing parameters use |pkey|'s defaults.
OPENSSL_EXPORT int NETSCAPE_SPKI_sign(NETSCAPE_SPKI *spki, EVP_PKEY *pkey,
                                      const EVP_MD *md);

// A Netscape_spkac_st, or |NETSCAPE_SPKAC|, represents a PublicKeyAndChallenge
// structure. This type is misnamed. The full SPKAC includes the signature,
// which is represented with the |NETSCAPE_SPKI| type.
struct Netscape_spkac_st {
  X509_PUBKEY *pubkey;
  ASN1_IA5STRING *challenge;
} /* NETSCAPE_SPKAC */;

// NETSCAPE_SPKAC is an |ASN1_ITEM| whose ASN.1 type is PublicKeyAndChallenge
// and C type is |NETSCAPE_SPKAC*|.
DECLARE_ASN1_ITEM(NETSCAPE_SPKAC)

// NETSCAPE_SPKAC_new returns a newly-allocated, empty |NETSCAPE_SPKAC| object,
// or NULL on error.
OPENSSL_EXPORT NETSCAPE_SPKAC *NETSCAPE_SPKAC_new(void);

// NETSCAPE_SPKAC_free releases memory associated with |spkac|.
OPENSSL_EXPORT void NETSCAPE_SPKAC_free(NETSCAPE_SPKAC *spkac);

// d2i_NETSCAPE_SPKAC parses up to |len| bytes from |*inp| as a DER-encoded
// PublicKeyAndChallenge structure, as described in |d2i_SAMPLE|.
OPENSSL_EXPORT NETSCAPE_SPKAC *d2i_NETSCAPE_SPKAC(NETSCAPE_SPKAC **out,
                                                  const uint8_t **inp,
                                                  long len);

// i2d_NETSCAPE_SPKAC marshals |spkac| as a DER-encoded PublicKeyAndChallenge
// structure, as described in |i2d_SAMPLE|.
OPENSSL_EXPORT int i2d_NETSCAPE_SPKAC(const NETSCAPE_SPKAC *spkac,
                                      uint8_t **outp);

// NETSCAPE_SPKI_print prints out the contents of |spki| to |out|.
OPENSSL_EXPORT int NETSCAPE_SPKI_print(BIO *out, NETSCAPE_SPKI *spki);


// RSASSA-PSS Parameters.
//
// In X.509, RSASSA-PSS signatures and keys use a complex parameter structure,
// defined in RFC 4055. The following functions are provided for compatibility
// with some OpenSSL APIs relating to this. Use of RSASSA-PSS in X.509 is
// discouraged. The parameters structure is very complex, and it takes more
// bytes to merely encode parameters than an entire P-256 ECDSA signature.

// An rsa_pss_params_st, aka |RSA_PSS_PARAMS|, represents a parsed
// RSASSA-PSS-params structure, as defined in (RFC 4055).
struct rsa_pss_params_st {
  X509_ALGOR *hashAlgorithm;
  X509_ALGOR *maskGenAlgorithm;
  ASN1_INTEGER *saltLength;
  ASN1_INTEGER *trailerField;
  // OpenSSL caches the MGF hash on |RSA_PSS_PARAMS| in some cases. None of the
  // cases apply to BoringSSL, so this is always NULL, but Node expects the
  // field to be present.
  X509_ALGOR *maskHash;
} /* RSA_PSS_PARAMS */;

// RSA_PSS_PARAMS is an |ASN1_ITEM| whose ASN.1 type is RSASSA-PSS-params (RFC
// 4055) and C type is |RSA_PSS_PARAMS*|.
DECLARE_ASN1_ITEM(RSA_PSS_PARAMS)

// RSA_PSS_PARAMS_new returns a new, empty |RSA_PSS_PARAMS|, or NULL on error.
OPENSSL_EXPORT RSA_PSS_PARAMS *RSA_PSS_PARAMS_new(void);

// RSA_PSS_PARAMS_free releases memory associated with |params|.
OPENSSL_EXPORT void RSA_PSS_PARAMS_free(RSA_PSS_PARAMS *params);

// d2i_RSA_PSS_PARAMS parses up to |len| bytes from |*inp| as a DER-encoded
// RSASSA-PSS-params (RFC 4055), as described in |d2i_SAMPLE|.
OPENSSL_EXPORT RSA_PSS_PARAMS *d2i_RSA_PSS_PARAMS(RSA_PSS_PARAMS **out,
                                                  const uint8_t **inp,
                                                  long len);

// i2d_RSA_PSS_PARAMS marshals |in| as a DER-encoded RSASSA-PSS-params (RFC
// 4055), as described in |i2d_SAMPLE|.
OPENSSL_EXPORT int i2d_RSA_PSS_PARAMS(const RSA_PSS_PARAMS *in, uint8_t **outp);


// PKCS#8 private keys.
//
// The |PKCS8_PRIV_KEY_INFO| type represents a PKCS#8 PrivateKeyInfo (RFC 5208)
// structure. This is analogous to SubjectPublicKeyInfo and uses the same
// AlgorithmIdentifiers, but carries private keys and is not part of X.509
// itself.
//
// TODO(davidben): Do these functions really belong in this header?

// PKCS8_PRIV_KEY_INFO is an |ASN1_ITEM| whose ASN.1 type is PrivateKeyInfo and
// C type is |PKCS8_PRIV_KEY_INFO*|.
DECLARE_ASN1_ITEM(PKCS8_PRIV_KEY_INFO)

// PKCS8_PRIV_KEY_INFO_new returns a newly-allocated, empty
// |PKCS8_PRIV_KEY_INFO| object, or NULL on error.
OPENSSL_EXPORT PKCS8_PRIV_KEY_INFO *PKCS8_PRIV_KEY_INFO_new(void);

// PKCS8_PRIV_KEY_INFO_free releases memory associated with |key|.
OPENSSL_EXPORT void PKCS8_PRIV_KEY_INFO_free(PKCS8_PRIV_KEY_INFO *key);

// d2i_PKCS8_PRIV_KEY_INFO parses up to |len| bytes from |*inp| as a DER-encoded
// PrivateKeyInfo, as described in |d2i_SAMPLE|.
OPENSSL_EXPORT PKCS8_PRIV_KEY_INFO *d2i_PKCS8_PRIV_KEY_INFO(
    PKCS8_PRIV_KEY_INFO **out, const uint8_t **inp, long len);

// i2d_PKCS8_PRIV_KEY_INFO marshals |key| as a DER-encoded PrivateKeyInfo, as
// described in |i2d_SAMPLE|.
OPENSSL_EXPORT int i2d_PKCS8_PRIV_KEY_INFO(const PKCS8_PRIV_KEY_INFO *key,
                                           uint8_t **outp);

// EVP_PKCS82PKEY returns |p8| as a newly-allocated |EVP_PKEY|, or NULL if the
// key was unsupported or could not be decoded. The caller must release the
// result with |EVP_PKEY_free| when done.
//
// Use |EVP_parse_private_key| instead.
OPENSSL_EXPORT EVP_PKEY *EVP_PKCS82PKEY(const PKCS8_PRIV_KEY_INFO *p8);

// EVP_PKEY2PKCS8 encodes |pkey| as a PKCS#8 PrivateKeyInfo (RFC 5208),
// represented as a newly-allocated |PKCS8_PRIV_KEY_INFO|, or NULL on error. The
// caller must release the result with |PKCS8_PRIV_KEY_INFO_free| when done.
//
// Use |EVP_marshal_private_key| instead.
OPENSSL_EXPORT PKCS8_PRIV_KEY_INFO *EVP_PKEY2PKCS8(const EVP_PKEY *pkey);


// Algorithm and octet string pairs.
//
// The |X509_SIG| type represents an ASN.1 SEQUENCE type of an
// AlgorithmIdentifier and an OCTET STRING. Although named |X509_SIG|, there is
// no type in X.509 which matches this format. The two common types which do are
// DigestInfo (RFC 2315 and RFC 8017), and EncryptedPrivateKeyInfo (RFC 5208).

// X509_SIG is an |ASN1_ITEM| whose ASN.1 type is the SEQUENCE described above
// and C type is |X509_SIG*|.
DECLARE_ASN1_ITEM(X509_SIG)

// X509_SIG_new returns a newly-allocated, empty |X509_SIG| object, or NULL on
// error.
OPENSSL_EXPORT X509_SIG *X509_SIG_new(void);

// X509_SIG_free releases memory associated with |key|.
OPENSSL_EXPORT void X509_SIG_free(X509_SIG *key);

// d2i_X509_SIG parses up to |len| bytes from |*inp| as a DER-encoded algorithm
// and octet string pair, as described in |d2i_SAMPLE|.
OPENSSL_EXPORT X509_SIG *d2i_X509_SIG(X509_SIG **out, const uint8_t **inp,
                                      long len);

// i2d_X509_SIG marshals |sig| as a DER-encoded algorithm
// and octet string pair, as described in |i2d_SAMPLE|.
OPENSSL_EXPORT int i2d_X509_SIG(const X509_SIG *sig, uint8_t **outp);

// X509_SIG_get0 sets |*out_alg| and |*out_digest| to non-owning pointers to
// |sig|'s algorithm and digest fields, respectively. Either |out_alg| and
// |out_digest| may be NULL to skip those fields.
OPENSSL_EXPORT void X509_SIG_get0(const X509_SIG *sig,
                                  const X509_ALGOR **out_alg,
                                  const ASN1_OCTET_STRING **out_digest);

// X509_SIG_getm behaves like |X509_SIG_get0| but returns mutable pointers.
OPENSSL_EXPORT void X509_SIG_getm(X509_SIG *sig, X509_ALGOR **out_alg,
                                  ASN1_OCTET_STRING **out_digest);


// Printing functions.
//
// The following functions output human-readable representations of
// X.509-related structures. They should only be used for debugging or logging
// and not parsed programmatically. In many cases, the outputs are ambiguous, so
// attempting to parse them can lead to string injection vulnerabilities.

// The following flags control |X509_print_ex| and |X509_REQ_print_ex|. These
// flags co-exist with |X509V3_EXT_*|, so avoid collisions when adding new ones.

// X509_FLAG_COMPAT disables all flags. It additionally causes names to be
// printed with a 16-byte indent.
#define X509_FLAG_COMPAT 0

// X509_FLAG_NO_HEADER skips a header identifying the type of object printed.
#define X509_FLAG_NO_HEADER 1L

// X509_FLAG_NO_VERSION skips printing the X.509 version number.
#define X509_FLAG_NO_VERSION (1L << 1)

// X509_FLAG_NO_SERIAL skips printing the serial number. It is ignored in
// |X509_REQ_print_fp|.
#define X509_FLAG_NO_SERIAL (1L << 2)

// X509_FLAG_NO_SIGNAME skips printing the signature algorithm in the
// TBSCertificate. It is ignored in |X509_REQ_print_fp|.
#define X509_FLAG_NO_SIGNAME (1L << 3)

// X509_FLAG_NO_ISSUER skips printing the issuer.
#define X509_FLAG_NO_ISSUER (1L << 4)

// X509_FLAG_NO_VALIDITY skips printing the notBefore and notAfter times. It is
// ignored in |X509_REQ_print_fp|.
#define X509_FLAG_NO_VALIDITY (1L << 5)

// X509_FLAG_NO_SUBJECT skips printing the subject.
#define X509_FLAG_NO_SUBJECT (1L << 6)

// X509_FLAG_NO_PUBKEY skips printing the public key.
#define X509_FLAG_NO_PUBKEY (1L << 7)

// X509_FLAG_NO_EXTENSIONS skips printing the extension list. It is ignored in
// |X509_REQ_print_fp|. CSRs instead have attributes, which is controlled by
// |X509_FLAG_NO_ATTRIBUTES|.
#define X509_FLAG_NO_EXTENSIONS (1L << 8)

// X509_FLAG_NO_SIGDUMP skips printing the signature and outer signature
// algorithm.
#define X509_FLAG_NO_SIGDUMP (1L << 9)

// X509_FLAG_NO_AUX skips printing auxiliary properties. (See |d2i_X509_AUX| and
// related functions.)
#define X509_FLAG_NO_AUX (1L << 10)

// X509_FLAG_NO_ATTRIBUTES skips printing CSR attributes. It does nothing for
// certificates and CRLs.
#define X509_FLAG_NO_ATTRIBUTES (1L << 11)

// X509_FLAG_NO_IDS skips printing the issuerUniqueID and subjectUniqueID in a
// certificate. It is ignored in |X509_REQ_print_fp|.
#define X509_FLAG_NO_IDS (1L << 12)

// The following flags control |X509_print_ex|, |X509_REQ_print_ex|,
// |X509V3_EXT_print|, and |X509V3_extensions_print|. These flags coexist with
// |X509_FLAG_*|, so avoid collisions when adding new ones.

// X509V3_EXT_UNKNOWN_MASK is a mask that determines how unknown extensions are
// processed.
#define X509V3_EXT_UNKNOWN_MASK (0xfL << 16)

// X509V3_EXT_DEFAULT causes unknown extensions or syntax errors to return
// failure.
#define X509V3_EXT_DEFAULT 0

// X509V3_EXT_ERROR_UNKNOWN causes unknown extensions or syntax errors to print
// as "<Not Supported>" or "<Parse Error>", respectively.
#define X509V3_EXT_ERROR_UNKNOWN (1L << 16)

// X509V3_EXT_PARSE_UNKNOWN is deprecated and behaves like
// |X509V3_EXT_DUMP_UNKNOWN|.
#define X509V3_EXT_PARSE_UNKNOWN (2L << 16)

// X509V3_EXT_DUMP_UNKNOWN causes unknown extensions to be displayed as a
// hexdump.
#define X509V3_EXT_DUMP_UNKNOWN (3L << 16)

// X509_print_ex writes a human-readable representation of |x| to |bp|. It
// returns one on success and zero on error. |nmflags| is the flags parameter
// for |X509_NAME_print_ex| when printing the subject and issuer. |cflag| should
// be some combination of the |X509_FLAG_*| and |X509V3_EXT_*| constants.
OPENSSL_EXPORT int X509_print_ex(BIO *bp, X509 *x, unsigned long nmflag,
                                 unsigned long cflag);

// X509_print_ex_fp behaves like |X509_print_ex| but writes to |fp|.
OPENSSL_EXPORT int X509_print_ex_fp(FILE *fp, X509 *x, unsigned long nmflag,
                                    unsigned long cflag);

// X509_print calls |X509_print_ex| with |XN_FLAG_COMPAT| and |X509_FLAG_COMPAT|
// flags.
OPENSSL_EXPORT int X509_print(BIO *bp, X509 *x);

// X509_print_fp behaves like |X509_print| but writes to |fp|.
OPENSSL_EXPORT int X509_print_fp(FILE *fp, X509 *x);

// X509_CRL_print writes a human-readable representation of |x| to |bp|. It
// returns one on success and zero on error.
OPENSSL_EXPORT int X509_CRL_print(BIO *bp, X509_CRL *x);

// X509_CRL_print_fp behaves like |X509_CRL_print| but writes to |fp|.
OPENSSL_EXPORT int X509_CRL_print_fp(FILE *fp, X509_CRL *x);

// X509_REQ_print_ex writes a human-readable representation of |x| to |bp|. It
// returns one on success and zero on error. |nmflags| is the flags parameter
// for |X509_NAME_print_ex|, when printing the subject. |cflag| should be some
// combination of the |X509_FLAG_*| and |X509V3_EXT_*| constants.
OPENSSL_EXPORT int X509_REQ_print_ex(BIO *bp, X509_REQ *x, unsigned long nmflag,
                                     unsigned long cflag);

// X509_REQ_print calls |X509_REQ_print_ex| with |XN_FLAG_COMPAT| and
// |X509_FLAG_COMPAT| flags.
OPENSSL_EXPORT int X509_REQ_print(BIO *bp, X509_REQ *req);

// X509_REQ_print_fp behaves like |X509_REQ_print| but writes to |fp|.
OPENSSL_EXPORT int X509_REQ_print_fp(FILE *fp, X509_REQ *req);

// The following flags are control |X509_NAME_print_ex|. They must not collide
// with |ASN1_STRFLGS_*|.
//
// TODO(davidben): This is far, far too many options and most of them are
// useless. Trim this down.

// XN_FLAG_COMPAT prints with |X509_NAME_print|'s format and return value
// convention.
#define XN_FLAG_COMPAT 0ul

// XN_FLAG_SEP_MASK determines the separators to use between attributes.
#define XN_FLAG_SEP_MASK (0xful << 16)

// XN_FLAG_SEP_COMMA_PLUS separates RDNs with "," and attributes within an RDN
// with "+", as in RFC 2253.
#define XN_FLAG_SEP_COMMA_PLUS (1ul << 16)

// XN_FLAG_SEP_CPLUS_SPC behaves like |XN_FLAG_SEP_COMMA_PLUS| but adds spaces
// between the separators.
#define XN_FLAG_SEP_CPLUS_SPC (2ul << 16)

// XN_FLAG_SEP_SPLUS_SPC separates RDNs with "; " and attributes within an RDN
// with " + ".
#define XN_FLAG_SEP_SPLUS_SPC (3ul << 16)

// XN_FLAG_SEP_MULTILINE prints each attribute on one line.
#define XN_FLAG_SEP_MULTILINE (4ul << 16)

// XN_FLAG_DN_REV prints RDNs in reverse, from least significant to most
// significant, as RFC 2253.
#define XN_FLAG_DN_REV (1ul << 20)

// XN_FLAG_FN_MASK determines how attribute types are displayed.
#define XN_FLAG_FN_MASK (0x3ul << 21)

// XN_FLAG_FN_SN uses the attribute type's short name, when available.
#define XN_FLAG_FN_SN 0ul

// XN_FLAG_FN_LN uses the attribute type's long name, when available.
#define XN_FLAG_FN_LN (1 << 21)

// XN_FLAG_FN_OID always prints attribute types as OIDs.
#define XN_FLAG_FN_OID (2 << 21)

// XN_FLAG_FN_NONE skips printing field names.
#define XN_FLAG_FN_NONE (3 << 21)

// XN_FLAG_SPC_EQ wraps the "=" operator with spaces when printing attributes.
#define XN_FLAG_SPC_EQ (1ul << 23)

// XN_FLAG_DUMP_UNKNOWN_FIELDS causes unknown attribute types to be printed in
// hex, as in RFC 2253.
#define XN_FLAG_DUMP_UNKNOWN_FIELDS (1ul << 24)

// XN_FLAG_FN_ALIGN aligns attribute names to 10 characters if using short
// names, and 25 characters if using long names.
#define XN_FLAG_FN_ALIGN (1 << 25)

// XN_FLAG_RFC2253 prints like RFC 2253.
#define XN_FLAG_RFC2253                                             \
  (ASN1_STRFLGS_RFC2253 | XN_FLAG_SEP_COMMA_PLUS | XN_FLAG_DN_REV | \
   XN_FLAG_FN_SN | XN_FLAG_DUMP_UNKNOWN_FIELDS)

// XN_FLAG_ONELINE prints a one-line representation of the name.
#define XN_FLAG_ONELINE                                                    \
  (ASN1_STRFLGS_RFC2253 | ASN1_STRFLGS_ESC_QUOTE | XN_FLAG_SEP_CPLUS_SPC | \
   XN_FLAG_SPC_EQ | XN_FLAG_FN_SN)

// XN_FLAG_MULTILINE prints a multi-line representation of the name.
#define XN_FLAG_MULTILINE                                                 \
  (ASN1_STRFLGS_ESC_CTRL | ASN1_STRFLGS_ESC_MSB | XN_FLAG_SEP_MULTILINE | \
   XN_FLAG_SPC_EQ | XN_FLAG_FN_LN | XN_FLAG_FN_ALIGN)

// X509_NAME_print_ex writes a human-readable representation of |nm| to |out|.
// Each line of output is indented by |indent| spaces. It returns the number of
// bytes written on success, and -1 on error. If |out| is NULL, it returns the
// number of bytes it would have written but does not write anything. |flags|
// should be some combination of |XN_FLAG_*| and |ASN1_STRFLGS_*| values and
// determines the output. If unsure, use |XN_FLAG_RFC2253|.
//
// If |flags| is |XN_FLAG_COMPAT|, or zero, this function calls
// |X509_NAME_print| instead. In that case, it returns one on success, rather
// than the output length.
OPENSSL_EXPORT int X509_NAME_print_ex(BIO *out, const X509_NAME *nm, int indent,
                                      unsigned long flags);

// X509_NAME_print prints a human-readable representation of |name| to |bp|. It
// returns one on success and zero on error. |obase| is ignored.
//
// This function outputs a legacy format that does not correctly handle string
// encodings and other cases. Prefer |X509_NAME_print_ex| if printing a name for
// debugging purposes.
OPENSSL_EXPORT int X509_NAME_print(BIO *bp, const X509_NAME *name, int obase);

// X509_NAME_oneline writes a human-readable representation to |name| to a
// buffer as a NUL-terminated C string.
//
// If |buf| is NULL, returns a newly-allocated buffer containing the result on
// success, or NULL on error. The buffer must be released with |OPENSSL_free|
// when done.
//
// If |buf| is non-NULL, at most |size| bytes of output are written to |buf|
// instead. |size| includes the trailing NUL. The function then returns |buf| on
// success or NULL on error. If the output does not fit in |size| bytes, the
// output is silently truncated at an attribute boundary.
//
// This function outputs a legacy format that does not correctly handle string
// encodings and other cases. Prefer |X509_NAME_print_ex| if printing a name for
// debugging purposes.
OPENSSL_EXPORT char *X509_NAME_oneline(const X509_NAME *name, char *buf,
                                       int size);

// X509_NAME_print_ex_fp behaves like |X509_NAME_print_ex| but writes to |fp|.
OPENSSL_EXPORT int X509_NAME_print_ex_fp(FILE *fp, const X509_NAME *nm,
                                         int indent, unsigned long flags);

// X509_signature_dump writes a human-readable representation of |sig| to |bio|,
// indented with |indent| spaces. It returns one on success and zero on error.
OPENSSL_EXPORT int X509_signature_dump(BIO *bio, const ASN1_STRING *sig,
                                       int indent);

// X509_signature_print writes a human-readable representation of |alg| and
// |sig| to |bio|. It returns one on success and zero on error.
OPENSSL_EXPORT int X509_signature_print(BIO *bio, const X509_ALGOR *alg,
                                        const ASN1_STRING *sig);

// X509V3_EXT_print prints a human-readable representation of |ext| to out. It
// returns one on success and zero on error. The output is indented by |indent|
// spaces. |flag| is one of the |X509V3_EXT_*| constants and controls printing
// of unknown extensions and syntax errors.
//
// WARNING: Although some applications programmatically parse the output of this
// function to process X.509 extensions, this is not safe. In many cases, the
// outputs are ambiguous to attempting to parse them can lead to string
// injection vulnerabilities. These functions should only be used for debugging
// or logging.
OPENSSL_EXPORT int X509V3_EXT_print(BIO *out, const X509_EXTENSION *ext,
                                    unsigned long flag, int indent);

// X509V3_EXT_print_fp behaves like |X509V3_EXT_print| but writes to a |FILE|
// instead of a |BIO|.
OPENSSL_EXPORT int X509V3_EXT_print_fp(FILE *out, const X509_EXTENSION *ext,
                                       int flag, int indent);

// X509V3_extensions_print prints |title|, followed by a human-readable
// representation of |exts| to |out|. It returns one on success and zero on
// error. The output is indented by |indent| spaces. |flag| is one of the
// |X509V3_EXT_*| constants and controls printing of unknown extensions and
// syntax errors.
OPENSSL_EXPORT int X509V3_extensions_print(BIO *out, const char *title,
                                           const STACK_OF(X509_EXTENSION) *exts,
                                           unsigned long flag, int indent);

// GENERAL_NAME_print prints a human-readable representation of |gen| to |out|.
// It returns one on success and zero on error.
//
// TODO(davidben): Actually, it just returns one and doesn't check for I/O or
// allocation errors. But it should return zero on error.
OPENSSL_EXPORT int GENERAL_NAME_print(BIO *out, const GENERAL_NAME *gen);


// Convenience functions.

// X509_pubkey_digest hashes the contents of the BIT STRING in |x509|'s
// subjectPublicKeyInfo field with |md| and writes the result to |out|.
// |EVP_MD_CTX_size| bytes are written, which is at most |EVP_MAX_MD_SIZE|. If
// |out_len| is not NULL, |*out_len| is set to the number of bytes written. This
// function returns one on success and zero on error.
//
// This hash omits the BIT STRING tag, length, and number of unused bits. It
// also omits the AlgorithmIdentifier which describes the key type. It
// corresponds to the OCSP KeyHash definition and is not suitable for other
// purposes.
OPENSSL_EXPORT int X509_pubkey_digest(const X509 *x509, const EVP_MD *md,
                                      uint8_t *out, unsigned *out_len);

// X509_digest hashes |x509|'s DER encoding with |md| and writes the result to
// |out|. |EVP_MD_CTX_size| bytes are written, which is at most
// |EVP_MAX_MD_SIZE|. If |out_len| is not NULL, |*out_len| is set to the number
// of bytes written. This function returns one on success and zero on error.
// Note this digest covers the entire certificate, not just the signed portion.
OPENSSL_EXPORT int X509_digest(const X509 *x509, const EVP_MD *md, uint8_t *out,
                               unsigned *out_len);

// X509_CRL_digest hashes |crl|'s DER encoding with |md| and writes the result
// to |out|. |EVP_MD_CTX_size| bytes are written, which is at most
// |EVP_MAX_MD_SIZE|. If |out_len| is not NULL, |*out_len| is set to the number
// of bytes written. This function returns one on success and zero on error.
// Note this digest covers the entire CRL, not just the signed portion.
OPENSSL_EXPORT int X509_CRL_digest(const X509_CRL *crl, const EVP_MD *md,
                                   uint8_t *out, unsigned *out_len);

// X509_REQ_digest hashes |req|'s DER encoding with |md| and writes the result
// to |out|. |EVP_MD_CTX_size| bytes are written, which is at most
// |EVP_MAX_MD_SIZE|. If |out_len| is not NULL, |*out_len| is set to the number
// of bytes written. This function returns one on success and zero on error.
// Note this digest covers the entire certificate request, not just the signed
// portion.
OPENSSL_EXPORT int X509_REQ_digest(const X509_REQ *req, const EVP_MD *md,
                                   uint8_t *out, unsigned *out_len);

// X509_NAME_digest hashes |name|'s DER encoding with |md| and writes the result
// to |out|. |EVP_MD_CTX_size| bytes are written, which is at most
// |EVP_MAX_MD_SIZE|. If |out_len| is not NULL, |*out_len| is set to the number
// of bytes written. This function returns one on success and zero on error.
OPENSSL_EXPORT int X509_NAME_digest(const X509_NAME *name, const EVP_MD *md,
                                    uint8_t *out, unsigned *out_len);

// The following functions behave like the corresponding unsuffixed |d2i_*|
// functions, but read the result from |bp| instead. Callers using these
// functions with memory |BIO|s to parse structures already in memory should use
// |d2i_*| instead.
OPENSSL_EXPORT X509 *d2i_X509_bio(BIO *bp, X509 **x509);
OPENSSL_EXPORT X509_CRL *d2i_X509_CRL_bio(BIO *bp, X509_CRL **crl);
OPENSSL_EXPORT X509_REQ *d2i_X509_REQ_bio(BIO *bp, X509_REQ **req);
OPENSSL_EXPORT RSA *d2i_RSAPrivateKey_bio(BIO *bp, RSA **rsa);
OPENSSL_EXPORT RSA *d2i_RSAPublicKey_bio(BIO *bp, RSA **rsa);
OPENSSL_EXPORT RSA *d2i_RSA_PUBKEY_bio(BIO *bp, RSA **rsa);
OPENSSL_EXPORT DSA *d2i_DSA_PUBKEY_bio(BIO *bp, DSA **dsa);
OPENSSL_EXPORT DSA *d2i_DSAPrivateKey_bio(BIO *bp, DSA **dsa);
OPENSSL_EXPORT EC_KEY *d2i_EC_PUBKEY_bio(BIO *bp, EC_KEY **eckey);
OPENSSL_EXPORT EC_KEY *d2i_ECPrivateKey_bio(BIO *bp, EC_KEY **eckey);
OPENSSL_EXPORT X509_SIG *d2i_PKCS8_bio(BIO *bp, X509_SIG **p8);
OPENSSL_EXPORT PKCS8_PRIV_KEY_INFO *d2i_PKCS8_PRIV_KEY_INFO_bio(
    BIO *bp, PKCS8_PRIV_KEY_INFO **p8inf);
OPENSSL_EXPORT EVP_PKEY *d2i_PUBKEY_bio(BIO *bp, EVP_PKEY **a);
OPENSSL_EXPORT DH *d2i_DHparams_bio(BIO *bp, DH **dh);

// d2i_PrivateKey_bio behaves like |d2i_AutoPrivateKey|, but reads from |bp|
// instead.
OPENSSL_EXPORT EVP_PKEY *d2i_PrivateKey_bio(BIO *bp, EVP_PKEY **a);

// The following functions behave like the corresponding unsuffixed |i2d_*|
// functions, but write the result to |bp|. They return one on success and zero
// on error. Callers using them with memory |BIO|s to encode structures to
// memory should use |i2d_*| directly instead.
OPENSSL_EXPORT int i2d_X509_bio(BIO *bp, X509 *x509);
OPENSSL_EXPORT int i2d_X509_CRL_bio(BIO *bp, X509_CRL *crl);
OPENSSL_EXPORT int i2d_X509_REQ_bio(BIO *bp, X509_REQ *req);
OPENSSL_EXPORT int i2d_RSAPrivateKey_bio(BIO *bp, RSA *rsa);
OPENSSL_EXPORT int i2d_RSAPublicKey_bio(BIO *bp, RSA *rsa);
OPENSSL_EXPORT int i2d_RSA_PUBKEY_bio(BIO *bp, RSA *rsa);
OPENSSL_EXPORT int i2d_DSA_PUBKEY_bio(BIO *bp, DSA *dsa);
OPENSSL_EXPORT int i2d_DSAPrivateKey_bio(BIO *bp, DSA *dsa);
OPENSSL_EXPORT int i2d_EC_PUBKEY_bio(BIO *bp, EC_KEY *eckey);
OPENSSL_EXPORT int i2d_ECPrivateKey_bio(BIO *bp, EC_KEY *eckey);
OPENSSL_EXPORT int i2d_PKCS8_bio(BIO *bp, X509_SIG *p8);
OPENSSL_EXPORT int i2d_PKCS8_PRIV_KEY_INFO_bio(BIO *bp,
                                               PKCS8_PRIV_KEY_INFO *p8inf);
OPENSSL_EXPORT int i2d_PrivateKey_bio(BIO *bp, EVP_PKEY *pkey);
OPENSSL_EXPORT int i2d_PUBKEY_bio(BIO *bp, EVP_PKEY *pkey);
OPENSSL_EXPORT int i2d_DHparams_bio(BIO *bp, const DH *dh);

// i2d_PKCS8PrivateKeyInfo_bio encodes |key| as a PKCS#8 PrivateKeyInfo
// structure (see |EVP_marshal_private_key|) and writes the result to |bp|. It
// returns one on success and zero on error.
OPENSSL_EXPORT int i2d_PKCS8PrivateKeyInfo_bio(BIO *bp, EVP_PKEY *key);

// The following functions behave like the corresponding |d2i_*_bio| functions,
// but read from |fp| instead.
OPENSSL_EXPORT X509 *d2i_X509_fp(FILE *fp, X509 **x509);
OPENSSL_EXPORT X509_CRL *d2i_X509_CRL_fp(FILE *fp, X509_CRL **crl);
OPENSSL_EXPORT X509_REQ *d2i_X509_REQ_fp(FILE *fp, X509_REQ **req);
OPENSSL_EXPORT RSA *d2i_RSAPrivateKey_fp(FILE *fp, RSA **rsa);
OPENSSL_EXPORT RSA *d2i_RSAPublicKey_fp(FILE *fp, RSA **rsa);
OPENSSL_EXPORT RSA *d2i_RSA_PUBKEY_fp(FILE *fp, RSA **rsa);
OPENSSL_EXPORT DSA *d2i_DSA_PUBKEY_fp(FILE *fp, DSA **dsa);
OPENSSL_EXPORT DSA *d2i_DSAPrivateKey_fp(FILE *fp, DSA **dsa);
OPENSSL_EXPORT EC_KEY *d2i_EC_PUBKEY_fp(FILE *fp, EC_KEY **eckey);
OPENSSL_EXPORT EC_KEY *d2i_ECPrivateKey_fp(FILE *fp, EC_KEY **eckey);
OPENSSL_EXPORT X509_SIG *d2i_PKCS8_fp(FILE *fp, X509_SIG **p8);
OPENSSL_EXPORT PKCS8_PRIV_KEY_INFO *d2i_PKCS8_PRIV_KEY_INFO_fp(
    FILE *fp, PKCS8_PRIV_KEY_INFO **p8inf);
OPENSSL_EXPORT EVP_PKEY *d2i_PrivateKey_fp(FILE *fp, EVP_PKEY **a);
OPENSSL_EXPORT EVP_PKEY *d2i_PUBKEY_fp(FILE *fp, EVP_PKEY **a);

// The following functions behave like the corresponding |i2d_*_bio| functions,
// but write to |fp| instead.
OPENSSL_EXPORT int i2d_X509_fp(FILE *fp, X509 *x509);
OPENSSL_EXPORT int i2d_X509_CRL_fp(FILE *fp, X509_CRL *crl);
OPENSSL_EXPORT int i2d_X509_REQ_fp(FILE *fp, X509_REQ *req);
OPENSSL_EXPORT int i2d_RSAPrivateKey_fp(FILE *fp, RSA *rsa);
OPENSSL_EXPORT int i2d_RSAPublicKey_fp(FILE *fp, RSA *rsa);
OPENSSL_EXPORT int i2d_RSA_PUBKEY_fp(FILE *fp, RSA *rsa);
OPENSSL_EXPORT int i2d_DSA_PUBKEY_fp(FILE *fp, DSA *dsa);
OPENSSL_EXPORT int i2d_DSAPrivateKey_fp(FILE *fp, DSA *dsa);
OPENSSL_EXPORT int i2d_EC_PUBKEY_fp(FILE *fp, EC_KEY *eckey);
OPENSSL_EXPORT int i2d_ECPrivateKey_fp(FILE *fp, EC_KEY *eckey);
OPENSSL_EXPORT int i2d_PKCS8_fp(FILE *fp, X509_SIG *p8);
OPENSSL_EXPORT int i2d_PKCS8_PRIV_KEY_INFO_fp(FILE *fp,
                                              PKCS8_PRIV_KEY_INFO *p8inf);
OPENSSL_EXPORT int i2d_PKCS8PrivateKeyInfo_fp(FILE *fp, EVP_PKEY *key);
OPENSSL_EXPORT int i2d_PrivateKey_fp(FILE *fp, EVP_PKEY *pkey);
OPENSSL_EXPORT int i2d_PUBKEY_fp(FILE *fp, EVP_PKEY *pkey);

// X509_find_by_issuer_and_serial returns the first |X509| in |sk| whose issuer
// and serial are |name| and |serial|, respectively. If no match is found, it
// returns NULL.
OPENSSL_EXPORT X509 *X509_find_by_issuer_and_serial(const STACK_OF(X509) *sk,
                                                    X509_NAME *name,
                                                    const ASN1_INTEGER *serial);

// X509_find_by_subject returns the first |X509| in |sk| whose subject is
// |name|. If no match is found, it returns NULL.
OPENSSL_EXPORT X509 *X509_find_by_subject(const STACK_OF(X509) *sk,
                                          X509_NAME *name);

// X509_cmp_time compares |s| against |*t|. On success, it returns a negative
// number if |s| <= |*t| and a positive number if |s| > |*t|. On error, it
// returns zero. If |t| is NULL, it uses the current time instead of |*t|.
//
// WARNING: Unlike most comparison functions, this function returns zero on
// error, not equality.
OPENSSL_EXPORT int X509_cmp_time(const ASN1_TIME *s, const time_t *t);

// X509_cmp_time_posix compares |s| against |t|. On success, it returns a
// negative number if |s| <= |t| and a positive number if |s| > |t|. On error,
// it returns zero.
//
// WARNING: Unlike most comparison functions, this function returns zero on
// error, not equality.
OPENSSL_EXPORT int X509_cmp_time_posix(const ASN1_TIME *s, int64_t t);

// X509_cmp_current_time behaves like |X509_cmp_time| but compares |s| against
// the current time.
OPENSSL_EXPORT int X509_cmp_current_time(const ASN1_TIME *s);

// X509_time_adj calls |X509_time_adj_ex| with |offset_day| equal to zero.
OPENSSL_EXPORT ASN1_TIME *X509_time_adj(ASN1_TIME *s, long offset_sec,
                                        const time_t *t);

// X509_time_adj_ex behaves like |ASN1_TIME_adj|, but adds an offset to |*t|. If
// |t| is NULL, it uses the current time instead of |*t|.
OPENSSL_EXPORT ASN1_TIME *X509_time_adj_ex(ASN1_TIME *s, int offset_day,
                                           long offset_sec, const time_t *t);

// X509_gmtime_adj behaves like |X509_time_adj_ex| but adds |offset_sec| to the
// current time.
OPENSSL_EXPORT ASN1_TIME *X509_gmtime_adj(ASN1_TIME *s, long offset_sec);

// X509_issuer_name_cmp behaves like |X509_NAME_cmp|, but compares |a| and |b|'s
// issuer names.
OPENSSL_EXPORT int X509_issuer_name_cmp(const X509 *a, const X509 *b);

// X509_subject_name_cmp behaves like |X509_NAME_cmp|, but compares |a| and
// |b|'s subject names.
OPENSSL_EXPORT int X509_subject_name_cmp(const X509 *a, const X509 *b);

// X509_CRL_cmp behaves like |X509_NAME_cmp|, but compares |a| and |b|'s
// issuer names.
//
// WARNING: This function is misnamed. It does not compare other parts of the
// CRL, only the issuer fields using |X509_NAME_cmp|.
OPENSSL_EXPORT int X509_CRL_cmp(const X509_CRL *a, const X509_CRL *b);

// X509_issuer_name_hash returns the hash of |x509|'s issuer name with
// |X509_NAME_hash|.
//
// This hash is specific to the |X509_LOOKUP_add_dir| filesystem format and is
// not suitable for general-purpose X.509 name processing. It is very short, so
// there will be hash collisions. It also depends on an OpenSSL-specific
// canonicalization process.
OPENSSL_EXPORT uint32_t X509_issuer_name_hash(X509 *x509);

// X509_subject_name_hash returns the hash of |x509|'s subject name with
// |X509_NAME_hash|.
//
// This hash is specific to the |X509_LOOKUP_add_dir| filesystem format and is
// not suitable for general-purpose X.509 name processing. It is very short, so
// there will be hash collisions. It also depends on an OpenSSL-specific
// canonicalization process.
OPENSSL_EXPORT uint32_t X509_subject_name_hash(X509 *x509);

// X509_issuer_name_hash_old returns the hash of |x509|'s issuer name with
// |X509_NAME_hash_old|.
//
// This hash is specific to the |X509_LOOKUP_add_dir| filesystem format and is
// not suitable for general-purpose X.509 name processing. It is very short, so
// there will be hash collisions.
OPENSSL_EXPORT uint32_t X509_issuer_name_hash_old(X509 *x509);

// X509_subject_name_hash_old returns the hash of |x509|'s usjbect name with
// |X509_NAME_hash_old|.
//
// This hash is specific to the |X509_LOOKUP_add_dir| filesystem format and is
// not suitable for general-purpose X.509 name processing. It is very short, so
// there will be hash collisions.
OPENSSL_EXPORT uint32_t X509_subject_name_hash_old(X509 *x509);


// ex_data functions.
//
// See |ex_data.h| for details.

OPENSSL_EXPORT int X509_get_ex_new_index(long argl, void *argp,
                                         CRYPTO_EX_unused *unused,
                                         CRYPTO_EX_dup *dup_unused,
                                         CRYPTO_EX_free *free_func);
OPENSSL_EXPORT int X509_set_ex_data(X509 *r, int idx, void *arg);
OPENSSL_EXPORT void *X509_get_ex_data(X509 *r, int idx);

OPENSSL_EXPORT int X509_STORE_CTX_get_ex_new_index(long argl, void *argp,
                                                   CRYPTO_EX_unused *unused,
                                                   CRYPTO_EX_dup *dup_unused,
                                                   CRYPTO_EX_free *free_func);
OPENSSL_EXPORT int X509_STORE_CTX_set_ex_data(X509_STORE_CTX *ctx, int idx,
                                              void *data);
OPENSSL_EXPORT void *X509_STORE_CTX_get_ex_data(X509_STORE_CTX *ctx, int idx);

OPENSSL_EXPORT int X509_STORE_get_ex_new_index(long argl, void *argp,
                                               CRYPTO_EX_unused *unused,
                                               CRYPTO_EX_dup *dup_unused,
                                               CRYPTO_EX_free *free_func);
OPENSSL_EXPORT int X509_STORE_set_ex_data(X509_STORE *ctx, int idx, void *data);
OPENSSL_EXPORT void *X509_STORE_get_ex_data(X509_STORE *ctx, int idx);

#define X509_STORE_CTX_set_app_data(ctx, data) \
  X509_STORE_CTX_set_ex_data(ctx, 0, data)
#define X509_STORE_CTX_get_app_data(ctx) X509_STORE_CTX_get_ex_data(ctx, 0)

// Hashing and signing ASN.1 structures.

// ASN1_digest serializes |data| with |i2d| and then hashes the result with
// |type|. On success, it returns one, writes the digest to |md|, and sets
// |*len| to the digest length if non-NULL. On error, it returns zero.
//
// |EVP_MD_CTX_size| bytes are written, which is at most |EVP_MAX_MD_SIZE|. The
// buffer must have sufficient space for this output.
OPENSSL_EXPORT int ASN1_digest(i2d_of_void *i2d, const EVP_MD *type, char *data,
                               unsigned char *md, unsigned int *len);

// ASN1_item_digest serializes |data| with |it| and then hashes the result with
// |type|. On success, it returns one, writes the digest to |md|, and sets
// |*len| to the digest length if non-NULL. On error, it returns zero.
//
// |EVP_MD_CTX_size| bytes are written, which is at most |EVP_MAX_MD_SIZE|. The
// buffer must have sufficient space for this output.
//
// WARNING: |data| must be a pointer with the same type as |it|'s corresponding
// C type. Using the wrong type is a potentially exploitable memory error.
OPENSSL_EXPORT int ASN1_item_digest(const ASN1_ITEM *it, const EVP_MD *type,
                                    void *data, unsigned char *md,
                                    unsigned int *len);

// ASN1_item_verify serializes |data| with |it| and then verifies |signature| is
// a valid signature for the result with |algor1| and |pkey|. It returns one on
// success and zero on error. The signature and algorithm are interpreted as in
// X.509.
//
// WARNING: |data| must be a pointer with the same type as |it|'s corresponding
// C type. Using the wrong type is a potentially exploitable memory error.
OPENSSL_EXPORT int ASN1_item_verify(const ASN1_ITEM *it,
                                    const X509_ALGOR *algor1,
                                    const ASN1_BIT_STRING *signature,
                                    void *data, EVP_PKEY *pkey);

// ASN1_item_sign serializes |data| with |it| and then signs the result with
// the private key |pkey|. It returns the length of the signature on success and
// zero on error. On success, it writes the signature to |signature| and the
// signature algorithm to each of |algor1| and |algor2|. Either of |algor1| or
// |algor2| may be NULL to ignore them. This function uses digest algorithm
// |md|, or |pkey|'s default if NULL. Other signing parameters use |pkey|'s
// defaults. To customize them, use |ASN1_item_sign_ctx|.
//
// WARNING: |data| must be a pointer with the same type as |it|'s corresponding
// C type. Using the wrong type is a potentially exploitable memory error.
OPENSSL_EXPORT int ASN1_item_sign(const ASN1_ITEM *it, X509_ALGOR *algor1,
                                  X509_ALGOR *algor2,
                                  ASN1_BIT_STRING *signature, void *data,
                                  EVP_PKEY *pkey, const EVP_MD *type);

// ASN1_item_sign_ctx behaves like |ASN1_item_sign| except the signature is
// signed with |ctx|, |ctx|, which must have been initialized with
// |EVP_DigestSignInit|. The caller should configure the corresponding
// |EVP_PKEY_CTX| with any additional parameters before calling this function.
//
// On success or failure, this function mutates |ctx| and resets it to the empty
// state. Caller should not rely on its contents after the function returns.
//
// WARNING: |data| must be a pointer with the same type as |it|'s corresponding
// C type. Using the wrong type is a potentially exploitable memory error.
OPENSSL_EXPORT int ASN1_item_sign_ctx(const ASN1_ITEM *it, X509_ALGOR *algor1,
                                      X509_ALGOR *algor2,
                                      ASN1_BIT_STRING *signature, void *asn,
                                      EVP_MD_CTX *ctx);


// Verification internals.
//
// The following functions expose portions of certificate validation. They are
// exported for compatibility with existing callers, or to support some obscure
// use cases. Most callers, however, will not need these functions and should
// instead use |X509_STORE_CTX| APIs.

// X509_supported_extension returns one if |ex| is a critical X.509 certificate
// extension, supported by |X509_verify_cert|, and zero otherwise.
//
// Note this function only reports certificate extensions (as opposed to CRL or
// CRL extensions), and only extensions that are expected to be marked critical.
// Additionally, |X509_verify_cert| checks for unsupported critical extensions
// internally, so most callers will not need to call this function separately.
OPENSSL_EXPORT int X509_supported_extension(const X509_EXTENSION *ex);

// X509_check_ca returns one if |x509| may be considered a CA certificate,
// according to basic constraints and key usage extensions. Otherwise, it
// returns zero. If |x509| is an X509v1 certificate, and thus has no extensions,
// it is considered eligible.
//
// This function returning one does not indicate that |x509| is trusted, only
// that it is eligible to be a CA.
//
// TODO(crbug.com/boringssl/407): |x509| should be const.
OPENSSL_EXPORT int X509_check_ca(X509 *x509);

// X509_check_issued checks if |issuer| and |subject|'s name, authority key
// identifier, and key usage fields allow |issuer| to have issued |subject|. It
// returns |X509_V_OK| on success and an |X509_V_ERR_*| value otherwise.
//
// This function does not check the signature on |subject|. Rather, it is
// intended to prune the set of possible issuer certificates during
// path-building.
//
// TODO(crbug.com/boringssl/407): Both parameters should be const.
OPENSSL_EXPORT int X509_check_issued(X509 *issuer, X509 *subject);

// NAME_CONSTRAINTS_check checks if |x509| satisfies name constraints in |nc|.
// It returns |X509_V_OK| on success and some |X509_V_ERR_*| constant on error.
//
// TODO(crbug.com/boringssl/407): Both parameters should be const.
OPENSSL_EXPORT int NAME_CONSTRAINTS_check(X509 *x509, NAME_CONSTRAINTS *nc);

// X509_check_host checks if |x509| matches the DNS name |chk|. It returns one
// on match, zero on mismatch, or a negative number on error. |flags| should be
// some combination of |X509_CHECK_FLAG_*| and modifies the behavior. On match,
// if |out_peername| is non-NULL, it additionally sets |*out_peername| to a
// newly-allocated, NUL-terminated string containing the DNS name or wildcard in
// the certificate which matched. The caller must then free |*out_peername| with
// |OPENSSL_free| when done.
//
// By default, both subject alternative names and the subject's common name
// attribute are checked. The latter has long been deprecated, so callers should
// include |X509_CHECK_FLAG_NEVER_CHECK_SUBJECT| in |flags| to use the standard
// behavior. https://crbug.com/boringssl/464 tracks fixing the default.
//
// This function does not check if |x509| is a trusted certificate, only if,
// were it trusted, it would match |chk|.
//
// WARNING: This function differs from the usual calling convention and may
// return either 0 or a negative number on error.
//
// TODO(davidben): Make the error case also return zero.
OPENSSL_EXPORT int X509_check_host(const X509 *x509, const char *chk,
                                   size_t chklen, unsigned int flags,
                                   char **out_peername);

// X509_check_email checks if |x509| matches the email address |chk|. It returns
// one on match, zero on mismatch, or a negative number on error. |flags| should
// be some combination of |X509_CHECK_FLAG_*| and modifies the behavior.
//
// By default, both subject alternative names and the subject's email address
// attribute are checked. The |X509_CHECK_FLAG_NEVER_CHECK_SUBJECT| flag may be
// used to change this behavior.
//
// This function does not check if |x509| is a trusted certificate, only if,
// were it trusted, it would match |chk|.
//
// WARNING: This function differs from the usual calling convention and may
// return either 0 or a negative number on error.
//
// TODO(davidben): Make the error case also return zero.
OPENSSL_EXPORT int X509_check_email(const X509 *x509, const char *chk,
                                    size_t chklen, unsigned int flags);

// X509_check_ip checks if |x509| matches the IP address |chk|. The IP address
// is represented in byte form and should be 4 bytes for an IPv4 address and 16
// bytes for an IPv6 address. It returns one on match, zero on mismatch, or a
// negative number on error. |flags| should be some combination of
// |X509_CHECK_FLAG_*| and modifies the behavior.
//
// This function does not check if |x509| is a trusted certificate, only if,
// were it trusted, it would match |chk|.
//
// WARNING: This function differs from the usual calling convention and may
// return either 0 or a negative number on error.
//
// TODO(davidben): Make the error case also return zero.
OPENSSL_EXPORT int X509_check_ip(const X509 *x509, const uint8_t *chk,
                                 size_t chklen, unsigned int flags);

// X509_check_ip_asc behaves like |X509_check_ip| except the IP address is
// specified in textual form in |ipasc|.
//
// WARNING: This function differs from the usual calling convention and may
// return either 0 or a negative number on error.
//
// TODO(davidben): Make the error case also return zero.
OPENSSL_EXPORT int X509_check_ip_asc(const X509 *x509, const char *ipasc,
                                     unsigned int flags);

// X509_STORE_CTX_get1_issuer looks up a candidate trusted issuer for |x509| out
// of |ctx|'s |X509_STORE|, based on the criteria in |X509_check_issued|. If one
// was found, it returns one and sets |*out_issuer| to the issuer. The caller
// must release |*out_issuer| with |X509_free| when done. If none was found, it
// returns zero and leaves |*out_issuer| unchanged.
//
// This function only searches for trusted issuers. It does not consider
// untrusted intermediates passed in to |X509_STORE_CTX_init|.
//
// TODO(crbug.com/boringssl/407): |x509| should be const.
OPENSSL_EXPORT int X509_STORE_CTX_get1_issuer(X509 **out_issuer,
                                              X509_STORE_CTX *ctx, X509 *x509);

// X509_check_purpose performs checks if |x509|'s basic constraints, key usage,
// and extended key usage extensions for the specified purpose. |purpose| should
// be one of |X509_PURPOSE_*| constants. See |X509_VERIFY_PARAM_set_purpose| for
// details. It returns one if |x509|'s extensions are consistent with |purpose|
// and zero otherwise. If |ca| is non-zero, |x509| is checked as a CA
// certificate. Otherwise, it is checked as an end-entity certificate.
//
// If |purpose| is -1, this function performs no purpose checks, but it parses
// some extensions in |x509| and may return zero on syntax error. Historically,
// callers primarily used this function to trigger this parsing, but this is no
// longer necessary. Functions acting on |X509| will internally parse as needed.
OPENSSL_EXPORT int X509_check_purpose(X509 *x509, int purpose, int ca);

#define X509_TRUST_TRUSTED 1
#define X509_TRUST_REJECTED 2
#define X509_TRUST_UNTRUSTED 3

// X509_check_trust checks if |x509| is a valid trust anchor for trust type
// |id|. See |X509_VERIFY_PARAM_set_trust| for details. It returns
// |X509_TRUST_TRUSTED| if |x509| is a trust anchor, |X509_TRUST_REJECTED| if it
// was distrusted, and |X509_TRUST_UNTRUSTED| otherwise. |id| should be one of
// the |X509_TRUST_*| constants, or zero to indicate the default behavior.
// |flags| should be zero and is ignored.
OPENSSL_EXPORT int X509_check_trust(X509 *x509, int id, int flags);

// X.509 information.
//
// |X509_INFO| is the return type for |PEM_X509_INFO_read_bio|, defined in
// <openssl/pem.h>. It is used to store a certificate, CRL, or private key. This
// type is defined in this header for OpenSSL compatibility.

struct private_key_st {
  EVP_PKEY *dec_pkey;
} /* X509_PKEY */;

struct X509_info_st {
  X509 *x509;
  X509_CRL *crl;
  X509_PKEY *x_pkey;

  EVP_CIPHER_INFO enc_cipher;
  int enc_len;
  char *enc_data;
} /* X509_INFO */;

DEFINE_STACK_OF(X509_INFO)

// X509_INFO_free releases memory associated with |info|.
OPENSSL_EXPORT void X509_INFO_free(X509_INFO *info);


// Deprecated custom extension registration.
//
// The following functions allow callers to register custom extensions for use
// with |X509V3_EXT_d2i| and related functions. This mechanism is deprecated and
// will be removed in the future. As discussed in |X509V3_EXT_add|, it is not
// possible to safely register a custom extension without risking race
// conditions and memory errors when linked with other users of BoringSSL.
//
// Moreover, it is not necessary to register a custom extension to process
// extensions unknown to BoringSSL. Registration does not impact certificate
// verification. Caller should instead use functions such as
// |ASN1_OBJECT_create|, |X509_get_ext_by_OBJ|, |X509_EXTENSION_get_data|, and
// |X509_EXTENSION_create_by_OBJ| to inspect or create extensions directly.

// The following function pointer types are used in |X509V3_EXT_METHOD|.
typedef void *(*X509V3_EXT_NEW)(void);
typedef void (*X509V3_EXT_FREE)(void *ext);
typedef void *(*X509V3_EXT_D2I)(void *ext, const uint8_t **inp, long len);
typedef int (*X509V3_EXT_I2D)(void *ext, uint8_t **outp);
typedef STACK_OF(CONF_VALUE) *(*X509V3_EXT_I2V)(const X509V3_EXT_METHOD *method,
                                                void *ext,
                                                STACK_OF(CONF_VALUE) *extlist);
typedef void *(*X509V3_EXT_V2I)(const X509V3_EXT_METHOD *method,
                                const X509V3_CTX *ctx,
                                const STACK_OF(CONF_VALUE) *values);
typedef char *(*X509V3_EXT_I2S)(const X509V3_EXT_METHOD *method, void *ext);
typedef void *(*X509V3_EXT_S2I)(const X509V3_EXT_METHOD *method,
                                const X509V3_CTX *ctx, const char *str);
typedef int (*X509V3_EXT_I2R)(const X509V3_EXT_METHOD *method, void *ext,
                              BIO *out, int indent);
typedef void *(*X509V3_EXT_R2I)(const X509V3_EXT_METHOD *method,
                                const X509V3_CTX *ctx, const char *str);

// A v3_ext_method, aka |X509V3_EXT_METHOD|, is a deprecated type which defines
// a custom extension.
struct v3_ext_method {
  // ext_nid is the NID of the extension.
  int ext_nid;

  // ext_flags is a combination of |X509V3_EXT_*| constants.
  int ext_flags;

  // it determines how values of this extension are allocated, released, parsed,
  // and marshalled. This must be non-NULL.
  ASN1_ITEM_EXP *it;

  // The following functions are ignored in favor of |it|. They are retained in
  // the struct only for source compatibility with existing struct definitions.
  X509V3_EXT_NEW ext_new;
  X509V3_EXT_FREE ext_free;
  X509V3_EXT_D2I d2i;
  X509V3_EXT_I2D i2d;

  // The following functions are used for string extensions.
  X509V3_EXT_I2S i2s;
  X509V3_EXT_S2I s2i;

  // The following functions are used for multi-valued extensions.
  X509V3_EXT_I2V i2v;
  X509V3_EXT_V2I v2i;

  // The following functions are used for "raw" extensions, which implement
  // custom printing behavior.
  X509V3_EXT_I2R i2r;
  X509V3_EXT_R2I r2i;

  void *usr_data;  // Any extension specific data
} /* X509V3_EXT_METHOD */;

// X509V3_EXT_MULTILINE causes the result of an |X509V3_EXT_METHOD|'s |i2v|
// function to be printed on separate lines, rather than separated by commas.
#define X509V3_EXT_MULTILINE 0x4

// X509V3_EXT_get returns the |X509V3_EXT_METHOD| corresponding to |ext|'s
// extension type, or NULL if none was registered.
OPENSSL_EXPORT const X509V3_EXT_METHOD *X509V3_EXT_get(
    const X509_EXTENSION *ext);

// X509V3_EXT_get_nid returns the |X509V3_EXT_METHOD| corresponding to |nid|, or
// NULL if none was registered.
OPENSSL_EXPORT const X509V3_EXT_METHOD *X509V3_EXT_get_nid(int nid);

// X509V3_EXT_add registers |ext| as a custom extension for the extension type
// |ext->ext_nid|. |ext| must be valid for the remainder of the address space's
// lifetime. It returns one on success and zero on error.
//
// WARNING: This function modifies global state. If other code in the same
// address space also registers an extension with type |ext->ext_nid|, the two
// registrations will conflict. Which registration takes effect is undefined. If
// the two registrations use incompatible in-memory representations, code
// expecting the other registration will then cast a type to the wrong type,
// resulting in a potentially exploitable memory error. This conflict can also
// occur if BoringSSL later adds support for |ext->ext_nid|, with a different
// in-memory representation than the one expected by |ext|.
//
// This function, additionally, is not thread-safe and cannot be called
// concurrently with any other BoringSSL function.
//
// As a result, it is impossible to safely use this function. Registering a
// custom extension has no impact on certificate verification so, instead,
// callers should simply handle the custom extension with the byte-based
// |X509_EXTENSION| APIs directly. Registering |ext| with the library has little
// practical value.
OPENSSL_EXPORT OPENSSL_DEPRECATED int X509V3_EXT_add(X509V3_EXT_METHOD *ext);

// X509V3_EXT_add_alias registers a custom extension with NID |nid_to|. The
// corresponding ASN.1 type is copied from |nid_from|. It returns one on success
// and zero on error.
//
// WARNING: Do not use this function. See |X509V3_EXT_add|.
OPENSSL_EXPORT OPENSSL_DEPRECATED int X509V3_EXT_add_alias(int nid_to,
                                                           int nid_from);


// Deprecated config-based extension creation.
//
// The following functions allow specifying X.509 extensions using OpenSSL's
// config file syntax, from the OpenSSL command-line tool. They are retained,
// for now, for compatibility with legacy software but may be removed in the
// future. Construct the extensions using the typed C APIs instead.
//
// Callers should especially avoid these functions if passing in non-constant
// values. They use ad-hoc, string-based formats which are prone to injection
// vulnerabilities. For a CA, this means using them risks misissuance.
//
// These functions are not safe to use with untrusted inputs. The string formats
// may implicitly reference context information and, in OpenSSL (though not
// BoringSSL), one even allows reading arbitrary files. Many formats can also
// produce far larger outputs than their inputs, so untrusted inputs may lead to
// denial-of-service attacks. Finally, the parsers see much less testing and
// review than most of the library and may have bugs including memory leaks or
// crashes.

// v3_ext_ctx, aka |X509V3_CTX|, contains additional context information for
// constructing extensions. Some string formats reference additional values in
// these objects. It must be initialized with |X509V3_set_ctx| or
// |X509V3_set_ctx_test| before use.
struct v3_ext_ctx {
  int flags;
  const X509 *issuer_cert;
  const X509 *subject_cert;
  const X509_REQ *subject_req;
  const X509_CRL *crl;
  const CONF *db;
};

#define X509V3_CTX_TEST 0x1
#define X509V3_CTX_REPLACE 0x2

// X509V3_set_ctx initializes |ctx| with the specified objects. Some string
// formats will reference fields in these objects. Each object may be NULL to
// omit it, in which case those formats cannot be used. |flags| should be zero,
// unless called via |X509V3_set_ctx_test|.
//
// |issuer|, |subject|, |req|, and |crl|, if non-NULL, must outlive |ctx|.
OPENSSL_EXPORT void X509V3_set_ctx(X509V3_CTX *ctx, const X509 *issuer,
                                   const X509 *subject, const X509_REQ *req,
                                   const X509_CRL *crl, int flags);

// X509V3_set_ctx_test calls |X509V3_set_ctx| without any reference objects and
// mocks out some features that use them. The resulting extensions may be
// incomplete and should be discarded. This can be used to partially validate
// syntax.
//
// TODO(davidben): Can we remove this?
#define X509V3_set_ctx_test(ctx) \
  X509V3_set_ctx(ctx, NULL, NULL, NULL, NULL, X509V3_CTX_TEST)

// X509V3_set_nconf sets |ctx| to use |conf| as the config database. |ctx| must
// have previously been initialized by |X509V3_set_ctx| or
// |X509V3_set_ctx_test|. Some string formats will reference sections in |conf|.
// |conf| may be NULL, in which case these formats cannot be used. If non-NULL,
// |conf| must outlive |ctx|.
OPENSSL_EXPORT void X509V3_set_nconf(X509V3_CTX *ctx, const CONF *conf);

// X509V3_set_ctx_nodb calls |X509V3_set_nconf| with no config database.
#define X509V3_set_ctx_nodb(ctx) X509V3_set_nconf(ctx, NULL)

// X509V3_EXT_nconf constructs an extension of type specified by |name|, and
// value specified by |value|. It returns a newly-allocated |X509_EXTENSION|
// object on success, or NULL on error. |conf| and |ctx| specify additional
// information referenced by some formats. Either |conf| or |ctx| may be NULL,
// in which case features which use it will be disabled.
//
// If non-NULL, |ctx| must be initialized with |X509V3_set_ctx| or
// |X509V3_set_ctx_test|.
//
// Both |conf| and |ctx| provide a |CONF| object. When |ctx| is non-NULL, most
// features use the |ctx| copy, configured with |X509V3_set_ctx|, but some use
// |conf|. Callers should ensure the two match to avoid surprisingly behavior.
OPENSSL_EXPORT X509_EXTENSION *X509V3_EXT_nconf(const CONF *conf,
                                                const X509V3_CTX *ctx,
                                                const char *name,
                                                const char *value);

// X509V3_EXT_nconf_nid behaves like |X509V3_EXT_nconf|, except the extension
// type is specified as a NID.
OPENSSL_EXPORT X509_EXTENSION *X509V3_EXT_nconf_nid(const CONF *conf,
                                                    const X509V3_CTX *ctx,
                                                    int ext_nid,
                                                    const char *value);

// X509V3_EXT_conf_nid calls |X509V3_EXT_nconf_nid|. |conf| must be NULL.
//
// TODO(davidben): This is the only exposed instance of an LHASH in our public
// headers. cryptography.io wraps this function so we cannot, yet, replace the
// type with a dummy struct.
OPENSSL_EXPORT X509_EXTENSION *X509V3_EXT_conf_nid(LHASH_OF(CONF_VALUE) *conf,
                                                   const X509V3_CTX *ctx,
                                                   int ext_nid,
                                                   const char *value);

// X509V3_EXT_add_nconf_sk looks up the section named |section| in |conf|. For
// each |CONF_VALUE| in the section, it constructs an extension as in
// |X509V3_EXT_nconf|, taking |name| and |value| from the |CONF_VALUE|. Each new
// extension is appended to |*sk|. If |*sk| is non-NULL, and at least one
// extension is added, it sets |*sk| to a newly-allocated
// |STACK_OF(X509_EXTENSION)|. It returns one on success and zero on error.
OPENSSL_EXPORT int X509V3_EXT_add_nconf_sk(const CONF *conf,
                                           const X509V3_CTX *ctx,
                                           const char *section,
                                           STACK_OF(X509_EXTENSION) **sk);

// X509V3_EXT_add_nconf adds extensions to |cert| as in
// |X509V3_EXT_add_nconf_sk|. It returns one on success and zero on error.
OPENSSL_EXPORT int X509V3_EXT_add_nconf(const CONF *conf, const X509V3_CTX *ctx,
                                        const char *section, X509 *cert);

// X509V3_EXT_REQ_add_nconf adds extensions to |req| as in
// |X509V3_EXT_add_nconf_sk|. It returns one on success and zero on error.
OPENSSL_EXPORT int X509V3_EXT_REQ_add_nconf(const CONF *conf,
                                            const X509V3_CTX *ctx,
                                            const char *section, X509_REQ *req);

// X509V3_EXT_CRL_add_nconf adds extensions to |crl| as in
// |X509V3_EXT_add_nconf_sk|. It returns one on success and zero on error.
OPENSSL_EXPORT int X509V3_EXT_CRL_add_nconf(const CONF *conf,
                                            const X509V3_CTX *ctx,
                                            const char *section, X509_CRL *crl);

// X509V3_EXT_conf calls |X509V3_EXT_nconf|. |conf| must be NULL.
//
// NOTE: This is only provided for compatibility. See |X509V3_EXT_nconf|
// instead.
OPENSSL_EXPORT X509_EXTENSION *X509V3_EXT_conf(LHASH_OF(CONF_VALUE) *conf,
                                               X509V3_CTX *ctx,
                                               const char *name,
                                               const char *value);

// i2s_ASN1_OCTET_STRING returns a human-readable representation of |oct| as a
// newly-allocated, NUL-terminated string, or NULL on error. |method| is
// ignored. The caller must release the result with |OPENSSL_free| when done.
OPENSSL_EXPORT char *i2s_ASN1_OCTET_STRING(const X509V3_EXT_METHOD *method,
                                           const ASN1_OCTET_STRING *oct);

// s2i_ASN1_OCTET_STRING decodes |str| as a hexdecimal byte string, with
// optional colon separators between bytes. It returns a newly-allocated
// |ASN1_OCTET_STRING| with the result on success, or NULL on error. |method|
// and |ctx| are ignored.
OPENSSL_EXPORT ASN1_OCTET_STRING *s2i_ASN1_OCTET_STRING(
    const X509V3_EXT_METHOD *method, const X509V3_CTX *ctx, const char *str);

// i2s_ASN1_INTEGER returns a human-readable representation of |aint| as a
// newly-allocated, NUL-terminated string, or NULL on error. |method| is
// ignored. The caller must release the result with |OPENSSL_free| when done.
OPENSSL_EXPORT char *i2s_ASN1_INTEGER(const X509V3_EXT_METHOD *method,
                                      const ASN1_INTEGER *aint);

// s2i_ASN1_INTEGER decodes |value| as the ASCII representation of an integer,
// and returns a newly-allocated |ASN1_INTEGER| containing the result, or NULL
// on error. |method| is ignored. If |value| begins with "0x" or "0X", the input
// is decoded in hexadecimal, otherwise decimal.
OPENSSL_EXPORT ASN1_INTEGER *s2i_ASN1_INTEGER(const X509V3_EXT_METHOD *method,
                                              const char *value);

// i2s_ASN1_ENUMERATED returns a human-readable representation of |aint| as a
// newly-allocated, NUL-terminated string, or NULL on error. |method| is
// ignored. The caller must release the result with |OPENSSL_free| when done.
OPENSSL_EXPORT char *i2s_ASN1_ENUMERATED(const X509V3_EXT_METHOD *method,
                                         const ASN1_ENUMERATED *aint);

// X509V3_conf_free releases memory associated with |CONF_VALUE|.
OPENSSL_EXPORT void X509V3_conf_free(CONF_VALUE *val);

// i2v_GENERAL_NAME serializes |gen| as a |CONF_VALUE|. If |ret| is non-NULL, it
// appends the value to |ret| and returns |ret| on success or NULL on error. If
// it returns NULL, the caller is still responsible for freeing |ret|. If |ret|
// is NULL, it returns a newly-allocated |STACK_OF(CONF_VALUE)| containing the
// result. |method| is ignored. When done, the caller should release the result
// with |sk_CONF_VALUE_pop_free| and |X509V3_conf_free|.
//
// Do not use this function. This is an internal implementation detail of the
// human-readable print functions. If extracting a SAN list from a certificate,
// look at |gen| directly.
OPENSSL_EXPORT STACK_OF(CONF_VALUE) *i2v_GENERAL_NAME(
    const X509V3_EXT_METHOD *method, const GENERAL_NAME *gen,
    STACK_OF(CONF_VALUE) *ret);

// i2v_GENERAL_NAMES serializes |gen| as a list of |CONF_VALUE|s. If |ret| is
// non-NULL, it appends the values to |ret| and returns |ret| on success or NULL
// on error. If it returns NULL, the caller is still responsible for freeing
// |ret|. If |ret| is NULL, it returns a newly-allocated |STACK_OF(CONF_VALUE)|
// containing the results. |method| is ignored.
//
// Do not use this function. This is an internal implementation detail of the
// human-readable print functions. If extracting a SAN list from a certificate,
// look at |gen| directly.
OPENSSL_EXPORT STACK_OF(CONF_VALUE) *i2v_GENERAL_NAMES(
    const X509V3_EXT_METHOD *method, const GENERAL_NAMES *gen,
    STACK_OF(CONF_VALUE) *extlist);

// a2i_IPADDRESS decodes |ipasc| as the textual representation of an IPv4 or
// IPv6 address. On success, it returns a newly-allocated |ASN1_OCTET_STRING|
// containing the decoded IP address. IPv4 addresses are represented as 4-byte
// strings and IPv6 addresses as 16-byte strings. On failure, it returns NULL.
OPENSSL_EXPORT ASN1_OCTET_STRING *a2i_IPADDRESS(const char *ipasc);

// a2i_IPADDRESS_NC decodes |ipasc| as the textual representation of an IPv4 or
// IPv6 address range. On success, it returns a newly-allocated
// |ASN1_OCTET_STRING| containing the decoded IP address, followed by the
// decoded mask. IPv4 ranges are represented as 8-byte strings and IPv6 ranges
// as 32-byte strings. On failure, it returns NULL.
//
// The text format decoded by this function is not the standard CIDR notiation.
// Instead, the mask after the "/" is represented as another IP address. For
// example, "192.168.0.0/16" would be written "192.168.0.0/255.255.0.0".
OPENSSL_EXPORT ASN1_OCTET_STRING *a2i_IPADDRESS_NC(const char *ipasc);


// Deprecated functions.

// X509_get_notBefore returns |x509|'s notBefore time. Note this function is not
// const-correct for legacy reasons. Use |X509_get0_notBefore| or
// |X509_getm_notBefore| instead.
OPENSSL_EXPORT ASN1_TIME *X509_get_notBefore(const X509 *x509);

// X509_get_notAfter returns |x509|'s notAfter time. Note this function is not
// const-correct for legacy reasons. Use |X509_get0_notAfter| or
// |X509_getm_notAfter| instead.
OPENSSL_EXPORT ASN1_TIME *X509_get_notAfter(const X509 *x509);

// X509_set_notBefore calls |X509_set1_notBefore|. Use |X509_set1_notBefore|
// instead.
OPENSSL_EXPORT int X509_set_notBefore(X509 *x509, const ASN1_TIME *tm);

// X509_set_notAfter calls |X509_set1_notAfter|. Use |X509_set1_notAfter|
// instead.
OPENSSL_EXPORT int X509_set_notAfter(X509 *x509, const ASN1_TIME *tm);

// X509_CRL_get_lastUpdate returns a mutable pointer to |crl|'s thisUpdate time.
// The OpenSSL API refers to this field as lastUpdate.
//
// Use |X509_CRL_get0_lastUpdate| or |X509_CRL_set1_lastUpdate| instead.
OPENSSL_EXPORT ASN1_TIME *X509_CRL_get_lastUpdate(X509_CRL *crl);

// X509_CRL_get_nextUpdate returns a mutable pointer to |crl|'s nextUpdate time,
// or NULL if |crl| has none. Use |X509_CRL_get0_nextUpdate| or
// |X509_CRL_set1_nextUpdate| instead.
OPENSSL_EXPORT ASN1_TIME *X509_CRL_get_nextUpdate(X509_CRL *crl);

// X509_extract_key is a legacy alias to |X509_get_pubkey|. Use
// |X509_get_pubkey| instead.
#define X509_extract_key(x) X509_get_pubkey(x)

// X509_REQ_extract_key is a legacy alias for |X509_REQ_get_pubkey|.
#define X509_REQ_extract_key(a) X509_REQ_get_pubkey(a)

// X509_name_cmp is a legacy alias for |X509_NAME_cmp|.
#define X509_name_cmp(a, b) X509_NAME_cmp((a), (b))

// The following symbols are deprecated aliases to |X509_CRL_set1_*|.
#define X509_CRL_set_lastUpdate X509_CRL_set1_lastUpdate
#define X509_CRL_set_nextUpdate X509_CRL_set1_nextUpdate

// X509_get_serialNumber returns a mutable pointer to |x509|'s serial number.
// Prefer |X509_get0_serialNumber|.
OPENSSL_EXPORT ASN1_INTEGER *X509_get_serialNumber(X509 *x509);

// X509_NAME_get_text_by_OBJ finds the first attribute with type |obj| in
// |name|. If found, it writes the value's UTF-8 representation to |buf|.
// followed by a NUL byte, and returns the number of bytes in the output,
// excluding the NUL byte. This is unlike OpenSSL which returns the raw
// ASN1_STRING data. The UTF-8 encoding of the |ASN1_STRING| may not contain a 0
// codepoint.
//
// This function writes at most |len| bytes, including the NUL byte.  If |buf|
// is NULL, it writes nothing and returns the number of bytes in the
// output, excluding the NUL byte that would be required for the full UTF-8
// output.
//
// This function may return -1 if an error occurs for any reason, including the
// value not being a recognized string type, |len| being of insufficient size to
// hold the full UTF-8 encoding and NUL byte, memory allocation failures, an
// object with type |obj| not existing in |name|, or if the UTF-8 encoding of
// the string contains a zero byte.
OPENSSL_EXPORT int X509_NAME_get_text_by_OBJ(const X509_NAME *name,
                                             const ASN1_OBJECT *obj, char *buf,
                                             int len);

// X509_NAME_get_text_by_NID behaves like |X509_NAME_get_text_by_OBJ| except it
// finds an attribute of type |nid|, which should be one of the |NID_*|
// constants.
OPENSSL_EXPORT int X509_NAME_get_text_by_NID(const X509_NAME *name, int nid,
                                             char *buf, int len);

// X509_STORE_CTX_get0_parent_ctx returns NULL.
OPENSSL_EXPORT X509_STORE_CTX *X509_STORE_CTX_get0_parent_ctx(
    X509_STORE_CTX *ctx);

// X509_LOOKUP_free releases memory associated with |ctx|. This function should
// never be used outside the library. No function in the public API hands
// ownership of an |X509_LOOKUP| to the caller.
//
// TODO(davidben): Unexport this function after rust-openssl is fixed to no
// longer call it.
OPENSSL_EXPORT void X509_LOOKUP_free(X509_LOOKUP *ctx);

// X509_STORE_CTX_cleanup resets |ctx| to the empty state.
//
// This function is a remnant of when |X509_STORE_CTX| was stack-allocated and
// should not be used. If releasing |ctx|, call |X509_STORE_CTX_free|. If
// reusing |ctx| for a new verification, release the old one and create a new
// one.
OPENSSL_EXPORT void X509_STORE_CTX_cleanup(X509_STORE_CTX *ctx);

// X509V3_add_standard_extensions returns one.
OPENSSL_EXPORT int X509V3_add_standard_extensions(void);

OPENSSL_EXPORT STACK_OF(CONF_VALUE) *X509V3_parse_list(const char *line);

// The following symbols are legacy aliases for |X509_STORE_CTX| functions.
#define X509_STORE_get1_certs X509_STORE_CTX_get1_certs
#define X509_STORE_get1_crls X509_STORE_CTX_get1_crls

// X509_STORE_CTX_get_chain is a legacy alias for |X509_STORE_CTX_get0_chain|.
OPENSSL_EXPORT STACK_OF(X509) *X509_STORE_CTX_get_chain(X509_STORE_CTX *ctx);

// X509_STORE_CTX_trusted_stack is a deprecated alias for
// |X509_STORE_CTX_set0_trusted_stack|.
OPENSSL_EXPORT void X509_STORE_CTX_trusted_stack(X509_STORE_CTX *ctx,
                                                 STACK_OF(X509) *sk);

typedef int (*X509_STORE_CTX_verify_cb)(int, X509_STORE_CTX *);

// X509_STORE_CTX_set_verify_cb configures a callback function for |ctx| that is
// called multiple times during |X509_verify_cert|. The callback returns zero to
// fail verification and one to proceed. Typically, it will return |ok|, which
// preserves the default behavior. Returning one when |ok| is zero will proceed
// past some error. The callback may inspect |ctx| and the error queue to
// attempt to determine the current stage of certificate verification, but this
// is often unreliable. When synthesizing an error, callbacks should use
// |X509_STORE_CTX_set_error| to set a corresponding error.
//
// WARNING: Do not use this function. It is extremely fragile and unpredictable.
// This callback exposes implementation details of certificate verification,
// which change as the library evolves. Attempting to use it for security checks
// can introduce vulnerabilities if making incorrect assumptions about when the
// callback is called. Some errors, when suppressed, may implicitly suppress
// other errors due to internal implementation details. Additionally, overriding
// |ok| may leave |ctx| in an inconsistent state and break invariants.
//
// Instead, customize certificate verification by configuring options on the
// |X509_STORE_CTX| before verification, or applying additional checks after
// |X509_verify_cert| completes successfully.
OPENSSL_EXPORT void X509_STORE_CTX_set_verify_cb(
    X509_STORE_CTX *ctx, int (*verify_cb)(int ok, X509_STORE_CTX *ctx));

OPENSSL_EXPORT X509_STORE_CTX_verify_cb
X509_STORE_get_verify_cb(X509_STORE *ctx);

// X509_STORE_set_verify_cb acts like |X509_STORE_CTX_set_verify_cb| but sets
// the verify callback for any |X509_STORE_CTX| created from this |X509_STORE|
//
// Do not use this function. See |X509_STORE_CTX_set_verify_cb| for details.
OPENSSL_EXPORT void X509_STORE_set_verify_cb(
    X509_STORE *store, X509_STORE_CTX_verify_cb verify_cb);

// X509_STORE_set_verify_cb_func is a deprecated alias for
// |X509_STORE_set_verify_cb|.
#define X509_STORE_set_verify_cb_func(store, func) \
  X509_STORE_set_verify_cb((store), (func))

typedef int (*X509_STORE_CTX_get_crl_fn)(X509_STORE_CTX *ctx, X509_CRL **crl,
                                         X509 *x);
typedef int (*X509_STORE_CTX_check_crl_fn)(X509_STORE_CTX *ctx, X509_CRL *crl);

// X509_STORE_set_get_crl override's |store|'s logic for looking up CRLs.
//
// Do not use this function. It is temporarily retained to support one caller
// and will be removed after that caller is fixed. It is not possible for
// external callers to correctly implement this callback. The real
// implementation sets some inaccessible internal state on |X509_STORE_CTX|.
OPENSSL_EXPORT void X509_STORE_set_get_crl(X509_STORE *store,
                                           X509_STORE_CTX_get_crl_fn get_crl);

// X509_STORE_set_check_crl override's |store|'s logic for checking CRL
// validity.
//
// Do not use this function. It is temporarily retained to support one caller
// and will be removed after that caller is fixed. It is not possible for
// external callers to correctly implement this callback. The real
// implementation relies some inaccessible internal state on |X509_STORE_CTX|.
OPENSSL_EXPORT void X509_STORE_set_check_crl(
    X509_STORE *store, X509_STORE_CTX_check_crl_fn check_crl);

// X509_STORE_CTX_set_chain configures |ctx| to use |sk| for untrusted
// intermediate certificates to use in verification. This function is redundant
// with the |chain| parameter of |X509_STORE_CTX_init|. Use the parameter
// instead.
//
// WARNING: Despite the similar name, this function is unrelated to
// |X509_STORE_CTX_get0_chain|.
//
// WARNING: This function saves a pointer to |sk| without copying or
// incrementing reference counts. |sk| must outlive |ctx| and may not be mutated
// for the duration of the certificate verification.
OPENSSL_EXPORT void X509_STORE_CTX_set_chain(X509_STORE_CTX *ctx,
                                             STACK_OF(X509) *sk);

// X509_STORE_CTX_set0_untrusted is an alias for  |X509_STORE_CTX_set_chain|.
OPENSSL_EXPORT void X509_STORE_CTX_set0_untrusted(X509_STORE_CTX *ctx,
                                                  STACK_OF(X509) *sk);

// The following flags do nothing. The corresponding non-standard options have
// been removed.
#define X509_CHECK_FLAG_MULTI_LABEL_WILDCARDS 0

// X509_CHECK_FLAG_NO_PARTIAL_WILDCARDS does nothing, but is necessary in
// OpenSSL to enable standard wildcard matching. In AWS-LC, this behavior is
// always enabled.
#define X509_CHECK_FLAG_NO_PARTIAL_WILDCARDS 0

// X509_PURPOSE stuff

#define NS_SSL_CLIENT 0x80
#define NS_SSL_SERVER 0x40
#define NS_SMIME 0x20
#define NS_OBJSIGN 0x10
#define NS_SSL_CA 0x04
#define NS_SMIME_CA 0x02
#define NS_OBJSIGN_CA 0x01
#define NS_ANY_CA (NS_SSL_CA | NS_SMIME_CA | NS_OBJSIGN_CA)

typedef struct x509_purpose_st {
  int purpose;
  int trust;  // Default trust ID
  int flags;
  int (*check_purpose)(const struct x509_purpose_st *, const X509 *, int);
  char *name;
  char *sname;
  void *usr_data;
} X509_PURPOSE;

DEFINE_STACK_OF(X509_PURPOSE)

// X509_STORE_get0_objects returns a non-owning pointer of |store|'s internal
// object list. Although this function is not const, callers must not modify
// the result of this function.
//
// WARNING: This function is not thread-safe. If |store| is shared across
// multiple threads, callers cannot safely inspect the result of this function,
// because another thread may have concurrently added to it. In particular,
// |X509_LOOKUP_add_dir| treats this list as a cache and may add to it in the
// course of certificate verification. This API additionally prevents fixing
// some quadratic worst-case behavior in |X509_STORE| and may be removed in the
// future. Use |X509_STORE_get1_objects| instead.
OPENSSL_EXPORT STACK_OF(X509_OBJECT) *X509_STORE_get0_objects(
    X509_STORE *store);

// X509_PURPOSE_get_by_sname returns the |X509_PURPOSE_*| constant corresponding
// a short name |sname|, or -1 if |sname| was not recognized.
//
// Use |X509_PURPOSE_*| constants directly instead. The short names used by this
// function look like "sslserver" or "smimeencrypt", so they do not make
// especially good APIs.
//
// This function differs from OpenSSL, which returns an "index" to be passed to
// |X509_PURPOSE_get0|, followed by |X509_PURPOSE_get_id|, to finally obtain an
// |X509_PURPOSE_*| value suitable for use with |X509_VERIFY_PARAM_set_purpose|.
OPENSSL_EXPORT int X509_PURPOSE_get_by_sname(const char *sname);

// X509_PURPOSE_get0 returns the |X509_PURPOSE| object corresponding to |id|,
// which should be one of the |X509_PURPOSE_*| constants, or NULL if none
// exists.
//
// This function differs from OpenSSL, which takes an "index", returned from
// |X509_PURPOSE_get_by_sname|. In BoringSSL, indices and |X509_PURPOSE_*| IDs
// are the same.
OPENSSL_EXPORT const X509_PURPOSE *X509_PURPOSE_get0(int id);

// X509_PURPOSE_get_id returns |purpose|'s ID. This will be one of the
// |X509_PURPOSE_*| constants.
OPENSSL_EXPORT int X509_PURPOSE_get_id(const X509_PURPOSE *purpose);


// Private structures.

struct X509_algor_st {
  ASN1_OBJECT *algorithm;
  ASN1_TYPE *parameter;
} /* X509_ALGOR */;


// Underdocumented functions.
//
// TODO(https://crbug.com/boringssl/426): Functions below this point have not
// yet been documented or organized into sections.

DECLARE_STACK_OF(DIST_POINT)

// This is used for a table of trust checking functions

struct x509_trust_st {
  int trust;
  int flags;
  int (*check_trust)(const X509_TRUST *, X509 *);
  char *name;
  int arg1;
  void *arg2;
} /* X509_TRUST */;

DEFINE_STACK_OF(X509_TRUST)

OPENSSL_EXPORT int X509_TRUST_set(int *t, int trust);
OPENSSL_EXPORT int X509_TRUST_get_count(void);
OPENSSL_EXPORT const X509_TRUST *X509_TRUST_get0(int idx);
OPENSSL_EXPORT int X509_TRUST_get_by_id(int id);
OPENSSL_EXPORT int X509_TRUST_get_flags(const X509_TRUST *xp);
OPENSSL_EXPORT char *X509_TRUST_get0_name(const X509_TRUST *xp);
OPENSSL_EXPORT int X509_TRUST_get_trust(const X509_TRUST *xp);
// X509_TRUST_cleanup intentionally does nothing.
// This function is maintained only for compatibility with applications
// that consume OpenSSL APIs. AWS-LC does not support the related
// static trust settings functions which were also deprecated in OpenSSL 1.1.0.
OPENSSL_EXPORT OPENSSL_DEPRECATED void X509_TRUST_cleanup(void);

#define X509_LU_NONE 0
#define X509_LU_X509 1
#define X509_LU_CRL 2
#define X509_LU_PKEY 3

// Internal use: mask of policy related options (hidden)

#define X509_V_FLAG_POLICY_MASK                             \
  (X509_V_FLAG_POLICY_CHECK | X509_V_FLAG_EXPLICIT_POLICY | \
   X509_V_FLAG_INHIBIT_ANY | X509_V_FLAG_INHIBIT_MAP)

// X509_OBJECT_get0_X509_CRL returns the |X509_CRL| associated with |a|
OPENSSL_EXPORT X509_CRL *X509_OBJECT_get0_X509_CRL(const X509_OBJECT *a);

// X509_OBJECT_set1_X509 sets |obj| on |a| and uprefs |obj|. As with other set1
// methods, |a| does not take ownership of |obj|; the caller is responsible for
// managing freeing |obj| when appropriate.
OPENSSL_EXPORT int X509_OBJECT_set1_X509(X509_OBJECT *a, X509 *obj);

// X509_OBJECT_set1_X509_CRL sets CRL |obj| on |a| and uprefs |obj|. As with
// other set1 methods, |a| does not take ownership of |obj|; the caller is
// responsible for managing freeing |obj| when appropriate.
OPENSSL_EXPORT int X509_OBJECT_set1_X509_CRL(X509_OBJECT *a, X509_CRL *obj);

// X509_STORE_lock takes a write lock on |v|. return 1 on success, 0 on failure.
//
// Avoid operations on the X509_STORE or a X509_STORE_CTX containing it while
// it is locked; many |X509_STORE_*| functions take this lock internally which
// will cause a deadlock when called on a locked store.
OPENSSL_EXPORT int X509_STORE_lock(X509_STORE *v);

// X509_STORE_unlock releases a lock on |v|. return 1 on success, 0 on failure
OPENSSL_EXPORT int X509_STORE_unlock(X509_STORE *v);

OPENSSL_EXPORT STACK_OF(X509) *X509_STORE_CTX_get1_certs(X509_STORE_CTX *st,
                                                         X509_NAME *nm);
OPENSSL_EXPORT STACK_OF(X509_CRL) *X509_STORE_CTX_get1_crls(X509_STORE_CTX *st,
                                                            X509_NAME *nm);


// X509_STORE_get_by_subject is an alias to |X509_STORE_CTX_get_by_subject| in
// OpenSSL 1.1.1.
#define X509_STORE_get_by_subject X509_STORE_CTX_get_by_subject

// X509_STORE_CTX_get_by_subject tries to find an object of a given type, which
// may be |X509_LU_X509| or |X509_LU_CRL|, and the subject name from the store
// in |vs|. If found and |ret| is not NULL, it increments the reference count
// and stores the object in |ret|.
OPENSSL_EXPORT int X509_STORE_CTX_get_by_subject(X509_STORE_CTX *vs, int type,
                                                 X509_NAME *name,
                                                 X509_OBJECT *ret);

// A BASIC_CONSTRAINTS_st, aka |BASIC_CONSTRAINTS| represents an
// BasicConstraints structure (RFC 5280).
struct BASIC_CONSTRAINTS_st {
  ASN1_BOOLEAN ca;
  ASN1_INTEGER *pathlen;
} /* BASIC_CONSTRAINTS */;

// An ACCESS_DESCRIPTION represents an AccessDescription structure (RFC 5280).
typedef struct ACCESS_DESCRIPTION_st {
  ASN1_OBJECT *method;
  GENERAL_NAME *location;
} ACCESS_DESCRIPTION;

DEFINE_STACK_OF(ACCESS_DESCRIPTION)

typedef STACK_OF(ACCESS_DESCRIPTION) AUTHORITY_INFO_ACCESS;

typedef STACK_OF(ASN1_OBJECT) EXTENDED_KEY_USAGE;

// A DIST_POINT_NAME represents a DistributionPointName structure (RFC 5280).
// The |name| field contains the CHOICE value and is determined by |type|. If
// |type| is zero, |name| must be a |fullname|. If |type| is one, |name| must be
// a |relativename|.
//
// |type| and |name| must be kept consistent. An inconsistency will result in a
// potentially exploitable memory error.
typedef struct DIST_POINT_NAME_st {
  int type;
  union {
    GENERAL_NAMES *fullname;
    STACK_OF(X509_NAME_ENTRY) *relativename;
  } name;
  // If relativename then this contains the full distribution point name
  X509_NAME *dpname;
} DIST_POINT_NAME;

// A DIST_POINT_st, aka |DIST_POINT|, represents a DistributionPoint structure
// (RFC 5280).
struct DIST_POINT_st {
  DIST_POINT_NAME *distpoint;
  ASN1_BIT_STRING *reasons;
  GENERAL_NAMES *CRLissuer;
} /* DIST_POINT */;

typedef STACK_OF(DIST_POINT) CRL_DIST_POINTS;

DEFINE_STACK_OF(DIST_POINT)

// A AUTHORITY_KEYID_st, aka |AUTHORITY_KEYID|, represents an
// AuthorityKeyIdentifier structure (RFC 5280).
struct AUTHORITY_KEYID_st {
  ASN1_OCTET_STRING *keyid;
  GENERAL_NAMES *issuer;
  ASN1_INTEGER *serial;
} /* AUTHORITY_KEYID */;

// A NOTICEREF represents a NoticeReference structure (RFC 5280).
typedef struct NOTICEREF_st {
  ASN1_STRING *organization;
  STACK_OF(ASN1_INTEGER) *noticenos;
} NOTICEREF;

// A USERNOTICE represents a UserNotice structure (RFC 5280).
typedef struct USERNOTICE_st {
  NOTICEREF *noticeref;
  ASN1_STRING *exptext;
} USERNOTICE;

// A POLICYQUALINFO represents a PolicyQualifierInfo structure (RFC 5280). |d|
// contains the qualifier field of the PolicyQualifierInfo. Its type is
// determined by |pqualid|. If |pqualid| is |NID_id_qt_cps|, |d| must be
// |cpsuri|. If |pqualid| is |NID_id_qt_unotice|, |d| must be |usernotice|.
// Otherwise, |d| must be |other|.
//
// |pqualid| and |d| must be kept consistent. An inconsistency will result in a
// potentially exploitable memory error.
typedef struct POLICYQUALINFO_st {
  ASN1_OBJECT *pqualid;
  union {
    ASN1_IA5STRING *cpsuri;
    USERNOTICE *usernotice;
    ASN1_TYPE *other;
  } d;
} POLICYQUALINFO;

DEFINE_STACK_OF(POLICYQUALINFO)

// A POLICYINFO represents a PolicyInformation structure (RFC 5280).
typedef struct POLICYINFO_st {
  ASN1_OBJECT *policyid;
  STACK_OF(POLICYQUALINFO) *qualifiers;
} POLICYINFO;

typedef STACK_OF(POLICYINFO) CERTIFICATEPOLICIES;

DEFINE_STACK_OF(POLICYINFO)

// A POLICY_MAPPING represents an individual element of a PolicyMappings
// structure (RFC 5280).
typedef struct POLICY_MAPPING_st {
  ASN1_OBJECT *issuerDomainPolicy;
  ASN1_OBJECT *subjectDomainPolicy;
} POLICY_MAPPING;

DEFINE_STACK_OF(POLICY_MAPPING)

typedef STACK_OF(POLICY_MAPPING) POLICY_MAPPINGS;

// A GENERAL_SUBTREE represents a GeneralSubtree structure (RFC 5280).
typedef struct GENERAL_SUBTREE_st {
  GENERAL_NAME *base;
  ASN1_INTEGER *minimum;
  ASN1_INTEGER *maximum;
} GENERAL_SUBTREE;

DEFINE_STACK_OF(GENERAL_SUBTREE)

// A NAME_CONSTRAINTS_st, aka |NAME_CONSTRAINTS|, represents a NameConstraints
// structure (RFC 5280).
struct NAME_CONSTRAINTS_st {
  STACK_OF(GENERAL_SUBTREE) *permittedSubtrees;
  STACK_OF(GENERAL_SUBTREE) *excludedSubtrees;
} /* NAME_CONSTRAINTS */;

// A POLICY_CONSTRAINTS represents a PolicyConstraints structure (RFC 5280).
typedef struct POLICY_CONSTRAINTS_st {
  ASN1_INTEGER *requireExplicitPolicy;
  ASN1_INTEGER *inhibitPolicyMapping;
} POLICY_CONSTRAINTS;

// A ISSUING_DIST_POINT_st, aka |ISSUING_DIST_POINT|, represents a
// IssuingDistributionPoint structure (RFC 5280).
struct ISSUING_DIST_POINT_st {
  DIST_POINT_NAME *distpoint;
  ASN1_BOOLEAN onlyuser;
  ASN1_BOOLEAN onlyCA;
  ASN1_BIT_STRING *onlysomereasons;
  ASN1_BOOLEAN indirectCRL;
  ASN1_BOOLEAN onlyattr;
} /* ISSUING_DIST_POINT */;


// BASIC_CONSTRAINTS is an |ASN1_ITEM| whose ASN.1 type is BasicConstraints (RFC
// 5280) and C type is |BASIC_CONSTRAINTS*|.
DECLARE_ASN1_ITEM(BASIC_CONSTRAINTS)

// BASIC_CONSTRAINTS_new returns a newly-allocated, empty |BASIC_CONSTRAINTS|
// object, or NULL on error.
OPENSSL_EXPORT BASIC_CONSTRAINTS *BASIC_CONSTRAINTS_new(void);

// BASIC_CONSTRAINTS_free releases memory associated with |bcons|.
OPENSSL_EXPORT void BASIC_CONSTRAINTS_free(BASIC_CONSTRAINTS *bcons);

// d2i_BASIC_CONSTRAINTS parses up to |len| bytes from |*inp| as a DER-encoded
// BasicConstraints (RFC 5280), as described in |d2i_SAMPLE|.
OPENSSL_EXPORT BASIC_CONSTRAINTS *d2i_BASIC_CONSTRAINTS(BASIC_CONSTRAINTS **out,
                                                        const uint8_t **inp,
                                                        long len);

// i2d_BASIC_CONSTRAINTS marshals |bcons| as a DER-encoded BasicConstraints (RFC
// 5280), as described in |i2d_SAMPLE|.
OPENSSL_EXPORT int i2d_BASIC_CONSTRAINTS(const BASIC_CONSTRAINTS *bcons,
                                         uint8_t **outp);

// AUTHORITY_KEYID is an |ASN1_ITEM| whose ASN.1 type is AuthorityKeyIdentifier
// (RFC 5280) and C type is |AUTHORITY_KEYID*|.
DECLARE_ASN1_ITEM(AUTHORITY_KEYID)

// AUTHORITY_KEYID_new returns a newly-allocated, empty |AUTHORITY_KEYID|
// object, or NULL on error.
OPENSSL_EXPORT AUTHORITY_KEYID *AUTHORITY_KEYID_new(void);

// AUTHORITY_KEYID_free releases memory associated with |akid|.
OPENSSL_EXPORT void AUTHORITY_KEYID_free(AUTHORITY_KEYID *akid);

// d2i_AUTHORITY_KEYID parses up to |len| bytes from |*inp| as a DER-encoded
// AuthorityKeyIdentifier (RFC 5280), as described in |d2i_SAMPLE|.
OPENSSL_EXPORT AUTHORITY_KEYID *d2i_AUTHORITY_KEYID(AUTHORITY_KEYID **out,
                                                    const uint8_t **inp,
                                                    long len);

// i2d_AUTHORITY_KEYID marshals |akid| as a DER-encoded AuthorityKeyIdentifier
// (RFC 5280), as described in |i2d_SAMPLE|.
//
// TODO(https://crbug.com/boringssl/407): |akid| is not const because it
// contains an |X509_NAME|.
OPENSSL_EXPORT int i2d_AUTHORITY_KEYID(AUTHORITY_KEYID *akid, uint8_t **outp);

// EXTENDED_KEY_USAGE is an |ASN1_ITEM| whose ASN.1 type is ExtKeyUsageSyntax
// (RFC 5280) and C type is |STACK_OF(ASN1_OBJECT)*|, or |EXTENDED_KEY_USAGE*|.
DECLARE_ASN1_ITEM(EXTENDED_KEY_USAGE)

// EXTENDED_KEY_USAGE_new returns a newly-allocated, empty |EXTENDED_KEY_USAGE|
// object, or NULL on error.
OPENSSL_EXPORT EXTENDED_KEY_USAGE *EXTENDED_KEY_USAGE_new(void);

// EXTENDED_KEY_USAGE_free releases memory associated with |eku|.
OPENSSL_EXPORT void EXTENDED_KEY_USAGE_free(EXTENDED_KEY_USAGE *eku);

// d2i_EXTENDED_KEY_USAGE parses up to |len| bytes from |*inp| as a DER-encoded
// ExtKeyUsageSyntax (RFC 5280), as described in |d2i_SAMPLE|.
OPENSSL_EXPORT EXTENDED_KEY_USAGE *d2i_EXTENDED_KEY_USAGE(
    EXTENDED_KEY_USAGE **out, const uint8_t **inp, long len);

// i2d_EXTENDED_KEY_USAGE marshals |eku| as a DER-encoded ExtKeyUsageSyntax (RFC
// 5280), as described in |i2d_SAMPLE|.
OPENSSL_EXPORT int i2d_EXTENDED_KEY_USAGE(const EXTENDED_KEY_USAGE *eku,
                                          uint8_t **outp);

// CERTIFICATEPOLICIES is an |ASN1_ITEM| whose ASN.1 type is CertificatePolicies
// (RFC 5280) and C type is |STACK_OF(POLICYINFO)*|, or |CERTIFICATEPOLICIES*|.
DECLARE_ASN1_ITEM(CERTIFICATEPOLICIES)

// CERTIFICATEPOLICIES_new returns a newly-allocated, empty
// |CERTIFICATEPOLICIES| object, or NULL on error.
OPENSSL_EXPORT CERTIFICATEPOLICIES *CERTIFICATEPOLICIES_new(void);

// CERTIFICATEPOLICIES_free releases memory associated with |policies|.
OPENSSL_EXPORT void CERTIFICATEPOLICIES_free(CERTIFICATEPOLICIES *policies);

// d2i_CERTIFICATEPOLICIES parses up to |len| bytes from |*inp| as a DER-encoded
// CertificatePolicies (RFC 5280), as described in |d2i_SAMPLE|.
OPENSSL_EXPORT CERTIFICATEPOLICIES *d2i_CERTIFICATEPOLICIES(
    CERTIFICATEPOLICIES **out, const uint8_t **inp, long len);

// i2d_CERTIFICATEPOLICIES marshals |policies| as a DER-encoded
// CertificatePolicies (RFC 5280), as described in |i2d_SAMPLE|.
OPENSSL_EXPORT int i2d_CERTIFICATEPOLICIES(const CERTIFICATEPOLICIES *policies,
                                           uint8_t **outp);

// POLICYINFO_new returns a newly-allocated, empty |POLICYINFO| object, or NULL
// on error.
OPENSSL_EXPORT POLICYINFO *POLICYINFO_new(void);

// POLICYINFO_free releases memory associated with |info|.
OPENSSL_EXPORT void POLICYINFO_free(POLICYINFO *info);

// POLICYQUALINFO_new returns a newly-allocated, empty |POLICYQUALINFO| object,
// or NULL on error.
OPENSSL_EXPORT POLICYQUALINFO *POLICYQUALINFO_new(void);

// POLICYQUALINFO_free releases memory associated with |info|.
OPENSSL_EXPORT void POLICYQUALINFO_free(POLICYQUALINFO *info);

// USERNOTICE_new returns a newly-allocated, empty |USERNOTICE| object, or NULL
// on error.
OPENSSL_EXPORT USERNOTICE *USERNOTICE_new(void);

// USERNOTICE_free releases memory associated with |notice|.
OPENSSL_EXPORT void USERNOTICE_free(USERNOTICE *notice);

// NOTICEREF_new returns a newly-allocated, empty |NOTICEREF| object, or NULL
// on error.
OPENSSL_EXPORT NOTICEREF *NOTICEREF_new(void);

// NOTICEREF_free releases memory associated with |ref|.
OPENSSL_EXPORT void NOTICEREF_free(NOTICEREF *ref);

// CRL_DIST_POINTS is an |ASN1_ITEM| whose ASN.1 type is CRLDistributionPoints
// (RFC 5280) and C type is |CRL_DIST_POINTS*|.
DECLARE_ASN1_ITEM(CRL_DIST_POINTS)

// CRL_DIST_POINTS_new returns a newly-allocated, empty |CRL_DIST_POINTS|
// object, or NULL on error.
OPENSSL_EXPORT CRL_DIST_POINTS *CRL_DIST_POINTS_new(void);

// CRL_DIST_POINTS_free releases memory associated with |crldp|.
OPENSSL_EXPORT void CRL_DIST_POINTS_free(CRL_DIST_POINTS *crldp);

// d2i_CRL_DIST_POINTS parses up to |len| bytes from |*inp| as a DER-encoded
// CRLDistributionPoints (RFC 5280), as described in |d2i_SAMPLE|.
OPENSSL_EXPORT CRL_DIST_POINTS *d2i_CRL_DIST_POINTS(CRL_DIST_POINTS **out,
                                                    const uint8_t **inp,
                                                    long len);

// i2d_CRL_DIST_POINTS marshals |crldp| as a DER-encoded CRLDistributionPoints
// (RFC 5280), as described in |i2d_SAMPLE|.
//
// TODO(https://crbug.com/boringssl/407): |crldp| is not const because it
// contains an |X509_NAME|.
OPENSSL_EXPORT int i2d_CRL_DIST_POINTS(CRL_DIST_POINTS *crldp, uint8_t **outp);

// DIST_POINT_new returns a newly-allocated, empty |DIST_POINT| object, or NULL
// on error.
OPENSSL_EXPORT DIST_POINT *DIST_POINT_new(void);

// DIST_POINT_free releases memory associated with |dp|.
OPENSSL_EXPORT void DIST_POINT_free(DIST_POINT *dp);

// DIST_POINT_NAME_new returns a newly-allocated, empty |DIST_POINT_NAME|
// object, or NULL on error.
OPENSSL_EXPORT DIST_POINT_NAME *DIST_POINT_NAME_new(void);

// DIST_POINT_NAME_free releases memory associated with |name|.
OPENSSL_EXPORT void DIST_POINT_NAME_free(DIST_POINT_NAME *name);

// ISSUING_DIST_POINT is an |ASN1_ITEM| whose ASN.1 type is
// IssuingDistributionPoint (RFC 5280) and C type is |ISSUING_DIST_POINT*|.
DECLARE_ASN1_ITEM(ISSUING_DIST_POINT)

// ISSUING_DIST_POINT_new returns a newly-allocated, empty |ISSUING_DIST_POINT|
// object, or NULL on error.
OPENSSL_EXPORT ISSUING_DIST_POINT *ISSUING_DIST_POINT_new(void);

// ISSUING_DIST_POINT_free releases memory associated with |idp|.
OPENSSL_EXPORT void ISSUING_DIST_POINT_free(ISSUING_DIST_POINT *idp);

// d2i_ISSUING_DIST_POINT parses up to |len| bytes from |*inp| as a DER-encoded
// IssuingDistributionPoint (RFC 5280), as described in |d2i_SAMPLE|.
OPENSSL_EXPORT ISSUING_DIST_POINT *d2i_ISSUING_DIST_POINT(
    ISSUING_DIST_POINT **out, const uint8_t **inp, long len);

// i2d_ISSUING_DIST_POINT marshals |idp| as a DER-encoded
// IssuingDistributionPoint (RFC 5280), as described in |i2d_SAMPLE|.
//
// TODO(https://crbug.com/boringssl/407): |idp| is not const because it
// contains an |X509_NAME|.
OPENSSL_EXPORT int i2d_ISSUING_DIST_POINT(ISSUING_DIST_POINT *idp,
                                          uint8_t **outp);

// ACCESS_DESCRIPTION_new returns a newly-allocated, empty |ACCESS_DESCRIPTION|
// object, or NULL on error.
OPENSSL_EXPORT ACCESS_DESCRIPTION *ACCESS_DESCRIPTION_new(void);

// ACCESS_DESCRIPTION_free releases memory associated with |desc|.
OPENSSL_EXPORT void ACCESS_DESCRIPTION_free(ACCESS_DESCRIPTION *desc);

// AUTHORITY_INFO_ACCESS is an |ASN1_ITEM| whose ASN.1 type is
// AuthorityInfoAccessSyntax (RFC 5280) and C type is
// |STACK_OF(ACCESS_DESCRIPTION)*|, or |AUTHORITY_INFO_ACCESS*|.
DECLARE_ASN1_ITEM(AUTHORITY_INFO_ACCESS)

// AUTHORITY_INFO_ACCESS_new returns a newly-allocated, empty
// |AUTHORITY_INFO_ACCESS| object, or NULL on error.
OPENSSL_EXPORT AUTHORITY_INFO_ACCESS *AUTHORITY_INFO_ACCESS_new(void);

// AUTHORITY_INFO_ACCESS_free releases memory associated with |aia|.
OPENSSL_EXPORT void AUTHORITY_INFO_ACCESS_free(AUTHORITY_INFO_ACCESS *aia);

// d2i_AUTHORITY_INFO_ACCESS parses up to |len| bytes from |*inp| as a
// DER-encoded AuthorityInfoAccessSyntax (RFC 5280), as described in
// |d2i_SAMPLE|.
OPENSSL_EXPORT AUTHORITY_INFO_ACCESS *d2i_AUTHORITY_INFO_ACCESS(
    AUTHORITY_INFO_ACCESS **out, const uint8_t **inp, long len);

// i2d_AUTHORITY_INFO_ACCESS marshals |aia| as a DER-encoded
// AuthorityInfoAccessSyntax (RFC 5280), as described in |i2d_SAMPLE|.
//
// TODO(https://crbug.com/boringssl/407): |aia| is not const because it
// contains an |X509_NAME|.
OPENSSL_EXPORT int i2d_AUTHORITY_INFO_ACCESS(AUTHORITY_INFO_ACCESS *aia,
                                             uint8_t **outp);

// POLICY_MAPPING_new returns a newly-allocated, empty |POLICY_MAPPING| object,
// or NULL on error.
OPENSSL_EXPORT POLICY_MAPPING *POLICY_MAPPING_new(void);

// POLICY_MAPPING_free releases memory associated with |mapping|.
OPENSSL_EXPORT void POLICY_MAPPING_free(POLICY_MAPPING *mapping);

// POLICY_MAPPINGS is an |ASN1_ITEM| whose ASN.1 type is PolicyMappings (RFC
// 5280) and C type is |STACK_OF(POLICY_MAPPING)*|, or |POLICY_MAPPINGS*|.
DECLARE_ASN1_ITEM(POLICY_MAPPINGS)

// GENERAL_SUBTREE_new returns a newly-allocated, empty |GENERAL_SUBTREE|
// object, or NULL on error.
OPENSSL_EXPORT GENERAL_SUBTREE *GENERAL_SUBTREE_new(void);

// GENERAL_SUBTREE_free releases memory associated with |subtree|.
OPENSSL_EXPORT void GENERAL_SUBTREE_free(GENERAL_SUBTREE *subtree);

// NAME_CONSTRAINTS is an |ASN1_ITEM| whose ASN.1 type is NameConstraints (RFC
// 5280) and C type is |NAME_CONSTRAINTS*|.
DECLARE_ASN1_ITEM(NAME_CONSTRAINTS)

// NAME_CONSTRAINTS_new returns a newly-allocated, empty |NAME_CONSTRAINTS|
// object, or NULL on error.
OPENSSL_EXPORT NAME_CONSTRAINTS *NAME_CONSTRAINTS_new(void);

// NAME_CONSTRAINTS_free releases memory associated with |ncons|.
OPENSSL_EXPORT void NAME_CONSTRAINTS_free(NAME_CONSTRAINTS *ncons);

// POLICY_CONSTRAINTS_new returns a newly-allocated, empty |POLICY_CONSTRAINTS|
// object, or NULL on error.
OPENSSL_EXPORT POLICY_CONSTRAINTS *POLICY_CONSTRAINTS_new(void);

// POLICY_CONSTRAINTS_free releases memory associated with |pcons|.
OPENSSL_EXPORT void POLICY_CONSTRAINTS_free(POLICY_CONSTRAINTS *pcons);

// POLICY_CONSTRAINTS is an |ASN1_ITEM| whose ASN.1 type is PolicyConstraints
// (RFC 5280) and C type is |POLICY_CONSTRAINTS*|.
DECLARE_ASN1_ITEM(POLICY_CONSTRAINTS)


OPENSSL_EXPORT int X509_PURPOSE_set(int *p, int purpose);

OPENSSL_EXPORT int X509_PURPOSE_get_count(void);
OPENSSL_EXPORT int X509_PURPOSE_get_by_id(int id);
OPENSSL_EXPORT char *X509_PURPOSE_get0_name(const X509_PURPOSE *xp);
OPENSSL_EXPORT char *X509_PURPOSE_get0_sname(const X509_PURPOSE *xp);
OPENSSL_EXPORT int X509_PURPOSE_get_trust(const X509_PURPOSE *xp);


#if defined(__cplusplus)
}  // extern C
#endif

#if !defined(BORINGSSL_NO_CXX)
extern "C++" {

BSSL_NAMESPACE_BEGIN

BORINGSSL_MAKE_DELETER(ACCESS_DESCRIPTION, ACCESS_DESCRIPTION_free)
BORINGSSL_MAKE_DELETER(AUTHORITY_KEYID, AUTHORITY_KEYID_free)
BORINGSSL_MAKE_DELETER(BASIC_CONSTRAINTS, BASIC_CONSTRAINTS_free)
// TODO(davidben): Move this to conf.h and rename to CONF_VALUE_free.
BORINGSSL_MAKE_DELETER(CONF_VALUE, X509V3_conf_free)
BORINGSSL_MAKE_DELETER(DIST_POINT, DIST_POINT_free)
BORINGSSL_MAKE_DELETER(GENERAL_NAME, GENERAL_NAME_free)
BORINGSSL_MAKE_DELETER(GENERAL_SUBTREE, GENERAL_SUBTREE_free)
BORINGSSL_MAKE_DELETER(NAME_CONSTRAINTS, NAME_CONSTRAINTS_free)
BORINGSSL_MAKE_DELETER(NETSCAPE_SPKI, NETSCAPE_SPKI_free)
BORINGSSL_MAKE_DELETER(POLICY_MAPPING, POLICY_MAPPING_free)
BORINGSSL_MAKE_DELETER(POLICYINFO, POLICYINFO_free)
BORINGSSL_MAKE_DELETER(RSA_PSS_PARAMS, RSA_PSS_PARAMS_free)
BORINGSSL_MAKE_DELETER(X509, X509_free)
BORINGSSL_MAKE_UP_REF(X509, X509_up_ref)
BORINGSSL_MAKE_DELETER(X509_ALGOR, X509_ALGOR_free)
BORINGSSL_MAKE_DELETER(X509_ATTRIBUTE, X509_ATTRIBUTE_free)
BORINGSSL_MAKE_DELETER(X509_CRL, X509_CRL_free)
BORINGSSL_MAKE_UP_REF(X509_CRL, X509_CRL_up_ref)
BORINGSSL_MAKE_DELETER(X509_EXTENSION, X509_EXTENSION_free)
BORINGSSL_MAKE_DELETER(X509_INFO, X509_INFO_free)
BORINGSSL_MAKE_DELETER(X509_LOOKUP, X509_LOOKUP_free)
BORINGSSL_MAKE_DELETER(X509_NAME, X509_NAME_free)
BORINGSSL_MAKE_DELETER(X509_NAME_ENTRY, X509_NAME_ENTRY_free)
BORINGSSL_MAKE_DELETER(X509_OBJECT, X509_OBJECT_free)
BORINGSSL_MAKE_DELETER(X509_PUBKEY, X509_PUBKEY_free)
BORINGSSL_MAKE_DELETER(X509_REQ, X509_REQ_free)
BORINGSSL_MAKE_DELETER(X509_REVOKED, X509_REVOKED_free)
BORINGSSL_MAKE_DELETER(X509_SIG, X509_SIG_free)
BORINGSSL_MAKE_DELETER(X509_STORE, X509_STORE_free)
BORINGSSL_MAKE_UP_REF(X509_STORE, X509_STORE_up_ref)
BORINGSSL_MAKE_DELETER(X509_STORE_CTX, X509_STORE_CTX_free)
BORINGSSL_MAKE_DELETER(X509_VERIFY_PARAM, X509_VERIFY_PARAM_free)

BSSL_NAMESPACE_END

}  // extern C++
#endif  // !BORINGSSL_NO_CXX

#define X509_R_AKID_MISMATCH 100
#define X509_R_BAD_PKCS7_VERSION 101
#define X509_R_BAD_X509_FILETYPE 102
#define X509_R_BASE64_DECODE_ERROR 103
#define X509_R_CANT_CHECK_DH_KEY 104
#define X509_R_CERT_ALREADY_IN_HASH_TABLE 105
#define X509_R_CRL_ALREADY_DELTA 106
#define X509_R_CRL_VERIFY_FAILURE 107
#define X509_R_IDP_MISMATCH 108
#define X509_R_INVALID_BIT_STRING_BITS_LEFT 109
#define X509_R_INVALID_DIRECTORY 110
#define X509_R_INVALID_FIELD_NAME 111
#define X509_R_INVALID_PSS_PARAMETERS 112
#define X509_R_INVALID_TRUST 113
#define X509_R_ISSUER_MISMATCH 114
#define X509_R_KEY_TYPE_MISMATCH 115
#define X509_R_KEY_VALUES_MISMATCH 116
#define X509_R_LOADING_CERT_DIR 117
#define X509_R_LOADING_DEFAULTS 118
#define X509_R_NEWER_CRL_NOT_NEWER 119
#define X509_R_NOT_PKCS7_SIGNED_DATA 120
#define X509_R_NO_CERTIFICATES_INCLUDED 121
#define X509_R_NO_CERT_SET_FOR_US_TO_VERIFY 122
#define X509_R_NO_CRLS_INCLUDED 123
#define X509_R_NO_CRL_NUMBER 124
#define X509_R_PUBLIC_KEY_DECODE_ERROR 125
#define X509_R_PUBLIC_KEY_ENCODE_ERROR 126
#define X509_R_SHOULD_RETRY 127
#define X509_R_UNKNOWN_KEY_TYPE 128
#define X509_R_UNKNOWN_NID 129
#define X509_R_UNKNOWN_PURPOSE_ID 130
#define X509_R_UNKNOWN_TRUST_ID 131
#define X509_R_UNSUPPORTED_ALGORITHM 132
#define X509_R_WRONG_LOOKUP_TYPE 133
#define X509_R_WRONG_TYPE 134
#define X509_R_NAME_TOO_LONG 135
#define X509_R_INVALID_PARAMETER 136
#define X509_R_SIGNATURE_ALGORITHM_MISMATCH 137
#define X509_R_DELTA_CRL_WITHOUT_CRL_NUMBER 138
#define X509_R_INVALID_FIELD_FOR_VERSION 139
#define X509_R_INVALID_VERSION 140
#define X509_R_NO_CERTIFICATE_FOUND 141
#define X509_R_NO_CERTIFICATE_OR_CRL_FOUND 142
#define X509_R_NO_CRL_FOUND 143
#define X509_R_INVALID_POLICY_EXTENSION 144
#define X509_R_UNKNOWN_SIGID_ALGS 145

#endif  // OPENSSL_HEADER_X509_H
