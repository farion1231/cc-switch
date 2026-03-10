/* Written by Dr Stephen N Henson (steve@openssl.org) for the OpenSSL
 * project 1999.
 */
/* ====================================================================
 * Copyright (c) 1999 The OpenSSL Project.  All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 *
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 *
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in
 *    the documentation and/or other materials provided with the
 *    distribution.
 *
 * 3. All advertising materials mentioning features or use of this
 *    software must display the following acknowledgment:
 *    "This product includes software developed by the OpenSSL Project
 *    for use in the OpenSSL Toolkit. (http://www.OpenSSL.org/)"
 *
 * 4. The names "OpenSSL Toolkit" and "OpenSSL Project" must not be used to
 *    endorse or promote products derived from this software without
 *    prior written permission. For written permission, please contact
 *    licensing@OpenSSL.org.
 *
 * 5. Products derived from this software may not be called "OpenSSL"
 *    nor may "OpenSSL" appear in their names without prior written
 *    permission of the OpenSSL Project.
 *
 * 6. Redistributions of any form whatsoever must retain the following
 *    acknowledgment:
 *    "This product includes software developed by the OpenSSL Project
 *    for use in the OpenSSL Toolkit (http://www.OpenSSL.org/)"
 *
 * THIS SOFTWARE IS PROVIDED BY THE OpenSSL PROJECT ``AS IS'' AND ANY
 * EXPRESSED OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE OpenSSL PROJECT OR
 * ITS CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
 * SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT
 * NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
 * LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT,
 * STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED
 * OF THE POSSIBILITY OF SUCH DAMAGE.
 * ====================================================================
 *
 * This product includes cryptographic software written by Eric Young
 * (eay@cryptsoft.com).  This product includes software written by Tim
 * Hudson (tjh@cryptsoft.com). */


#ifndef OPENSSL_HEADER_PKCS8_H
#define OPENSSL_HEADER_PKCS8_H

#include <openssl/base.h>
#include <openssl/x509.h>


#if defined(__cplusplus)
extern "C" {
#endif


// PKCS8_encrypt serializes and encrypts a PKCS8_PRIV_KEY_INFO with PBES1 or
// PBES2 as defined in PKCS #5. Only pbeWithSHAAnd128BitRC4,
// pbeWithSHAAnd3-KeyTripleDES-CBC and pbeWithSHA1And40BitRC2, defined in PKCS
// #12, and PBES2, are supported.  PBES2 is selected by setting |cipher| and
// passing -1 for |pbe_nid|.  Otherwise, PBES1 is used and |cipher| is ignored.
//
// |pass| is used as the password. If a PBES1 scheme from PKCS #12 is used, this
// will be converted to a raw byte string as specified in B.1 of PKCS #12. If
// |pass| is NULL, it will be encoded as the empty byte string rather than two
// zero bytes, the PKCS #12 encoding of the empty string.
//
// If |salt| is NULL, a random salt of |salt_len| bytes is generated. If
// |salt_len| is zero, a default salt length is used instead.
//
// The resulting structure is stored in an |X509_SIG| which must be freed by the
// caller.
OPENSSL_EXPORT X509_SIG *PKCS8_encrypt(int pbe_nid, const EVP_CIPHER *cipher,
                                       const char *pass, int pass_len,
                                       const uint8_t *salt, size_t salt_len,
                                       int iterations,
                                       PKCS8_PRIV_KEY_INFO *p8inf);

// PKCS8_marshal_encrypted_private_key behaves like |PKCS8_encrypt| but encrypts
// an |EVP_PKEY| and writes the serialized EncryptedPrivateKeyInfo to |out|. It
// returns one on success and zero on error.
OPENSSL_EXPORT int PKCS8_marshal_encrypted_private_key(
    CBB *out, int pbe_nid, const EVP_CIPHER *cipher, const char *pass,
    size_t pass_len, const uint8_t *salt, size_t salt_len, int iterations,
    const EVP_PKEY *pkey);

// PKCS8_decrypt decrypts and decodes a PKCS8_PRIV_KEY_INFO with PBES1 or PBES2
// as defined in PKCS #5. Only pbeWithSHAAnd128BitRC4,
// pbeWithSHAAnd3-KeyTripleDES-CBC and pbeWithSHA1And40BitRC2, and PBES2,
// defined in PKCS #12, are supported.
//
// |pass| is used as the password. If a PBES1 scheme from PKCS #12 is used, this
// will be converted to a raw byte string as specified in B.1 of PKCS #12. If
// |pass| is NULL, it will be encoded as the empty byte string rather than two
// zero bytes, the PKCS #12 encoding of the empty string.
//
// The resulting structure must be freed by the caller.
OPENSSL_EXPORT PKCS8_PRIV_KEY_INFO *PKCS8_decrypt(X509_SIG *pkcs8,
                                                  const char *pass,
                                                  int pass_len);

// PKCS8_parse_encrypted_private_key behaves like |PKCS8_decrypt| but it parses
// the EncryptedPrivateKeyInfo structure from |cbs| and advances |cbs|. It
// returns a newly-allocated |EVP_PKEY| on success and zero on error.
OPENSSL_EXPORT EVP_PKEY *PKCS8_parse_encrypted_private_key(CBS *cbs,
                                                           const char *pass,
                                                           size_t pass_len);

// PKCS12_get_key_and_certs parses a PKCS#12 structure from |in|, authenticates
// and decrypts it using |password|, sets |*out_key| to the included private
// key and appends the included certificates to |out_certs|. It returns one on
// success and zero on error. The caller takes ownership of the outputs.
// Any friendlyName attributes (RFC 2985) in the PKCS#12 structure will be
// returned on the |X509| objects as aliases. See also |X509_alias_get0|.
OPENSSL_EXPORT int PKCS12_get_key_and_certs(EVP_PKEY **out_key,
                                            STACK_OF(X509) *out_certs, CBS *in,
                                            const char *password);


// Deprecated functions.

// PKCS12_PBE_add does nothing. It exists for compatibility with OpenSSL.
OPENSSL_EXPORT void PKCS12_PBE_add(void);

// d2i_PKCS12 is a dummy function that copies |*ber_bytes| into a
// |PKCS12| structure. The |out_p12| argument should be NULL(✝). On exit,
// |*ber_bytes| will be advanced by |ber_len|. It returns a fresh |PKCS12|
// structure or NULL on error.
//
// Note: unlike other d2i functions, |d2i_PKCS12| will always consume |ber_len|
// bytes.
//
// (✝) If |out_p12| is not NULL and the function is successful, |*out_p12| will
// be freed if not NULL itself and the result will be written to |*out_p12|.
// New code should not depend on this.
OPENSSL_EXPORT PKCS12 *d2i_PKCS12(PKCS12 **out_p12, const uint8_t **ber_bytes,
                                  size_t ber_len);

// d2i_PKCS12_bio acts like |d2i_PKCS12| but reads from a |BIO|.
OPENSSL_EXPORT PKCS12 *d2i_PKCS12_bio(BIO *bio, PKCS12 **out_p12);

// d2i_PKCS12_fp acts like |d2i_PKCS12| but reads from a |FILE|.
OPENSSL_EXPORT PKCS12 *d2i_PKCS12_fp(FILE *fp, PKCS12 **out_p12);

// i2d_PKCS12 is a dummy function which copies the contents of |p12|. If |out|
// is not NULL then the result is written to |*out| and |*out| is advanced just
// past the output. It returns the number of bytes in the result, whether
// written or not, or a negative value on error.
OPENSSL_EXPORT int i2d_PKCS12(const PKCS12 *p12, uint8_t **out);

// i2d_PKCS12_bio writes the contents of |p12| to |bio|. It returns one on
// success and zero on error.
OPENSSL_EXPORT int i2d_PKCS12_bio(BIO *bio, const PKCS12 *p12);

// i2d_PKCS12_fp writes the contents of |p12| to |fp|. It returns one on
// success and zero on error.
OPENSSL_EXPORT int i2d_PKCS12_fp(FILE *fp, const PKCS12 *p12);

// PKCS12_parse calls |PKCS12_get_key_and_certs| on the ASN.1 data stored in
// |p12|. The |out_pkey| and |out_cert| arguments must not be NULL and, on
// successful exit, the private key and matching certificate will be stored in
// them. The |out_ca_certs| argument may be NULL but, if not, then any extra
// certificates will be appended to |*out_ca_certs|. If |*out_ca_certs| is NULL
// then it will be set to a freshly allocated stack containing the extra certs.
//
// Note if |p12| does not contain a private key, both |*out_pkey| and
// |*out_cert| will be set to NULL and all certificates will be returned via
// |*out_ca_certs|. Also note this function differs from OpenSSL in that extra
// certificates are returned in the order they appear in the file. OpenSSL 1.1.1
// returns them in reverse order, but this will be fixed in OpenSSL 3.0.
//
// It returns one on success and zero on error.
//
// Use |PKCS12_get_key_and_certs| instead.
OPENSSL_EXPORT int PKCS12_parse(const PKCS12 *p12, const char *password,
                                EVP_PKEY **out_pkey, X509 **out_cert,
                                STACK_OF(X509) **out_ca_certs);

// PKCS12_set_mac generates the MAC for |p12| with the designated |password|,
// |salt|, |mac_iterations|, and |md| specified. |password| MUST be the same
// password originally used to encrypt |p12|. Although OpenSSL will allow an
// invalid state with a different |password|, AWS-LC will throw an error and
// return 0.
//
// If |salt| is NULL, a random salt of |salt_len| bytes is generated. If
// |salt_len| is zero, a default salt length is used instead.
// If |md| is NULL, the default is use SHA1 to align with OpenSSL.
//
// TODO (CryptoAlg-2897): Update the default |md| to SHA-256 to align with
//                        OpenSSL 3.x.
OPENSSL_EXPORT int PKCS12_set_mac(PKCS12 *p12, const char *password,
                                  int password_len, unsigned char *salt,
                                  int salt_len, int mac_iterations,
                                  const EVP_MD *md);

// PKCS12_verify_mac returns one if |password| is a valid password for |p12|
// and zero otherwise. Since |PKCS12_parse| doesn't take a length parameter,
// it's not actually possible to use a non-NUL-terminated password to actually
// get anything from a |PKCS12|. Thus |password| and |password_len| may be
// |NULL| and zero, respectively, or else |password_len| may be -1, or else
// |password[password_len]| must be zero and no other NUL bytes may appear in
// |password|. If the |password_len| checks fail, zero is returned
// immediately.
OPENSSL_EXPORT int PKCS12_verify_mac(const PKCS12 *p12, const char *password,
                                     int password_len);

// PKCS12_DEFAULT_ITER is the default number of KDF iterations used when
// creating a |PKCS12| object.
#define PKCS12_DEFAULT_ITER 2048

// PKCS12_create returns a newly-allocated |PKCS12| object containing |pkey|,
// |cert|, and |chain|, encrypted with the specified password. |name|, if not
// NULL, specifies a user-friendly name to encode with the key and
// certificate. The key and certificates are encrypted with |key_nid| and
// |cert_nid|, respectively, using |iterations| iterations in the
// KDF. |mac_iterations| is the number of iterations when deriving the MAC
// key. |key_type| must be zero. |pkey| and |cert| may be NULL to omit them.
//
// Each of |key_nid|, |cert_nid|, |iterations|, and |mac_iterations| may be zero
// to use defaults, which are |NID_pbe_WithSHA1And3_Key_TripleDES_CBC|,
// |NID_pbe_WithSHA1And40BitRC2_CBC|, |PKCS12_DEFAULT_ITER|, and one,
// respectively.
//
// |key_nid| or |cert_nid| may also be -1 to disable encryption of the key or
// certificate, respectively. This option is not recommended and is only
// implemented for compatibility with external packages. Note the output still
// requires a password for the MAC. Unencrypted keys in PKCS#12 are also not
// widely supported and may not open in other implementations.
//
// If |cert| or |chain| have associated aliases (see |X509_alias_set1|), they
// will be included in the output as friendlyName attributes (RFC 2985). It is
// an error to specify both an alias on |cert| and a non-NULL |name|
// parameter.
OPENSSL_EXPORT PKCS12 *PKCS12_create(const char *password, const char *name,
                                     const EVP_PKEY *pkey, X509 *cert,
                                     const STACK_OF(X509) *chain, int key_nid,
                                     int cert_nid, int iterations,
                                     int mac_iterations, int key_type);

// PKCS12_new returns a newly-allocated |PKCS12| object.
OPENSSL_EXPORT PKCS12 *PKCS12_new(void);

// PKCS12_free frees |p12| and its contents.
OPENSSL_EXPORT void PKCS12_free(PKCS12 *p12);


#if defined(__cplusplus)
}  // extern C

extern "C++" {

BSSL_NAMESPACE_BEGIN

BORINGSSL_MAKE_DELETER(PKCS12, PKCS12_free)
BORINGSSL_MAKE_DELETER(PKCS8_PRIV_KEY_INFO, PKCS8_PRIV_KEY_INFO_free)

BSSL_NAMESPACE_END

}  // extern C++

#endif

#define PKCS8_R_BAD_PKCS12_DATA 100
#define PKCS8_R_BAD_PKCS12_VERSION 101
#define PKCS8_R_CIPHER_HAS_NO_OBJECT_IDENTIFIER 102
#define PKCS8_R_CRYPT_ERROR 103
#define PKCS8_R_DECODE_ERROR 104
#define PKCS8_R_ENCODE_ERROR 105
#define PKCS8_R_ENCRYPT_ERROR 106
#define PKCS8_R_ERROR_SETTING_CIPHER_PARAMS 107
#define PKCS8_R_INCORRECT_PASSWORD 108
#define PKCS8_R_KEYGEN_FAILURE 109
#define PKCS8_R_KEY_GEN_ERROR 110
#define PKCS8_R_METHOD_NOT_SUPPORTED 111
#define PKCS8_R_MISSING_MAC 112
#define PKCS8_R_MULTIPLE_PRIVATE_KEYS_IN_PKCS12 113
#define PKCS8_R_PKCS12_PUBLIC_KEY_INTEGRITY_NOT_SUPPORTED 114
#define PKCS8_R_PKCS12_TOO_DEEPLY_NESTED 115
#define PKCS8_R_PRIVATE_KEY_DECODE_ERROR 116
#define PKCS8_R_PRIVATE_KEY_ENCODE_ERROR 117
#define PKCS8_R_TOO_LONG 118
#define PKCS8_R_UNKNOWN_ALGORITHM 119
#define PKCS8_R_UNKNOWN_CIPHER 120
#define PKCS8_R_UNKNOWN_CIPHER_ALGORITHM 121
#define PKCS8_R_UNKNOWN_DIGEST 122
#define PKCS8_R_UNKNOWN_HASH 123
#define PKCS8_R_UNSUPPORTED_PRIVATE_KEY_ALGORITHM 124
#define PKCS8_R_UNSUPPORTED_KEYLENGTH 125
#define PKCS8_R_UNSUPPORTED_SALT_TYPE 126
#define PKCS8_R_UNSUPPORTED_CIPHER 127
#define PKCS8_R_UNSUPPORTED_KEY_DERIVATION_FUNCTION 128
#define PKCS8_R_BAD_ITERATION_COUNT 129
#define PKCS8_R_UNSUPPORTED_PRF 130
#define PKCS8_R_INVALID_CHARACTERS 131
#define PKCS8_R_UNSUPPORTED_OPTIONS 132
#define PKCS8_R_AMBIGUOUS_FRIENDLY_NAME 133

// PKCS12_R_MAC_VERIFY_FAILURE is an error code defined for
// compatability. It points to our equivalent for this OpenSSL error,
// |PKCS8_R_INCORRECT_PASSWORD|
#define PKCS12_R_MAC_VERIFY_FAILURE PKCS8_R_INCORRECT_PASSWORD

#endif  // OPENSSL_HEADER_PKCS8_H
