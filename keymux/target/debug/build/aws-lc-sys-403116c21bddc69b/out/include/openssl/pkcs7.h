/* Copyright (c) 2014, Google Inc.
 *
 * Permission to use, copy, modify, and/or distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY
 * SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN ACTION
 * OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF OR IN
 * CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE. */

#ifndef OPENSSL_HEADER_PKCS7_H
#define OPENSSL_HEADER_PKCS7_H

#include <openssl/asn1.h>
#include <openssl/base.h>

#include <openssl/stack.h>

#if defined(__cplusplus)
extern "C" {
#endif


// PKCS#7.
//
// This library contains functions for extracting information from PKCS#7
// structures (RFC 2315).

DECLARE_STACK_OF(CRYPTO_BUFFER)
DECLARE_STACK_OF(X509)
DECLARE_STACK_OF(X509_CRL)

// PKCS7_get_raw_certificates parses a PKCS#7, SignedData structure from |cbs|
// and appends the included certificates to |out_certs|. It returns one on
// success and zero on error. |cbs| is advanced passed the structure.
//
// Note that a SignedData structure may contain no certificates, in which case
// this function succeeds but does not append any certificates. Additionally,
// certificates in SignedData structures are unordered. Callers should not
// assume a particular order in |*out_certs| and may need to search for matches
// or run path-building algorithms.
OPENSSL_EXPORT int PKCS7_get_raw_certificates(
    STACK_OF(CRYPTO_BUFFER) *out_certs, CBS *cbs, CRYPTO_BUFFER_POOL *pool);

// PKCS7_get_certificates behaves like |PKCS7_get_raw_certificates| but parses
// them into |X509| objects.
OPENSSL_EXPORT int PKCS7_get_certificates(STACK_OF(X509) *out_certs, CBS *cbs);

// PKCS7_bundle_raw_certificates appends a PKCS#7, SignedData structure
// containing |certs| to |out|. It returns one on success and zero on error.
// Note that certificates in SignedData structures are unordered. The order in
// |certs| will not be preserved.
OPENSSL_EXPORT int PKCS7_bundle_raw_certificates(
    CBB *out, const STACK_OF(CRYPTO_BUFFER) *certs);

// PKCS7_bundle_certificates behaves like |PKCS7_bundle_raw_certificates| but
// takes |X509| objects as input.
OPENSSL_EXPORT int PKCS7_bundle_certificates(CBB *out,
                                             const STACK_OF(X509) *certs);

// PKCS7_get_CRLs parses a PKCS#7, SignedData structure from |cbs| and appends
// the included CRLs to |out_crls|. It returns one on success and zero on error.
// |cbs| is advanced passed the structure.
//
// Note that a SignedData structure may contain no CRLs, in which case this
// function succeeds but does not append any CRLs. Additionally, CRLs in
// SignedData structures are unordered. Callers should not assume an order in
// |*out_crls| and may need to search for matches.
OPENSSL_EXPORT int PKCS7_get_CRLs(STACK_OF(X509_CRL) *out_crls, CBS *cbs);

// PKCS7_bundle_CRLs appends a PKCS#7, SignedData structure containing
// |crls| to |out|. It returns one on success and zero on error. Note that CRLs
// in SignedData structures are unordered. The order in |crls| will not be
// preserved.
OPENSSL_EXPORT int PKCS7_bundle_CRLs(CBB *out, const STACK_OF(X509_CRL) *crls);

// PKCS7_get_PEM_certificates reads a PEM-encoded, PKCS#7, SignedData structure
// from |pem_bio| and appends the included certificates to |out_certs|. It
// returns one on success and zero on error.
//
// Note that a SignedData structure may contain no certificates, in which case
// this function succeeds but does not append any certificates. Additionally,
// certificates in SignedData structures are unordered. Callers should not
// assume a particular order in |*out_certs| and may need to search for matches
// or run path-building algorithms.
OPENSSL_EXPORT int PKCS7_get_PEM_certificates(STACK_OF(X509) *out_certs,
                                              BIO *pem_bio);

// PKCS7_get_PEM_CRLs reads a PEM-encoded, PKCS#7, SignedData structure from
// |pem_bio| and appends the included CRLs to |out_crls|. It returns one on
// success and zero on error.
//
// Note that a SignedData structure may contain no CRLs, in which case this
// function succeeds but does not append any CRLs. Additionally, CRLs in
// SignedData structures are unordered. Callers should not assume an order in
// |*out_crls| and may need to search for matches.
OPENSSL_EXPORT int PKCS7_get_PEM_CRLs(STACK_OF(X509_CRL) *out_crls,
                                      BIO *pem_bio);

// d2i_PKCS7_bio behaves like |d2i_PKCS7| but reads the input from |bio|.  If
// the length of the object is indefinite the full contents of |bio| are read.
//
// If the function fails then some unknown amount of data may have been read
// from |bio|.
OPENSSL_EXPORT PKCS7 *d2i_PKCS7_bio(BIO *bio, PKCS7 **out);

// i2d_PKCS7_bio writes |p7| to |bio|. It returns one on success and zero on
// error.
OPENSSL_EXPORT int i2d_PKCS7_bio(BIO *bio, const PKCS7 *p7);

// PKCS7_type_is_data returns 1 if |p7| is of type data
OPENSSL_EXPORT int PKCS7_type_is_data(const PKCS7 *p7);

// PKCS7_type_is_digest returns 1 if |p7| is of type digest
OPENSSL_EXPORT int PKCS7_type_is_digest(const PKCS7 *p7);

// PKCS7_type_is_encrypted returns 1 if |p7| is of type encrypted
OPENSSL_EXPORT int PKCS7_type_is_encrypted(const PKCS7 *p7);

// PKCS7_type_is_enveloped returns 1 if |p7| is of type enveloped
OPENSSL_EXPORT int PKCS7_type_is_enveloped(const PKCS7 *p7);

// PKCS7_type_is_signed returns 1 if |p7| is of type signed
OPENSSL_EXPORT int PKCS7_type_is_signed(const PKCS7 *p7);

// PKCS7_type_is_signedAndEnveloped returns 1 if |p7| is of type
// signedAndEnveloped
OPENSSL_EXPORT int PKCS7_type_is_signedAndEnveloped(const PKCS7 *p7);


// Deprecated functions.
//
// These functions are a compatibility layer over a subset of OpenSSL's PKCS#7
// API. It intentionally does not implement the whole thing, only the minimum
// needed to build cryptography.io and CRuby.

// ASN.1 defined here https://datatracker.ietf.org/doc/html/rfc2315#section-7
//
//   ContentInfo ::= SEQUENCE {
//     contentType ContentType,
//     content
//       [0] EXPLICIT ANY DEFINED BY contentType OPTIONAL }
//
//   ContentType ::= OBJECT IDENTIFIER
struct pkcs7_st {
  // Unlike OpenSSL, the following fields are immutable. They filled in when the
  // object is parsed and ignored in serialization.
  ASN1_OBJECT *type;
  union {
    char *ptr;
    ASN1_OCTET_STRING *data;
    PKCS7_SIGNED *sign;
    PKCS7_ENVELOPE *enveloped;
    PKCS7_SIGN_ENVELOPE *signed_and_enveloped;
    PKCS7_DIGEST *digest;
    PKCS7_ENCRYPT *encrypted;
    // Other things provided by the user. Not specified in the RFC.
    ASN1_TYPE *other;
  } d;
};

// ASN.1 defined here https://datatracker.ietf.org/doc/html/rfc2315#section-9.1
//
//   SignedData ::= SEQUENCE {
//     version Version,
//     digestAlgorithms DigestAlgorithmIdentifiers,
//     contentInfo ContentInfo,
//     certificates
//        [0] IMPLICIT ExtendedCertificatesAndCertificates
//          OPTIONAL,
//     crls
//       [1] IMPLICIT CertificateRevocationLists OPTIONAL,
//     signerInfos SignerInfos }
//
//   DigestAlgorithmIdentifiers ::=
//
//     SET OF DigestAlgorithmIdentifier
//
//   SignerInfos ::= SET OF SignerInfo
struct pkcs7_signed_st {
  ASN1_INTEGER *version;
  STACK_OF(X509_ALGOR) *md_algs;
  PKCS7 *contents;
  STACK_OF(X509) *cert;
  STACK_OF(X509_CRL) *crl;
  STACK_OF(PKCS7_SIGNER_INFO) *signer_info;
};

// ASN.1 defined here https://datatracker.ietf.org/doc/html/rfc2315#section-9.2
//
//   SignerInfo ::= SEQUENCE {
//     version Version,
//     issuerAndSerialNumber IssuerAndSerialNumber,
//     digestAlgorithm DigestAlgorithmIdentifier,
//     authenticatedAttributes
//       [0] IMPLICIT Attributes OPTIONAL,
//     digestEncryptionAlgorithm
//       DigestEncryptionAlgorithmIdentifier,
//     encryptedDigest EncryptedDigest,
//     unauthenticatedAttributes
//       [1] IMPLICIT Attributes OPTIONAL }
//
//   EncryptedDigest ::= OCTET STRING
struct pkcs7_signer_info_st {
  ASN1_INTEGER *version;
  PKCS7_ISSUER_AND_SERIAL *issuer_and_serial;
  X509_ALGOR *digest_alg;
  STACK_OF(X509_ATTRIBUTE) *auth_attr;
  X509_ALGOR *digest_enc_alg;
  ASN1_OCTET_STRING *enc_digest;
  STACK_OF(X509_ATTRIBUTE) *unauth_attr;
  EVP_PKEY *pkey;  // NOTE: |pkey| is not serialized.
};

// ASN.1 defined here https://datatracker.ietf.org/doc/html/rfc2315#section-11.1
//
//   SignedAndEnvelopedData ::= SEQUENCE {
//     version Version,
//     recipientInfos RecipientInfos,
//     digestAlgorithms DigestAlgorithmIdentifiers,
//     encryptedContentInfo EncryptedContentInfo,
//     certificates
//        [0] IMPLICIT ExtendedCertificatesAndCertificates
//          OPTIONAL,
//     crls
//       [1] IMPLICIT CertificateRevocationLists OPTIONAL,
//     signerInfos SignerInfos }
struct pkcs7_sign_envelope_st {
  ASN1_INTEGER *version;
  STACK_OF(PKCS7_RECIP_INFO) *recipientinfo;
  STACK_OF(X509_ALGOR) *md_algs;
  PKCS7_ENC_CONTENT *enc_data;
  STACK_OF(X509) *cert;
  STACK_OF(X509_CRL) *crl;
  STACK_OF(PKCS7_SIGNER_INFO) *signer_info;
};

// ASN.1 defined here https://datatracker.ietf.org/doc/html/rfc2315#section-10.1
//
//    EnvelopedData ::= SEQUENCE {
//      version Version,
//      recipientInfos RecipientInfos,
//      encryptedContentInfo EncryptedContentInfo }
//
//    RecipientInfos ::= SET OF RecipientInfo
struct pkcs7_envelope_st {
  ASN1_INTEGER *version;
  PKCS7_ENC_CONTENT *enc_data;
  STACK_OF(PKCS7_RECIP_INFO) *recipientinfo;
};

// ASN.1 defined here https://datatracker.ietf.org/doc/html/rfc2315#section-10.2
//
//   RecipientInfo ::= SEQUENCE {
//     version Version,
//     issuerAndSerialNumber IssuerAndSerialNumber,
//     keyEncryptionAlgorithm
//
//       KeyEncryptionAlgorithmIdentifier,
//     encryptedKey EncryptedKey }
//
//   EncryptedKey ::= OCTET STRING
struct pkcs7_recip_info_st {
  ASN1_INTEGER *version;
  PKCS7_ISSUER_AND_SERIAL *issuer_and_serial;
  X509_ALGOR *key_enc_algor;
  ASN1_OCTET_STRING *enc_key;
  X509 *cert;  // NOTE: |cert| is not serialized
};

// ASN.1 defined here https://datatracker.ietf.org/doc/html/rfc2315#section-6.7
//
//   IssuerAndSerialNumber ::= SEQUENCE {
//     issuer Name,
//     serialNumber CertificateSerialNumber }
struct pkcs7_issuer_and_serial_st {
  X509_NAME *issuer;
  ASN1_INTEGER *serial;
};

// Only declare ASN1 functions or define stacks publibly if needed by supported
// projects that depend on them.
DECLARE_ASN1_FUNCTIONS(PKCS7)
DECLARE_ASN1_FUNCTIONS(PKCS7_RECIP_INFO)
DECLARE_ASN1_FUNCTIONS(PKCS7_SIGNER_INFO)

DEFINE_STACK_OF(PKCS7_RECIP_INFO)
DEFINE_STACK_OF(PKCS7_SIGNER_INFO)

// PKCS7_dup returns a newly allocated copy of |p7| without deep-copying
// internal references.
OPENSSL_EXPORT OPENSSL_DEPRECATED PKCS7 *PKCS7_dup(PKCS7 *p7);

// PKCS7_get_signed_attribute returns a pointer to the first signed attribute
// from |si| with NID |nid| if one is present, else NULL.
OPENSSL_EXPORT OPENSSL_DEPRECATED ASN1_TYPE *PKCS7_get_signed_attribute(
    const PKCS7_SIGNER_INFO *si, int nid);

// PKCS7_get_signer_info returns |p7|'s attached PKCS7_SIGNER_INFO if present
// and |p7| is of a relevant type, else NULL. This function only pertains to
// signedData and signedAndEnvelopedData.
OPENSSL_EXPORT OPENSSL_DEPRECATED STACK_OF(PKCS7_SIGNER_INFO) *
PKCS7_get_signer_info(PKCS7 *p7);

// PKCS7_RECIP_INFO_set attaches |x509| to |p7i| and increments |x509|'s
// reference count. It returns 1 on success and 0 on failure or if |x509|'s
// public key not usable for encryption.
OPENSSL_EXPORT OPENSSL_DEPRECATED int PKCS7_RECIP_INFO_set(
    PKCS7_RECIP_INFO *p7i, X509 *x509);

// PKCS7_SIGNER_INFO_set attaches the other parameters to |p7i|, returning 1 on
// success and 0 on error or if specified parameters are inapplicable to
// signing. Only EC, DH, and RSA |pkey|s are supported. |pkey|'s reference
// count is incremented, but neither |x509|'s nor |dgst|'s is.
OPENSSL_EXPORT OPENSSL_DEPRECATED int PKCS7_SIGNER_INFO_set(
    PKCS7_SIGNER_INFO *p7i, X509 *x509, EVP_PKEY *pkey, const EVP_MD *dgst);

// PKCS7_add_certificate adds |x509| to |p7|'s certificate stack, incrementing
// |x509|'s reference count.  It returns 1 on success and 0 on failure or if
// |p7| isn't of an applicable type: signedData and signedAndEnvelopedData.
OPENSSL_EXPORT OPENSSL_DEPRECATED int PKCS7_add_certificate(PKCS7 *p7,
                                                            X509 *x509);

// PKCS7_add_crl adds |x509| to |p7|'s CRL stack, incrementing |x509|'s
// reference count. It returns 1 on success and 0 on failure or if |p7| isn't
// of an applicable type: signedData and signedAndEnvelopedData.
OPENSSL_EXPORT OPENSSL_DEPRECATED int PKCS7_add_crl(PKCS7 *p7, X509_CRL *x509);

// PKCS7_add_recipient_info adds |ri| to |p7|, returning 1 on succes or 0 if
// |p7| is of an inapplicable type: envelopedData and signedAndEnvelopedData.
OPENSSL_EXPORT OPENSSL_DEPRECATED int PKCS7_add_recipient_info(
    PKCS7 *p7, PKCS7_RECIP_INFO *ri);

// PKCS7_add_signer adds |p7i| to |p7|, returning 1 on succes or 0 if
// |p7| is of an inapplicable type: signedData and signedAndEnvelopedData.
OPENSSL_EXPORT OPENSSL_DEPRECATED int PKCS7_add_signer(PKCS7 *p7,
                                                       PKCS7_SIGNER_INFO *p7i);

// PKCS7_content_new allocates a new PKCS7 and adds it to |p7| as content. It
// returns 1 on success and 0 on failure.
OPENSSL_EXPORT OPENSSL_DEPRECATED int PKCS7_content_new(PKCS7 *p7, int nid);

// PKCS7_set_content sets |p7_data| as content on |p7| for applicable types of
// |p7|. It frees any existing content on |p7|, returning 1 on success and 0 on
// failure.
OPENSSL_EXPORT OPENSSL_DEPRECATED int PKCS7_set_content(PKCS7 *p7,
                                                        PKCS7 *p7_data);

// PKCS7_set_content sets |p7_data| as content on |p7| for applicable types of
// |p7|: signedData and digestData. |p7_data| may be NULL. It frees any
// existing content on |p7|, returning 1 on success and 0 on failure.
OPENSSL_EXPORT OPENSSL_DEPRECATED int PKCS7_set_cipher(
    PKCS7 *p7, const EVP_CIPHER *cipher);

// PKCS7_set_type instantiates |p7| as type |type|. It returns 1 on success and
// 0 on failure or if |type| is not a valid PKCS7 content type.
OPENSSL_EXPORT OPENSSL_DEPRECATED int PKCS7_set_type(PKCS7 *p7, int type);

// PKCS7_RECIP_INFO_get0_alg sets |*penc| to |ri|'s key encryption algorithm,
// if present. Ownership of |*penc| is retained by |ri|.
OPENSSL_EXPORT OPENSSL_DEPRECATED void PKCS7_RECIP_INFO_get0_alg(
    PKCS7_RECIP_INFO *ri, X509_ALGOR **penc);

// PKCS7_SIGNER_INFO_get0_algs sets all of, if present: |*pk| to |si|'s key,
// |*pdig| to |si|'s digest angorithm, and |*psig| to |si|'s signature
// algorithm. Ownership of |*pk|, |*pdig|, and |*psig) is retained by |si|.
OPENSSL_EXPORT OPENSSL_DEPRECATED void PKCS7_SIGNER_INFO_get0_algs(
    PKCS7_SIGNER_INFO *si, EVP_PKEY **pk, X509_ALGOR **pdig, X509_ALGOR **psig);


// Deprecated flags
//
// Not all defined flags are acted upon, and the behavior associated with some
// flags is performed unconditionally. See each |PKCS7_*| for details.

// PKCS7_DETACHED indicates that the PKCS#7 file specifies its data externally.
#define PKCS7_DETACHED 0x40

// PKCS7_BINARY disables the default translation to MIME canonical format (as
// required by the S/MIME specifications). It is assumed in |PKCS7_sign| unless
// the caller is just bundling certs.
#define PKCS7_BINARY 0x80

// PKCS7_NOINTERN disables verification against certificate public keys included
// in a PKCS7 ContentInfo. If this flag is specified, the caller must supply a
// stack of certificates to verify against.
#define PKCS7_NOINTERN 0x10

// PKCS7_NOATTR disables usage of authenticatedAttributes. It is assumed in
// |PKCS7_sign| unless the caller is just bundling certs.
#define PKCS7_NOATTR 0x100

// PKCS7_NOCERTS excludes the signer's certificate and the extra certs defined
// from the PKCS7 structure. Using this will fail |PKCS7_sign| unless used as
// described in |PKCS7_sign|'s documentation.
#define PKCS7_NOCERTS 0x2

// PKCS7_NOVERIFY will skip trust chain verification against the trust store.
// It will still verify signatures against signer infos included in the PKCS7.
#define PKCS7_NOVERIFY 0x20

// The following flags are used in OpenSSL, but are ignored by AWS-LC. They are
// defined here solely for build compatibility.
#define PKCS7_TEXT 0x1
#define PKCS7_NOSIGS 0x4
#define PKCS7_NOCHAIN 0x8
#define PKCS7_NOSMIMECAP 0x200
#define PKCS7_STREAM 0x1000
#define PKCS7_PARTIAL 0x4000

// PKCS7_sign can operate in three modes to provide some backwards
// compatibility:
//
// The first mode assembles |certs| into a PKCS#7 signed data ContentInfo with
// external data and no signatures. It returns a newly-allocated |PKCS7| on
// success or NULL on error. |sign_cert| and |pkey| must be NULL. |data| is
// ignored. |flags| must be equal to |PKCS7_DETACHED|. Additionally,
// certificates in SignedData structures are unordered. The order of |certs|
// will not be preserved.
//
// The second mode generates a detached RSA SHA-256 signature of |data| using
// |pkey| and produces a PKCS#7 SignedData structure containing it. |certs|
// must be NULL and |flags| must be exactly |PKCS7_NOATTR | PKCS7_BINARY |
// PKCS7_NOCERTS | PKCS7_DETACHED|.
//
// The third mode is used for more general signing and does not require the
// specification of any flags, but does require |sign_cert|, |pkey|, and |data|
// to be populated. This mode always behaves as if |PKCS7_NOATTR| and
// |PKCS7_BINARY| are set. It honors the specification (or elision) of
// |PKCS7_DETACHED|. It does not allow |PKCS7_NOCERTS|.
//
// Note this function only implements a subset of the corresponding OpenSSL
// function. It is provided for backwards compatibility only.
OPENSSL_EXPORT OPENSSL_DEPRECATED PKCS7 *PKCS7_sign(X509 *sign_cert,
                                                    EVP_PKEY *pkey,
                                                    STACK_OF(X509) *certs,
                                                    BIO *data, int flags);

// PKCS7_verify takes in a |p7| with signed ContentInfo and verifies its
// signature against |certs| or |store|. If |certs| is specified, this function
// will attempt to verify |p7|'s signature against those certificates' public
// keys. If |store| is specified, its contents will be treated as certificate
// authorities (CAs) for establishing trust of any certificates bundled in |p7|.
//
// If |p7| is detached, |indata| must contain the data over which |p7|'s
// signature was computed. If verification succeeds, the verified content is
// written to |out| and 1 is returned. On error or verification failure, 0 is
// returned.
//
// Flags: If |PKCS7_NOVERIFY| is specified, trust chain validation is skipped.
// This function also enforces the behavior of OpenSSL's |PKCS7_NO_DUAL_CONTENT|
// meaning that |indata| may not be specified if |p7|'s signed data is attached.
// If |PKCS7_NOINTERN| is set, this function will not verify against certificate
// public keys included |p7|, instead relying solely on |certs|, which must be
// specified.
OPENSSL_EXPORT OPENSSL_DEPRECATED int PKCS7_verify(PKCS7 *p7,
                                                   STACK_OF(X509) *certs,
                                                   X509_STORE *store,
                                                   BIO *indata, BIO *outdata,
                                                   int flags);

// PKCS7_is_detached returns 0 if |p7| has attached content and 1 otherwise.
OPENSSL_EXPORT OPENSSL_DEPRECATED int PKCS7_is_detached(PKCS7 *p7);

// PKCS7_set_detached frees the attached content of |p7| if |detach| is set to
// 1. It returns 0 if otherwise or if |p7| is not of type signed.
//
// Note: |detach| is intended to be a boolean and MUST be set with either 1 or
//       0.
OPENSSL_EXPORT OPENSSL_DEPRECATED int PKCS7_set_detached(PKCS7 *p7, int detach);

// PKCS7_get_detached returns 0 if |p7| has attached content and 1 otherwise.
OPENSSL_EXPORT OPENSSL_DEPRECATED int PKCS7_get_detached(PKCS7 *p7);

// PKCS7_dataInit creates or initializes a BIO chain for reading data from or
// writing data to |p7|. If |bio| is non-null, it is added to the chain.
// Otherwise, a new BIO is allocated and returned to anchor the chain.
OPENSSL_EXPORT OPENSSL_DEPRECATED BIO *PKCS7_dataInit(PKCS7 *p7, BIO *bio);

// PKCS7_dataFinal serializes data written to |bio|'s chain into |p7|. It should
// only be called on BIO chains created by |PKCS7_dataInit|.
OPENSSL_EXPORT OPENSSL_DEPRECATED int PKCS7_dataFinal(PKCS7 *p7, BIO *bio);

// PKCS7_set_digest sets |p7|'s digest to |md|. It returns 1 on success and 0 if
// |p7| is of the wrong content type.
OPENSSL_EXPORT OPENSSL_DEPRECATED int PKCS7_set_digest(PKCS7 *p7,
                                                       const EVP_MD *md);

// PKCS7_get_recipient_info returns a pointer to a stack containing |p7|'s
// |PKCS7_RECIP_INFO| or NULL if none are present.
OPENSSL_EXPORT OPENSSL_DEPRECATED STACK_OF(PKCS7_RECIP_INFO) *
PKCS7_get_recipient_info(PKCS7 *p7);

// PKCS7_add_recipient allocates a new |PCKS7_RECEPIENT_INFO|, adds |x509| to it
// and returns that |PCKS7_RECEPIENT_INFO|.
OPENSSL_EXPORT OPENSSL_DEPRECATED PKCS7_RECIP_INFO *PKCS7_add_recipient(
    PKCS7 *p7, X509 *x509);

// PKCS7_get0_signers retrieves the signer's certificates from p7. It does not
// check their validity or whether any signatures are valid. The caller owns the
// returned X509 stack and is responsible for freeing it.
OPENSSL_EXPORT OPENSSL_DEPRECATED STACK_OF(X509) *PKCS7_get0_signers(
    PKCS7 *p7, STACK_OF(X509) *certs, int flags);

// PKCS7_encrypt encrypts the contents of |in| with |cipher| and adds |certs| as
// recipient infos and returns an encrypted |PKCS7| or NULL on failed
// encryption. |flags| is ignored. We only perform key encryption using RSA, so
// |certs| must use RSA public keys.
OPENSSL_EXPORT OPENSSL_DEPRECATED PKCS7 *PKCS7_encrypt(STACK_OF(X509) *certs,
                                                       BIO *in,
                                                       const EVP_CIPHER *cipher,
                                                       int flags);

// PKCS7_decrypt decrypts |p7| with |pkey| and writes the plaintext to |data|.
// If |cert| is present, it's public key is checked against |pkey| and |p7|'s
// recipient infos. 1 is returned on success and 0 on failure. |flags| is
// ignored. |pkey| must be an |EVP_PKEY_RSA|.
//
// NOTE: If |p7| was encrypted with a stream cipher, this operation may return 1
// even on decryption failure. The reason for this is detailed in RFC 3218 and
// comments in the |PKCS7_decrypt| source.
OPENSSL_EXPORT OPENSSL_DEPRECATED int PKCS7_decrypt(PKCS7 *p7, EVP_PKEY *pkey,
                                                    X509 *cert, BIO *data,
                                                    int flags);

// No-ops
//
// These functions do nothing. They're provided solely for build compatibility

// SMIME_read_PKCS7 is a no-op and returns NULL
OPENSSL_EXPORT OPENSSL_DEPRECATED PKCS7 *SMIME_read_PKCS7(BIO *in, BIO **bcont);

// SMIME_write_PKCS7 is a no-op and returns 0
OPENSSL_EXPORT OPENSSL_DEPRECATED int SMIME_write_PKCS7(BIO *out, PKCS7 *p7,
                                                        BIO *data, int flags);

// PKCS7_print_ctx prints "PKCS7 printing is not supported" and returns 1.
OPENSSL_EXPORT OPENSSL_DEPRECATED int PKCS7_print_ctx(BIO *bio, PKCS7 *pkcs7,
                                                      int indent,
                                                      const ASN1_PCTX *pctx);

#if defined(__cplusplus)
}  // extern C

extern "C++" {
BSSL_NAMESPACE_BEGIN

BORINGSSL_MAKE_DELETER(PKCS7, PKCS7_free)
BORINGSSL_MAKE_DELETER(PKCS7_SIGNER_INFO, PKCS7_SIGNER_INFO_free)

BSSL_NAMESPACE_END
}  // extern C++
#endif

#define PKCS7_R_BAD_PKCS7_VERSION 100
#define PKCS7_R_NOT_PKCS7_SIGNED_DATA 101
#define PKCS7_R_NO_CERTIFICATES_INCLUDED 102
#define PKCS7_R_NO_CRLS_INCLUDED 103
#define PKCS7_R_INVALID_NULL_POINTER 104
#define PKCS7_R_NO_CONTENT 105
#define PKCS7_R_CIPHER_NOT_INITIALIZED 106
#define PKCS7_R_UNSUPPORTED_CONTENT_TYPE 107
#define PKCS7_R_UNABLE_TO_FIND_MESSAGE_DIGEST 108
#define PKCS7_R_UNABLE_TO_FIND_MEM_BIO 109
#define PKCS7_R_WRONG_CONTENT_TYPE 110
#define PKCS7_R_CONTENT_AND_DATA_PRESENT 111
#define PKCS7_R_NO_SIGNATURES_ON_DATA 112
#define PKCS7_R_CERTIFICATE_VERIFY_ERROR 113
#define PKCS7_R_SMIME_TEXT_ERROR 114
#define PKCS7_R_SIGNATURE_FAILURE 115
#define PKCS7_R_NO_SIGNERS 116
#define PKCS7_R_SIGNER_CERTIFICATE_NOT_FOUND 117
#define PKCS7_R_ERROR_SETTING_CIPHER 118
#define PKCS7_R_ERROR_ADDING_RECIPIENT 119
#define PKCS7_R_PRIVATE_KEY_DOES_NOT_MATCH_CERTIFICATE 120
#define PKCS7_R_DECRYPT_ERROR 121
#define PKCS7_R_PKCS7_DATASIGN 122
#define PKCS7_R_CIPHER_HAS_NO_OBJECT_IDENTIFIER 123
#define PKCS7_R_SIGNING_NOT_SUPPORTED_FOR_THIS_KEY_TYPE 124
#define PKCS7_R_UNKNOWN_DIGEST_TYPE 125
#define PKCS7_R_INVALID_SIGNED_DATA_TYPE 126
#define PKCS7_R_UNSUPPORTED_CIPHER_TYPE 127
#define PKCS7_R_NO_RECIPIENT_MATCHES_CERTIFICATE 128
#define PKCS7_R_DIGEST_FAILURE 129
#define PKCS7_R_WRONG_PKCS7_TYPE 130
#define PKCS7_R_PKCS7_ADD_SIGNER_ERROR 131
#define PKCS7_R_PKCS7_ADD_SIGNATURE_ERROR 132
#define PKCS7_R_NO_DEFAULT_DIGEST 133
#define PKCS7_R_CERT_MUST_BE_RSA 134
#define PKCS7_R_OPERATION_NOT_SUPPORTED_ON_THIS_TYPE 135

#endif  // OPENSSL_HEADER_PKCS7_H
