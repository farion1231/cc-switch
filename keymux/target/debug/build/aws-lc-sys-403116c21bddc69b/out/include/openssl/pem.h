/* Copyright (C) 1995-1997 Eric Young (eay@cryptsoft.com)
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

#ifndef OPENSSL_HEADER_PEM_H
#define OPENSSL_HEADER_PEM_H

#include <openssl/base64.h>
#include <openssl/bio.h>
#include <openssl/cipher.h>
#include <openssl/digest.h>
#include <openssl/evp.h>
#include <openssl/pkcs7.h>
#include <openssl/stack.h>
#include <openssl/x509.h>

// For compatibility with open-iscsi, which assumes that it can get
// |OPENSSL_malloc| from pem.h or err.h
#include <openssl/crypto.h>

#ifdef __cplusplus
extern "C" {
#endif


#define PEM_BUFSIZE 1024

#define PEM_STRING_X509_OLD "X509 CERTIFICATE"
#define PEM_STRING_X509 "CERTIFICATE"
#define PEM_STRING_X509_PAIR "CERTIFICATE PAIR"
#define PEM_STRING_X509_TRUSTED "TRUSTED CERTIFICATE"
#define PEM_STRING_X509_REQ_OLD "NEW CERTIFICATE REQUEST"
#define PEM_STRING_X509_REQ "CERTIFICATE REQUEST"
#define PEM_STRING_X509_CRL "X509 CRL"
#define PEM_STRING_EVP_PKEY "ANY PRIVATE KEY"
#define PEM_STRING_PUBLIC "PUBLIC KEY"
#define PEM_STRING_RSA "RSA PRIVATE KEY"
#define PEM_STRING_RSA_PUBLIC "RSA PUBLIC KEY"
#define PEM_STRING_DSA "DSA PRIVATE KEY"
#define PEM_STRING_DSA_PUBLIC "DSA PUBLIC KEY"
#define PEM_STRING_EC "EC PRIVATE KEY"
#define PEM_STRING_PKCS7 "PKCS7"
#define PEM_STRING_PKCS7_SIGNED "PKCS #7 SIGNED DATA"
#define PEM_STRING_PKCS8 "ENCRYPTED PRIVATE KEY"
#define PEM_STRING_PKCS8INF "PRIVATE KEY"
#define PEM_STRING_DHPARAMS "DH PARAMETERS"
#define PEM_STRING_SSL_SESSION "SSL SESSION PARAMETERS"
#define PEM_STRING_DSAPARAMS "DSA PARAMETERS"
#define PEM_STRING_ECDSA_PUBLIC "ECDSA PUBLIC KEY"
#define PEM_STRING_ECPARAMETERS "EC PARAMETERS"
#define PEM_STRING_ECPRIVATEKEY "EC PRIVATE KEY"
#define PEM_STRING_PARAMETERS "PARAMETERS"
#define PEM_STRING_CMS "CMS"

// enc_type is one off
#define PEM_TYPE_ENCRYPTED 10
#define PEM_TYPE_MIC_ONLY 20
#define PEM_TYPE_MIC_CLEAR 30
#define PEM_TYPE_CLEAR 40

// For compatibility with OpenSSL. First argument ignored.
#define PEMerr(f, r) OPENSSL_PUT_ERROR(PEM, (r))

// These macros make the PEM_read/PEM_write functions easier to maintain and
// write. Now they are all implemented with either:
// IMPLEMENT_PEM_rw(...) or IMPLEMENT_PEM_rw_cb(...)


#define IMPLEMENT_PEM_read_fp(name, type, str, asn1)                         \
  static void *pem_read_##name##_d2i(void **x, const unsigned char **inp,    \
                                     long len) {                             \
    return d2i_##asn1((type **)x, inp, len);                                 \
  }                                                                          \
  OPENSSL_EXPORT type *PEM_read_##name(FILE *fp, type **x,                   \
                                       pem_password_cb *cb, void *u) {       \
    return (type *)PEM_ASN1_read(pem_read_##name##_d2i, str, fp, (void **)x, \
                                 cb, u);                                     \
  }

#define IMPLEMENT_PEM_write_fp(name, type, str, asn1)                        \
  static int pem_write_##name##_i2d(const void *x, unsigned char **outp) {   \
    return i2d_##asn1((type *)x, outp);                                      \
  }                                                                          \
  OPENSSL_EXPORT int PEM_write_##name(FILE *fp, type *x) {                   \
    return PEM_ASN1_write(pem_write_##name##_i2d, str, fp, x, NULL, NULL, 0, \
                          NULL, NULL);                                       \
  }

#define IMPLEMENT_PEM_write_fp_const(name, type, str, asn1)                 \
  static int pem_write_##name##_i2d(const void *x, unsigned char **outp) {  \
    return i2d_##asn1((const type *)x, outp);                               \
  }                                                                         \
  OPENSSL_EXPORT int PEM_write_##name(FILE *fp, const type *x) {            \
    return PEM_ASN1_write(pem_write_##name##_i2d, str, fp, (void *)x, NULL, \
                          NULL, 0, NULL, NULL);                             \
  }

#define IMPLEMENT_PEM_write_cb_fp(name, type, str, asn1)                   \
  static int pem_write_##name##_i2d(const void *x, unsigned char **outp) { \
    return i2d_##asn1((type *)x, outp);                                    \
  }                                                                        \
  OPENSSL_EXPORT int PEM_write_##name(                                     \
      FILE *fp, type *x, const EVP_CIPHER *enc, const unsigned char *pass, \
      int pass_len, pem_password_cb *cb, void *u) {                        \
    return PEM_ASN1_write(pem_write_##name##_i2d, str, fp, x, enc, pass,   \
                          pass_len, cb, u);                                \
  }

#define IMPLEMENT_PEM_write_cb_fp_const(name, type, str, asn1)             \
  static int pem_write_##name##_i2d(const void *x, unsigned char **outp) { \
    return i2d_##asn1((const type *)x, outp);                              \
  }                                                                        \
  OPENSSL_EXPORT int PEM_write_##name(                                     \
      FILE *fp, type *x, const EVP_CIPHER *enc, const unsigned char *pass, \
      int pass_len, pem_password_cb *cb, void *u) {                        \
    return PEM_ASN1_write(pem_write_##name##_i2d, str, fp, x, enc, pass,   \
                          pass_len, cb, u);                                \
  }


#define IMPLEMENT_PEM_read_bio(name, type, str, asn1)                         \
  static void *pem_read_bio_##name##_d2i(void **x, const unsigned char **inp, \
                                         long len) {                          \
    return d2i_##asn1((type **)x, inp, len);                                  \
  }                                                                           \
  OPENSSL_EXPORT type *PEM_read_bio_##name(BIO *bp, type **x,                 \
                                           pem_password_cb *cb, void *u) {    \
    return (type *)PEM_ASN1_read_bio(pem_read_bio_##name##_d2i, str, bp,      \
                                     (void **)x, cb, u);                      \
  }

#define IMPLEMENT_PEM_write_bio(name, type, str, asn1)                         \
  static int pem_write_bio_##name##_i2d(const void *x, unsigned char **outp) { \
    return i2d_##asn1((type *)x, outp);                                        \
  }                                                                            \
  OPENSSL_EXPORT int PEM_write_bio_##name(BIO *bp, type *x) {                  \
    return PEM_ASN1_write_bio(pem_write_bio_##name##_i2d, str, bp, x, NULL,    \
                              NULL, 0, NULL, NULL);                            \
  }

#define IMPLEMENT_PEM_write_bio_const(name, type, str, asn1)                   \
  static int pem_write_bio_##name##_i2d(const void *x, unsigned char **outp) { \
    return i2d_##asn1((const type *)x, outp);                                  \
  }                                                                            \
  OPENSSL_EXPORT int PEM_write_bio_##name(BIO *bp, const type *x) {            \
    return PEM_ASN1_write_bio(pem_write_bio_##name##_i2d, str, bp, (void *)x,  \
                              NULL, NULL, 0, NULL, NULL);                      \
  }

#define IMPLEMENT_PEM_write_cb_bio(name, type, str, asn1)                      \
  static int pem_write_bio_##name##_i2d(const void *x, unsigned char **outp) { \
    return i2d_##asn1((type *)x, outp);                                        \
  }                                                                            \
  OPENSSL_EXPORT int PEM_write_bio_##name(                                     \
      BIO *bp, type *x, const EVP_CIPHER *enc, const unsigned char *pass,      \
      int pass_len, pem_password_cb *cb, void *u) {                            \
    return PEM_ASN1_write_bio(pem_write_bio_##name##_i2d, str, bp, x, enc,     \
                              pass, pass_len, cb, u);                          \
  }

#define IMPLEMENT_PEM_write_cb_bio_const(name, type, str, asn1)                \
  static int pem_write_bio_##name##_i2d(const void *x, unsigned char **outp) { \
    return i2d_##asn1((const type *)x, outp);                                  \
  }                                                                            \
  OPENSSL_EXPORT int PEM_write_bio_##name(                                     \
      BIO *bp, type *x, const EVP_CIPHER *enc, const unsigned char *pass,      \
      int pass_len, pem_password_cb *cb, void *u) {                            \
    return PEM_ASN1_write_bio(pem_write_bio_##name##_i2d, str, bp, (void *)x,  \
                              enc, pass, pass_len, cb, u);                     \
  }

#define IMPLEMENT_PEM_write(name, type, str, asn1) \
  IMPLEMENT_PEM_write_bio(name, type, str, asn1)   \
  IMPLEMENT_PEM_write_fp(name, type, str, asn1)

#define IMPLEMENT_PEM_write_const(name, type, str, asn1) \
  IMPLEMENT_PEM_write_bio_const(name, type, str, asn1)   \
  IMPLEMENT_PEM_write_fp_const(name, type, str, asn1)

#define IMPLEMENT_PEM_write_cb(name, type, str, asn1) \
  IMPLEMENT_PEM_write_cb_bio(name, type, str, asn1)   \
  IMPLEMENT_PEM_write_cb_fp(name, type, str, asn1)

#define IMPLEMENT_PEM_write_cb_const(name, type, str, asn1) \
  IMPLEMENT_PEM_write_cb_bio_const(name, type, str, asn1)   \
  IMPLEMENT_PEM_write_cb_fp_const(name, type, str, asn1)

#define IMPLEMENT_PEM_read(name, type, str, asn1) \
  IMPLEMENT_PEM_read_bio(name, type, str, asn1)   \
  IMPLEMENT_PEM_read_fp(name, type, str, asn1)

#define IMPLEMENT_PEM_rw(name, type, str, asn1) \
  IMPLEMENT_PEM_read(name, type, str, asn1)     \
  IMPLEMENT_PEM_write(name, type, str, asn1)

#define IMPLEMENT_PEM_rw_const(name, type, str, asn1) \
  IMPLEMENT_PEM_read(name, type, str, asn1)           \
  IMPLEMENT_PEM_write_const(name, type, str, asn1)

#define IMPLEMENT_PEM_rw_cb(name, type, str, asn1) \
  IMPLEMENT_PEM_read(name, type, str, asn1)        \
  IMPLEMENT_PEM_write_cb(name, type, str, asn1)

// These are the same except they are for the declarations

#define DECLARE_PEM_read_fp(name, type)                    \
  OPENSSL_EXPORT type *PEM_read_##name(FILE *fp, type **x, \
                                       pem_password_cb *cb, void *u);

#define DECLARE_PEM_write_fp(name, type) \
  OPENSSL_EXPORT int PEM_write_##name(FILE *fp, type *x);

#define DECLARE_PEM_write_fp_const(name, type) \
  OPENSSL_EXPORT int PEM_write_##name(FILE *fp, const type *x);

#define DECLARE_PEM_write_cb_fp(name, type)                                \
  OPENSSL_EXPORT int PEM_write_##name(                                     \
      FILE *fp, type *x, const EVP_CIPHER *enc, const unsigned char *pass, \
      int pass_len, pem_password_cb *cb, void *u);

#define DECLARE_PEM_read_bio(name, type)                      \
  OPENSSL_EXPORT type *PEM_read_bio_##name(BIO *bp, type **x, \
                                           pem_password_cb *cb, void *u);

#define DECLARE_PEM_write_bio(name, type) \
  OPENSSL_EXPORT int PEM_write_bio_##name(BIO *bp, type *x);

#define DECLARE_PEM_write_bio_const(name, type) \
  OPENSSL_EXPORT int PEM_write_bio_##name(BIO *bp, const type *x);

#define DECLARE_PEM_write_cb_bio(name, type)                              \
  OPENSSL_EXPORT int PEM_write_bio_##name(                                \
      BIO *bp, type *x, const EVP_CIPHER *enc, const unsigned char *pass, \
      int pass_len, pem_password_cb *cb, void *u);


#define DECLARE_PEM_write(name, type) \
  DECLARE_PEM_write_bio(name, type)   \
  DECLARE_PEM_write_fp(name, type)

#define DECLARE_PEM_write_const(name, type) \
  DECLARE_PEM_write_bio_const(name, type)   \
  DECLARE_PEM_write_fp_const(name, type)

#define DECLARE_PEM_write_cb(name, type) \
  DECLARE_PEM_write_cb_bio(name, type)   \
  DECLARE_PEM_write_cb_fp(name, type)

#define DECLARE_PEM_read(name, type) \
  DECLARE_PEM_read_bio(name, type)   \
  DECLARE_PEM_read_fp(name, type)

#define DECLARE_PEM_rw(name, type) \
  DECLARE_PEM_read(name, type)     \
  DECLARE_PEM_write(name, type)

#define DECLARE_PEM_rw_const(name, type) \
  DECLARE_PEM_read(name, type)           \
  DECLARE_PEM_write_const(name, type)

#define DECLARE_PEM_rw_cb(name, type) \
  DECLARE_PEM_read(name, type)        \
  DECLARE_PEM_write_cb(name, type)

// "userdata": new with OpenSSL 0.9.4
typedef int pem_password_cb(char *buf, int size, int rwflag, void *userdata);

OPENSSL_EXPORT int PEM_get_EVP_CIPHER_INFO(char *header,
                                           EVP_CIPHER_INFO *cipher);

// PEM_do_header decrypts PEM-encoded data using the cipher info in |cipher|.
// It processes |data| of length |len| using a password obtained via |callback|
// (or the default callback provided via |PEM_def_callback| if NULL) with callback
// data |u|. It then updates |len| with decrypted length.
// Returns 1 on success or if |cipher| is NULL, 0 on failure.
OPENSSL_EXPORT int PEM_do_header(EVP_CIPHER_INFO *cipher, unsigned char *data,
                                 long *len, pem_password_cb *callback, void *u);

// PEM_read_bio reads from |bp|, until the next PEM block. If one is found, it
// returns one and sets |*name|, |*header|, and |*data| to newly-allocated
// buffers containing the PEM type, the header block, and the decoded data,
// respectively. |*name| and |*header| are NUL-terminated C strings, while
// |*data| has |*len| bytes. The caller must release each of |*name|, |*header|,
// and |*data| with |OPENSSL_free| when done. If no PEM block is found, this
// function returns zero and pushes |PEM_R_NO_START_LINE| to the error queue. If
// one is found, but there is an error decoding it, it returns zero and pushes
// some other error to the error queue.
OPENSSL_EXPORT int PEM_read_bio(BIO *bp, char **name, char **header,
                                unsigned char **data, long *len);

// PEM_write_bio writes a PEM block to |bp|, containing |len| bytes from |data|
// as data. |name| and |hdr| are NUL-terminated C strings containing the PEM
// type and header block, respectively. This function returns zero on error and
// the number of bytes written on success.
OPENSSL_EXPORT int PEM_write_bio(BIO *bp, const char *name, const char *hdr,
                                 const unsigned char *data, long len);

// PEM_bytes_read_bio reads PEM-formatted data from |bp| for the data type given
// in |name|. If a PEM block is found, it returns one and sets |*pnm| and
// |*pdata| to newly-allocated buffers containing the PEM type and the decoded
// data, respectively. |*pnm| is a NUL-terminated C string, while |*pdata| has
// |*plen| bytes. The caller must release each of |*pnm| and |*pdata| with
// |OPENSSL_free| when done. If no PEM block is found, this function returns
// zero and pushes |PEM_R_NO_START_LINE| to the error queue. If one is found,
// but there is an error decoding it, it returns zero and pushes some other
// error to the error queue. |cb| is the callback to use when querying for
// pass phrase used for encrypted PEM structures (normally only private keys)
// and |u| is interpreted as the null terminated string to use as the
// passphrase.
OPENSSL_EXPORT int PEM_bytes_read_bio(unsigned char **pdata, long *plen,
                                      char **pnm, const char *name, BIO *bp,
                                      pem_password_cb *cb, void *u);

OPENSSL_EXPORT void *PEM_ASN1_read_bio(d2i_of_void *d2i, const char *name,
                                       BIO *bp, void **x, pem_password_cb *cb,
                                       void *u);

// PEM_ASN1_write_bio writes ASN.1 structure |x| encoded by |i2d| to BIO |bp| in PEM format
// with name |name|. If |enc| is non-NULL, encrypts data using cipher with password from
// |pass| and |pass_len|, or via |callback| with user data |u| (uses PEM_def_callback if
// callback is NULL). Returns 1 on success, 0 on failure.
OPENSSL_EXPORT int PEM_ASN1_write_bio(i2d_of_void *i2d, const char *name,
                                      BIO *bp, void *x, const EVP_CIPHER *enc,
                                      const unsigned char *pass, int pass_len,
                                      pem_password_cb *cb, void *u);

// PEM_X509_INFO_read_bio reads PEM blocks from |bp| and decodes any
// certificates, CRLs, and private keys found. It returns a
// |STACK_OF(X509_INFO)| structure containing the results, or NULL on error.
//
// If |sk| is NULL, the result on success will be a newly-allocated
// |STACK_OF(X509_INFO)| structure which should be released with
// |sk_X509_INFO_pop_free| and |X509_INFO_free| when done.
//
// If |sk| is non-NULL, it appends the results to |sk| instead and returns |sk|
// on success. In this case, the caller retains ownership of |sk| in both
// success and failure.
//
// WARNING: If the input contains "TRUSTED CERTIFICATE" PEM blocks, this
// function parses auxiliary properties as in |d2i_X509_AUX|. Passing untrusted
// input to this function allows an attacker to influence those properties. See
// |d2i_X509_AUX| for details.
OPENSSL_EXPORT STACK_OF(X509_INFO) *PEM_X509_INFO_read_bio(
    BIO *bp, STACK_OF(X509_INFO) *sk, pem_password_cb *cb, void *u);

// PEM_X509_INFO_write_bio writes the contents of the |X509_INFO| structure |xi|
// to the |BIO| object |bp| in PEM format. If the X509_INFO contains a
// certificate (x509), it will be written after the private key (if any). Other
// fields in X509_INFO (such as CRLs) are currently ignored.
//
// It returns 1 on success and 0 on failure.
OPENSSL_EXPORT int PEM_X509_INFO_write_bio(BIO *bp, X509_INFO *xi,
                                           EVP_CIPHER *enc, unsigned char *kstr,
                                           int klen, pem_password_cb *cd,
                                           void *u);

// PEM_X509_INFO_read behaves like |PEM_X509_INFO_read_bio| but reads from a
// |FILE|.
OPENSSL_EXPORT STACK_OF(X509_INFO) *PEM_X509_INFO_read(FILE *fp,
                                                       STACK_OF(X509_INFO) *sk,
                                                       pem_password_cb *cb,
                                                       void *u);

OPENSSL_EXPORT int PEM_read(FILE *fp, char **name, char **header,
                            unsigned char **data, long *len);
OPENSSL_EXPORT int PEM_write(FILE *fp, const char *name, const char *hdr,
                             const unsigned char *data, long len);
OPENSSL_EXPORT void *PEM_ASN1_read(d2i_of_void *d2i, const char *name, FILE *fp,
                                   void **x, pem_password_cb *cb, void *u);
OPENSSL_EXPORT int PEM_ASN1_write(i2d_of_void *i2d, const char *name, FILE *fp,
                                  void *x, const EVP_CIPHER *enc,
                                  const unsigned char *pass, int pass_len,
                                  pem_password_cb *callback, void *u);

// PEM_def_callback provides a password for PEM encryption/decryption operations.
// This function is used as the default callback to provide a password for PEM
// functions such as |PEM_do_header| and |PEM_ASN1_write_bio|.
// If |userdata| is non-NULL, it treats |userdata| as a string and copies it
// into |buf|, assuming |size| is sufficient. If |userdata| is NULL, it prompts
// the user for a password using the prompt from EVP_get_pw_prompt() (or default
// "Enter PEM pass phrase:"). For encryption (|rwflag|=1), a minimum password
// length is enforced, while for decryption (|rwflag|=0) any password length is
// accepted. Returns the length of the password (excluding null
// terminator) on success, or 0 on error or if |buf| is null, if |buf| is too small,
// or |size| is negative, or |size| is smaller than user input length.
OPENSSL_EXPORT int PEM_def_callback(char *buf, int size, int rwflag,
                                    void *userdata);


DECLARE_PEM_rw(X509, X509)

// TODO(crbug.com/boringssl/426): When documenting these, copy the warning
// about auxiliary properties from |PEM_X509_INFO_read_bio|.
DECLARE_PEM_rw(X509_AUX, X509)

DECLARE_PEM_rw(X509_REQ, X509_REQ)
DECLARE_PEM_write(X509_REQ_NEW, X509_REQ)

DECLARE_PEM_rw(X509_CRL, X509_CRL)

DECLARE_PEM_rw(PKCS7, PKCS7)
DECLARE_PEM_rw(PKCS8, X509_SIG)

DECLARE_PEM_rw(PKCS8_PRIV_KEY_INFO, PKCS8_PRIV_KEY_INFO)

DECLARE_PEM_rw_cb(RSAPrivateKey, RSA)

DECLARE_PEM_rw_const(RSAPublicKey, RSA)
DECLARE_PEM_rw(RSA_PUBKEY, RSA)

#ifndef OPENSSL_NO_DSA

DECLARE_PEM_rw_cb(DSAPrivateKey, DSA)

DECLARE_PEM_rw(DSA_PUBKEY, DSA)

DECLARE_PEM_rw_const(DSAparams, DSA)

#endif

DECLARE_PEM_rw_cb(ECPrivateKey, EC_KEY)
DECLARE_PEM_rw(EC_PUBKEY, EC_KEY)


DECLARE_PEM_rw_const(DHparams, DH)


DECLARE_PEM_rw_cb(PrivateKey, EVP_PKEY)

DECLARE_PEM_rw(PUBKEY, EVP_PKEY)

OPENSSL_EXPORT int PEM_write_bio_PKCS8PrivateKey_nid(BIO *bp, const EVP_PKEY *x,
                                                     int nid, const char *pass,
                                                     int pass_len,
                                                     pem_password_cb *cb,
                                                     void *u);
OPENSSL_EXPORT int PEM_write_bio_PKCS8PrivateKey(BIO *bp, const EVP_PKEY *x,
                                                 const EVP_CIPHER *enc,
                                                 const char *pass, int pass_len,
                                                 pem_password_cb *cb, void *u);
OPENSSL_EXPORT int i2d_PKCS8PrivateKey_bio(BIO *bp, const EVP_PKEY *x,
                                           const EVP_CIPHER *enc,
                                           const char *pass, int pass_len,
                                           pem_password_cb *cb, void *u);
OPENSSL_EXPORT int i2d_PKCS8PrivateKey_nid_bio(BIO *bp, const EVP_PKEY *x,
                                               int nid, const char *pass,
                                               int pass_len,
                                               pem_password_cb *cb, void *u);
OPENSSL_EXPORT EVP_PKEY *d2i_PKCS8PrivateKey_bio(BIO *bp, EVP_PKEY **x,
                                                 pem_password_cb *cb, void *u);

OPENSSL_EXPORT int i2d_PKCS8PrivateKey_fp(FILE *fp, const EVP_PKEY *x,
                                          const EVP_CIPHER *enc,
                                          const char *pass, int pass_len,
                                          pem_password_cb *cb, void *u);
OPENSSL_EXPORT int i2d_PKCS8PrivateKey_nid_fp(FILE *fp, const EVP_PKEY *x,
                                              int nid, const char *pass,
                                              int pass_len, pem_password_cb *cb,
                                              void *u);
OPENSSL_EXPORT int PEM_write_PKCS8PrivateKey_nid(FILE *fp, const EVP_PKEY *x,
                                                 int nid, const char *pass,
                                                 int pass_len,
                                                 pem_password_cb *cb, void *u);

OPENSSL_EXPORT EVP_PKEY *d2i_PKCS8PrivateKey_fp(FILE *fp, EVP_PKEY **x,
                                                pem_password_cb *cb, void *u);

OPENSSL_EXPORT int PEM_write_PKCS8PrivateKey(FILE *fp, const EVP_PKEY *x,
                                             const EVP_CIPHER *enc,
                                             const char *pass, int pass_len,
                                             pem_password_cb *cd, void *u);

// PEM_read_bio_Parameters is a generic PEM deserialization function that
// parses the public "parameters" in |bio| and returns a corresponding
// |EVP_PKEY|. If |*pkey| is non-null, the original |*pkey| is freed and the
// returned |EVP_PKEY| is also written to |*pkey|. |*pkey| must be either NULL
// or an allocated value, passing in an uninitialized pointer is undefined
// behavior. This is only supported with |EVP_PKEY_EC|, |EVP_PKEY_DH|, and
// |EVP_PKEY_DSA|.
OPENSSL_EXPORT EVP_PKEY *PEM_read_bio_Parameters(BIO *bio, EVP_PKEY **pkey);

// PEM_write_bio_Parameters is a generic PEM serialization function that parses
// the public "parameters" of |pkey| to |bio|. It returns 1 on success or 0 on
// failure. This is only supported with |EVP_PKEY_EC|, |EVP_PKEY_DH|, and
// |EVP_PKEY_DSA|.
OPENSSL_EXPORT int PEM_write_bio_Parameters(BIO *bio, EVP_PKEY *pkey);

// PEM_read_bio_ECPKParameters deserializes the PEM file written in |bio|
// according to |ECPKParameters| in RFC 3279. It returns the |EC_GROUP|
// corresponding to deserialized output and also writes it to |out_group|. Only
// deserialization of namedCurves or explicitly-encoded versions of namedCurves
// are supported.
OPENSSL_EXPORT EC_GROUP *PEM_read_bio_ECPKParameters(BIO *bio,
                                                     EC_GROUP **out_group,
                                                     pem_password_cb *cb,
                                                     void *u);

// PEM_write_bio_ECPKParameters serializes |group| as a PEM file to |out|
// according to |ECPKParameters| in RFC 3279. Only serialization of namedCurves
// are supported.
OPENSSL_EXPORT int PEM_write_bio_ECPKParameters(BIO *out,
                                                const EC_GROUP *group);

// PEM_write_bio_PrivateKey_traditional calls |PEM_ASN1_write_bio| to write
// out |x|'s private key in the "traditional" ASN1 format. Use
// |PEM_write_bio_PrivateKey| instead.
OPENSSL_EXPORT int PEM_write_bio_PrivateKey_traditional(
    BIO *bp, EVP_PKEY *x, const EVP_CIPHER *enc, unsigned char *kstr, int klen,
    pem_password_cb *cb, void *u);

#ifdef __cplusplus
}  // extern "C"
#endif

#define PEM_R_BAD_BASE64_DECODE 100
#define PEM_R_BAD_DECRYPT 101
#define PEM_R_BAD_END_LINE 102
#define PEM_R_BAD_IV_CHARS 103
#define PEM_R_BAD_PASSWORD_READ 104
#define PEM_R_CIPHER_IS_NULL 105
#define PEM_R_ERROR_CONVERTING_PRIVATE_KEY 106
#define PEM_R_NOT_DEK_INFO 107
#define PEM_R_NOT_ENCRYPTED 108
#define PEM_R_NOT_PROC_TYPE 109
#define PEM_R_NO_START_LINE 110
#define PEM_R_READ_KEY 111
#define PEM_R_SHORT_HEADER 112
#define PEM_R_UNSUPPORTED_CIPHER 113
#define PEM_R_UNSUPPORTED_ENCRYPTION 114
#define PEM_R_PROBLEMS_GETTING_PASSWORD 115

#endif  // OPENSSL_HEADER_PEM_H
