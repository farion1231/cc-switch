/*
 * Copyright 2015-2021 The OpenSSL Project Authors. All Rights Reserved.
 *
 * Licensed under the Apache License 2.0 (the "License").  You may not use
 * this file except in compliance with the License.  You can obtain a copy
 * in the file LICENSE in the source distribution or at
 * https://www.openssl.org/source/license.html
 */

#ifndef AWSLC_OCSP_H
#define AWSLC_OCSP_H

#include <openssl/asn1t.h>
#include <openssl/safestack.h>
#include <openssl/x509.h>
#include <openssl/x509v3.h>

#if defined(__cplusplus)
extern "C" {
#endif

// Various OCSP flags and values


// The following constants are OCSP reason codes identify the reason for the
// certificate revocation.
//
//  CRLReason ::= ENUMERATED {
//        unspecified             (0),
//        keyCompromise           (1),
//        cACompromise            (2),
//        affiliationChanged      (3),
//        superseded              (4),
//        cessationOfOperation    (5),
//        -- value 7 is not used
//        certificateHold         (6),
//        removeFromCRL           (8),
//        privilegeWithdrawn      (9),
//        aACompromise            (10) }
//
// Reason Code RFC: https://www.rfc-editor.org/rfc/rfc5280#section-5.3.1
//
// Note: OCSP_REVOKED_STATUS_NOSTATUS is defined by OpenSSL and is not defined
//       within the RFC.
#define OCSP_REVOKED_STATUS_NOSTATUS -1
#define OCSP_REVOKED_STATUS_UNSPECIFIED 0
#define OCSP_REVOKED_STATUS_KEYCOMPROMISE 1
#define OCSP_REVOKED_STATUS_CACOMPROMISE 2
#define OCSP_REVOKED_STATUS_AFFILIATIONCHANGED 3
#define OCSP_REVOKED_STATUS_SUPERSEDED 4
#define OCSP_REVOKED_STATUS_CESSATIONOFOPERATION 5
#define OCSP_REVOKED_STATUS_CERTIFICATEHOLD 6
#define OCSP_REVOKED_STATUS_REMOVEFROMCRL 8
#define OCSP_REVOKED_STATUS_PRIVILEGEWITHDRAWN 9
#define OCSP_REVOKED_STATUS_AACOMPROMISE 10

// OCSP_NOCERTS is for |OCSP_request_sign| and |OCSP_basic_sign|. Setting
// this excludes certificates request/response and ignores the |certs|
// parameter. Certificates are optional.
#define OCSP_NOCERTS 0x1
// OCSP_NOINTERN is for |OCSP_basic_verify| and |OCSP_request_verify|.
// Certificates included within |bs| or |req| will be included in the
// search for the signing certificate by default, unless |OCSP_NOINTERN| is set.
#define OCSP_NOINTERN 0x2
// OCSP_NOCHAIN is for |OCSP_basic_verify| and |OCSP_request_verify|.
// For |OCSP_basic_verify|, certificates in both |certs| and in |bs| are
// considered as certificates for the construction of the validation path for
// the signer certificate by default, unless |OCSP_NOCHAIN| is set.
// For |OCSP_request_verify|, certificates in |req| are considered as
// certificates for the construction of the validation path for the signer
// certificate by default, unless |OCSP_NOCHAIN| is set.
#define OCSP_NOCHAIN 0x8
// OCSP_NOVERIFY is for |OCSP_basic_verify| and |OCSP_request_verify|. When
// setting this flag, the signature on the OCSP response/request will still be
// verified, but additionally verification of the signer certificate will be
// skipped.
#define OCSP_NOVERIFY 0x10
// OCSP_NOEXPLICIT is for |OCSP_basic_verify|. We will check for explicit trust
// for OCSP signing in the root CA certificate, unless the flags contain
// |OCSP_NOEXPLICIT|.
#define OCSP_NOEXPLICIT 0x20
// OCSP_TRUSTOTHER is for |OCSP_basic_verify| and |OCSP_request_verify|. When
// set, all certificates within |certs| are implicitly trusted.
#define OCSP_TRUSTOTHER 0x200
// OCSP_RESPID_KEY is for |OCSP_basic_sign|. By default, the OCSP responder is
// identified by name and included in the response. Setting this changes the
// default identifier to be the hash of the issuer's public key instead.
#define OCSP_RESPID_KEY 0x400
// OCSP_NOTIME is for |OCSP_basic_sign|. Setting this excludes the default
// behavior of setting the |producedAt| time field in |resp| against the current
// time and leaves it empty.
#define OCSP_NOTIME 0x800


typedef struct ocsp_cert_id_st OCSP_CERTID;
typedef struct ocsp_one_request_st OCSP_ONEREQ;
typedef struct ocsp_req_info_st OCSP_REQINFO;
typedef struct ocsp_signature_st OCSP_SIGNATURE;
typedef struct ocsp_request_st OCSP_REQUEST;
typedef struct ocsp_resp_bytes_st OCSP_RESPBYTES;
typedef struct ocsp_revoked_info_st OCSP_REVOKEDINFO;
typedef struct ocsp_cert_status_st OCSP_CERTSTATUS;
typedef struct ocsp_single_response_st OCSP_SINGLERESP;
typedef struct ocsp_response_data_st OCSP_RESPDATA;
typedef struct ocsp_response_st OCSP_RESPONSE;
typedef struct ocsp_responder_id_st OCSP_RESPID;
typedef struct ocsp_basic_response_st OCSP_BASICRESP;

DEFINE_STACK_OF(OCSP_CERTID)
DEFINE_STACK_OF(OCSP_ONEREQ)
DEFINE_STACK_OF(OCSP_RESPID)
DEFINE_STACK_OF(OCSP_SINGLERESP)

DECLARE_ASN1_FUNCTIONS(OCSP_BASICRESP)
DECLARE_ASN1_FUNCTIONS(OCSP_RESPONSE)
DECLARE_ASN1_FUNCTIONS(OCSP_CERTID)
DECLARE_ASN1_FUNCTIONS(OCSP_REQUEST)
DECLARE_ASN1_FUNCTIONS(OCSP_SINGLERESP)
DECLARE_ASN1_FUNCTIONS(OCSP_ONEREQ)

// d2i_OCSP_REQUEST_bio parses a DER-encoded OCSP request from |bp|, converts it
// into an |OCSP_REQUEST|, and writes the result in |preq|.
OPENSSL_EXPORT OCSP_REQUEST *d2i_OCSP_REQUEST_bio(BIO *bp, OCSP_REQUEST **preq);

// d2i_OCSP_RESPONSE_bio parses a DER-encoded OCSP response from |bp|, converts
// it into an |OCSP_RESPONSE|, and writes the result in |presp|.
OPENSSL_EXPORT OCSP_RESPONSE *d2i_OCSP_RESPONSE_bio(BIO *bp,
                                                    OCSP_RESPONSE **presp);

// i2d_OCSP_RESPONSE_bio marshals |presp| as a DER-encoded OCSP response and
// writes the result to |bp|.
OPENSSL_EXPORT int i2d_OCSP_RESPONSE_bio(BIO *bp, OCSP_RESPONSE *presp);

// i2d_OCSP_REQUEST_bio marshals |preq| as a DER-encoded OCSP request and
// writes the result to |bp|.
OPENSSL_EXPORT int i2d_OCSP_REQUEST_bio(BIO *bp, OCSP_REQUEST *preq);

// OCSP_CERTID_dup allocates a new |OCSP_CERTID| and sets it equal to the state
// of |id|. It returns the new |OCSP_CERTID| or NULL on error.
OPENSSL_EXPORT OCSP_CERTID *OCSP_CERTID_dup(OCSP_CERTID *id);

// OCSP_sendreq_bio is a blocking OCSP request handler which is a special case
// of non-blocking I/O.
// |OCSP_sendreq_bio| combines |OCSP_sendreq_new| with as many calls of
// |OCSP_sendreq_nbio| as needed and then |OCSP_REQ_CTX_free|, with a response
// header maximum line length of 4k. It waits indefinitely on a response, if
// |BIO_should_retry| is true and the |BIO| persists.
//
// WARNING: This is retained only for compatibility. This does not support
// setting a timeout or adding your own HTTP headers.
// Use |OCSP_sendreq_nbio| and handle the timeout accordingly to the |BIO| type.
// You can also use |OCSP_REQ_CTX_add1_header| to add your own HTTP headers.
OPENSSL_EXPORT OCSP_RESPONSE *OCSP_sendreq_bio(BIO *b, const char *path,
                                               OCSP_REQUEST *req);

// OCSP_sendreq_new returns an |OCSP_REQ_CTX| structure using the responder io,
// the URL path, the |OCSP_REQUEST| req to be sent, and with a response header
// maximum line length of maxline. If maxline is zero or less, a default value
// of 4k is used. The |OCSP_REQUEST| req may be set to NULL and provided later
// if required.
OPENSSL_EXPORT OCSP_REQ_CTX *OCSP_sendreq_new(BIO *io, const char *path,
                                              OCSP_REQUEST *req, int maxline);

// OCSP_sendreq_nbio attempts to send the request prepared in |rctx| and to
// gather the response via HTTP, using the |BIO| io and path that were given
// when calling |OCSP_sendreq_new|.
OPENSSL_EXPORT int OCSP_sendreq_nbio(OCSP_RESPONSE **presp, OCSP_REQ_CTX *rctx);

// OCSP_REQ_CTX_new creates a new |OCSP_REQ_CTX|. |OCSP_REQ_CTX| is used to
// contain the information to send the OCSP request and gather the response
// over HTTP.
OPENSSL_EXPORT OCSP_REQ_CTX *OCSP_REQ_CTX_new(BIO *io, int maxline);

// OCSP_REQ_CTX_free frees the memory allocated by |OCSP_REQ_CTX|.
OPENSSL_EXPORT void OCSP_REQ_CTX_free(OCSP_REQ_CTX *rctx);

// OCSP_set_max_response_length sets the maximum response length for an OCSP
// request over HTTP to |len|. If a custom max response length is needed, this
// should be set before |OCSP_REQ_CTX| is sent out to retrieve the OCSP
// response.
OPENSSL_EXPORT void OCSP_set_max_response_length(OCSP_REQ_CTX *rctx,
                                                 unsigned long len);

// OCSP_REQ_CTX_http adds the HTTP request line to the context.
OPENSSL_EXPORT int OCSP_REQ_CTX_http(OCSP_REQ_CTX *rctx, const char *op,
                                     const char *path);

// OCSP_REQ_CTX_set1_req finalizes the HTTP request context. It is needed if
// an ASN.1-encoded request should be sent.
OPENSSL_EXPORT int OCSP_REQ_CTX_set1_req(OCSP_REQ_CTX *rctx, OCSP_REQUEST *req);

// OCSP_REQ_CTX_add1_header adds header name with value |value| to the
// context |rctx|. It can be called  more than once to add multiple header
// lines.
OPENSSL_EXPORT int OCSP_REQ_CTX_add1_header(OCSP_REQ_CTX *rctx,
                                            const char *name,
                                            const char *value);

// OCSP_REQ_CTX_i2d parses the ASN.1 contents of |rctx| into the der format.
int OCSP_REQ_CTX_i2d(OCSP_REQ_CTX *rctx, const ASN1_ITEM *it, ASN1_VALUE *val);

// OCSP_request_add0_id adds |cid| to |req|. Returns the new |OCSP_ONEREQ|
// pointer allocated on the stack within |req|. This is useful if we want to
// add extensions.
// WARNING: This allocates a new |OCSP_ONEREQ| and assigns the  pointer to |cid|
// to it. It then adds the newly allocated |OCSP_ONEREQ| to the stack within
// |req|. |req| now takes ownership of |cid|, and also maintains ownership of
// the pointer to |OCSP_ONEREQ|.
OPENSSL_EXPORT OCSP_ONEREQ *OCSP_request_add0_id(OCSP_REQUEST *req,
                                                 OCSP_CERTID *cid);

// OCSP_onereq_get0_id returns the certificate identifier
// associated with an OCSP request
OPENSSL_EXPORT OCSP_CERTID *OCSP_onereq_get0_id(OCSP_ONEREQ *one);

// OCSP_request_add1_nonce adds a nonce of value |val| and length |len| to
// |req|. If |val| is NULL, a random nonce is generated and used. If |len| is
// zero or negative, a default length of 16 bytes will be used.
// If |val| is non-NULL, |len| must equal the length of |val|. This is different
// from OpenSSL, which allows a default length for |len| to be used. Mis-usage
// of the default length could result in a read overflow, so we disallow it.
OPENSSL_EXPORT int OCSP_request_add1_nonce(OCSP_REQUEST *req,
                                           unsigned char *val, int len);

// OCSP_basic_add1_nonce is identical to |OCSP_request_add1_nonce|, but adds the
// nonce to |resp| instead (the response).
OPENSSL_EXPORT int OCSP_basic_add1_nonce(OCSP_BASICRESP *resp,
                                         unsigned char *val, int len);

// OCSP_check_nonce checks nonce existence and equality in |req| and |bs|. If
// there is parsing issue with |req| or |bs|, it will be determined that a
// nonce does not exist within |req| or |bs|.
//
// Return value reflects result:
//    OCSP_NONCE_EQUAL (1: nonces present and equal.)
//    OCSP_NONCE_BOTH_ABSENT (2: nonces both absent.)
//    OCSP_NONCE_RESPONSE_ONLY (3: nonce present in |bs| only.)
//    OCSP_NONCE_NOT_EQUAL (0: parameters are NULL or nonces are both present
//                             but not equal.)
//    OCSP_NONCE_REQUEST_ONLY (-1: nonce in |req| only.)
//
//  For most responders, clients can check "return > 0".
//  If an OCSP responder doesn't handle nonces, "return != 0" may be necessary.
//  "return == 0" will always be an error. The error can mean that NULL
//  parameter was passed into the function, or that the nonces are both present,
//  but aren't equal.
OPENSSL_EXPORT int OCSP_check_nonce(OCSP_REQUEST *req, OCSP_BASICRESP *bs);

// OCSP_copy_nonce copies the nonce value (if any) from |req| to |resp|. Returns
// 1 on success and 0 on failure. If the optional nonce value does not exist in
// |req|, we return 2 instead.
//
// Note: |OCSP_copy_nonce| allows for multiple OCSP nonces to exist and appends
// the new nonce to the end of the extension list. This causes issues with
// |OCSP_check_nonce|, since it looks for the first one in the list. The old
// nonce extension should be deleted prior to calling |OCSP_copy_nonce|.
OPENSSL_EXPORT int OCSP_copy_nonce(OCSP_BASICRESP *resp, OCSP_REQUEST *req);

// OCSP_request_set1_name sets |requestorName| from an |X509_NAME| structure.
OPENSSL_EXPORT int OCSP_request_set1_name(OCSP_REQUEST *req, X509_NAME *nm);

// OCSP_request_add1_cert adds a certificate to an |OCSP_REQUEST|.
OPENSSL_EXPORT int OCSP_request_add1_cert(OCSP_REQUEST *req, X509 *cert);

// OCSP_request_is_signed checks if the optional signature exists for |req|.
OPENSSL_EXPORT int OCSP_request_is_signed(OCSP_REQUEST *req);

// OCSP_request_onereq_count returns the number of |OCSP_ONEREQ|s in |req|.
OPENSSL_EXPORT int OCSP_request_onereq_count(OCSP_REQUEST *req);

// OCSP_request_onereq_get0 returns the |OCSP_ONEREQ| in |req| at index |i| or
// NULL if |i| is out of bounds.
OPENSSL_EXPORT OCSP_ONEREQ *OCSP_request_onereq_get0(OCSP_REQUEST *req, int i);

// OCSP_request_sign signs the OCSP request |req| using |key| and |dgst|. |key|
// MUST be the private key of |signer|. One or more optional certificates can be
// added to |resp| with |certs|. This function will fail if a signature in |req|
// already exists.
//
// Note: 1. The OCSP requester is identified by the subject name from |signer|
//          and included in |req|.
//       2. All certificates in |certs| are added to |req| by default. Setting
//          |OCSP_NOCERTS| excludes certificates from being added in |req| and
//          ignores the |certs| parameter.
OPENSSL_EXPORT int OCSP_request_sign(OCSP_REQUEST *req, X509 *signer,
                                     EVP_PKEY *key, const EVP_MD *dgst,
                                     STACK_OF(X509) *certs,
                                     unsigned long flags);

// OCSP_response_status returns response status from |OCSP_RESPONSE|.
OPENSSL_EXPORT int OCSP_response_status(OCSP_RESPONSE *resp);

// OCSP_response_get1_basic returns |OCSP_BASICRESP| from |OCSP_RESPONSE|.
OPENSSL_EXPORT OCSP_BASICRESP *OCSP_response_get1_basic(OCSP_RESPONSE *resp);

// OCSP_resp_count returns the number of |OCSP_SINGLERESP| responses present
// in |bs|.
OPENSSL_EXPORT int OCSP_resp_count(OCSP_BASICRESP *bs);

// OCSP_resp_get0 returns the |OCSP_SINGLERESP| at the |idx| within |bs|.
OPENSSL_EXPORT OCSP_SINGLERESP *OCSP_resp_get0(OCSP_BASICRESP *bs, size_t idx);

// OCSP_single_get0_status returns the status of |single|.
//
// Note: 1. |reason| value is allowed to be null.
//       2. Time values passed into function are allowed to be NULL if
//          certificate fields are empty.
//       3. |revtime| and |reason| values only set if the certificate status is
//          revoked.
OPENSSL_EXPORT int OCSP_single_get0_status(OCSP_SINGLERESP *single, int *reason,
                                           ASN1_GENERALIZEDTIME **revtime,
                                           ASN1_GENERALIZEDTIME **thisupd,
                                           ASN1_GENERALIZEDTIME **nextupd);

// OCSP_resp_find returns the index of the |OCSP_SINGLERESP| in |bs| which
// matches |id| if found, or -1 if not found.
OPENSSL_EXPORT int OCSP_resp_find(OCSP_BASICRESP *bs, OCSP_CERTID *id,
                                  int last);

// OCSP_resp_find_status looks up a cert id and extract the update time and
// revocation status of  certificate sent back from OCSP responder if found.
// Returns 1 on success.
//
// Note: 1. Revocation status code is passed into |*status| parameter. Status
//          code will not be passed if |*status| is NULL.
OPENSSL_EXPORT int OCSP_resp_find_status(OCSP_BASICRESP *bs, OCSP_CERTID *id,
                                         int *status, int *reason,
                                         ASN1_GENERALIZEDTIME **revtime,
                                         ASN1_GENERALIZEDTIME **thisupd,
                                         ASN1_GENERALIZEDTIME **nextupd);

// OCSP_check_validity checks the validity of |thisUpdate| and |nextUpdate|
// fields from an |OCSP_SINGLERESP|.
//
// Note: 1. It is possible that the request will take a few seconds to process
//          and/or the local system time isn't exactly the same as the OCSP
//          responder's time. Therefore, to avoid rejecting otherwise valid time
//          we allow the times to be within |drift_num_seconds| of the current
//          time.
//      2.  Also, to avoid accepting very old responses without a
//          |nextUpdate| field, an optional |max_age_seconds| parameter
//          specifies the maximum age the |thisUpdate| field can be.
//          |max_age_seconds| should be the number of seconds relative to
//          |thisUpdate|. You can also set |max_age_seconds| to "-1", if the
//          maximum age should not be checked.
//      3.  |thisUpdate| should be within the range of: (current time -
//          max_age_seconds) < |thisUpdate| < (current time +
//          drift_num_seconds).
//          |nextUpdate| should be in the future: (current time +
//          drift_num_seconds) < |nextUpdate|.
//      4.  |thisUpdate| and |nextUpdate| are defined in the RFC:
//          https://datatracker.ietf.org/doc/html/rfc6960#section-2.4
OPENSSL_EXPORT int OCSP_check_validity(ASN1_GENERALIZEDTIME *thisUpdate,
                                       ASN1_GENERALIZEDTIME *nextUpdate,
                                       long drift_num_seconds,
                                       long max_age_seconds);

// OCSP_basic_verify verifies a basic response message. It checks that |bs| is
// correctly signed and that the signer certificate can be validated.
// Returns 1 if the response is valid, 0 if the signature cannot be verified,
// or -1 on fatal errors such as malloc failure.
//
// Note: 1. Checks that OCSP response CAN be verified, but does not imply
//          anything about the corresponding certificate's revocation status.
//       2. |OCSP_resp_find_status| should be used to check if the OCSP
//          response's cert status is |V_OCSP_CERTSTATUS_GOOD|.
//          |OCSP_check_validity| should also be used to validate that the OCSP
//          response's timestamps are correct.
OPENSSL_EXPORT int OCSP_basic_verify(OCSP_BASICRESP *bs, STACK_OF(X509) *certs,
                                     X509_STORE *st, unsigned long flags);

// OCSP_request_verify verifies the OCSP request message, |req|, with |st|.
// OCSP request signatures are optional according to RFC6960, but one can check
// that |req| is correctly signed and that the signer certificate can be
// validated if a signature exists. This returns 1 if |req| is valid or returns
// 0 if |req|'s signature is non-existent or cannot be verified.
OPENSSL_EXPORT int OCSP_request_verify(OCSP_REQUEST *req, STACK_OF(X509) *certs,
                                       X509_STORE *st, unsigned long flags);

// OCSP_cert_id_new creates and returns a new |OCSP_CERTID| using |dgst|,
// |issuerName|, |issuerKey|, and |serialNumber| as its contents.
OPENSSL_EXPORT OCSP_CERTID *OCSP_cert_id_new(const EVP_MD *dgst,
                                             const X509_NAME *issuerName,
                                             const ASN1_BIT_STRING *issuerKey,
                                             const ASN1_INTEGER *serialNumber);

// OCSP_cert_to_id returns a |OCSP_CERTID| converted from a certificate and its
// issuer.
//
// Note: 1. If |subject| is NULL, we get the subject name from the issuer and
//          set the serial number to NULL.
//       2. OpenSSL's legacy OCSP code decided to make SHA-1 as default hash
//          algorithm when the |dgst| is set as NULL. We keep this to maintain
//          backwards compatibility, but strongly advise to set a digest when
//          using this function. Even though this is not used cryptographically,
//          there is the possibility of a response being returned with a forced
//          issuer name when using SHA-1 (assuming a preimage attack, which is
//          beyond the scope of how SHA-1 is currently vulnerable).
OPENSSL_EXPORT OCSP_CERTID *OCSP_cert_to_id(const EVP_MD *dgst,
                                            const X509 *subject,
                                            const X509 *issuer);

// OCSP_parse_url parses an OCSP responder URL and returns its component parts.
// |url| argument must be a null-terminated string containing the URL to be
// parsed. The other arguments are pointers to variables that will be set to the
// parsed components of the URL. When |OCSP_parse_url| returns 1, these
// arguments will allocate new memory with a copy of value. It is the caller's
// responsibility to free these.
//
//  |phost|: A pointer to a char pointer that will be set to the host component
//           of the URL. If the URL does not contain a host component, this will
//           be set to an empty string.
//  |pport|: A pointer to an int that will be set to the port number specified
//           in the URL, or to the default port (80 for HTTP, 443 for HTTPS)
//           if no port number is specified.
//  |ppath|: A pointer to a char pointer that will be set to the path component
//           of the URL. If the URL does not contain a path component, this
//           will be set to "/".
//  |pssl|:  A pointer to an int that will be set to 1 if the URL specifies the
//           HTTPS protocol, or 0 if HTTP.
//
// Note: |OCSP_parse_url| does not perform any validation of the URL or its
//        components beyond basic parsing. It is the responsibility of the
//        caller to ensure that the URL is well-formed and valid.
OPENSSL_EXPORT int OCSP_parse_url(const char *url, char **phost, char **pport,
                                  char **ppath, int *pssl);

// OCSP_id_issuer_cmp compares the issuers' name and key hash of |a| and |b|. It
// returns 0 on equal.
OPENSSL_EXPORT int OCSP_id_issuer_cmp(const OCSP_CERTID *a,
                                      const OCSP_CERTID *b);

// OCSP_id_cmp calls |OCSP_id_issuer_cmp| and additionally compares the
// |serialNumber| of |a| and |b|. It returns 0 on equal.
OPENSSL_EXPORT int OCSP_id_cmp(const OCSP_CERTID *a, const OCSP_CERTID *b);

// OCSP_id_get0_info returns the issuer name hash, hash OID, issuer key hash,
// and the serial number contained in |cid|. If any of the values are not
// required, the corresponding parameter can be set to NULL.
OPENSSL_EXPORT int OCSP_id_get0_info(ASN1_OCTET_STRING **nameHash,
                                     ASN1_OBJECT **algor,
                                     ASN1_OCTET_STRING **keyHash,
                                     ASN1_INTEGER **serial, OCSP_CERTID *cid);

// OCSP_basic_add1_cert adds |cert| to the |resp|.
OPENSSL_EXPORT int OCSP_basic_add1_cert(OCSP_BASICRESP *resp, X509 *cert);

// OCSP_basic_add1_status creates and returns an |OCSP_SINGLERESP| with |cid|,
// |status|, |this_update| and |next_update|. The newly created
// |OCSP_SINGLERESP| is pushed onto the internal |OCSP_SINGLERESP| stack in
// |resp|. |status| should be a value defined by |V_OCSP_CERTSTATUS_*|.
//
// 1. If |status| has the value |V_OCSP_CERTSTATUS_REVOKED|, |revoked_reason|
// should be a valid |OCSP_REVOKED_STATUS_*| value and |revoked_time| cannot be
// empty.
// 2. If |status| has the value of either |V_OCSP_CERTSTATUS_GOOD| or
// |V_OCSP_CERTSTATUS_UNKNOWN|, |revoked_reason| and |revoked_time| are ignored.
OPENSSL_EXPORT OCSP_SINGLERESP *OCSP_basic_add1_status(
    OCSP_BASICRESP *resp, OCSP_CERTID *cid, int status, int revoked_reason,
    ASN1_TIME *revoked_time, ASN1_TIME *this_update, ASN1_TIME *next_update);

// OCSP_basic_sign signs the OCSP response |resp| using |key| and |dgst|. |key|
// MUST be the private key of |signer|. One or more optional certificates can be
// added to |resp| with |certs|.
//
// Note: 1. By default, the OCSP responder is identified by the subject name
//          from |signer| and included in |resp|. Users can set
//          |OCSP_RESPID_KEY| with |flags|, if they wish for the responder to
//          be identified by the hash of |signer|'s public key instead.
//       2. All certificates in |certs| are added to |resp| by default. Setting
//          |OCSP_NOCERTS| excludes certificates from being added in |resp| and
//          ignores the |certs| parameter.
//       3. The |producedAt| time field is set to the current time by default.
//          Setting |OCSP_NOTIME| excludes setting the |producedAt| time field
//          in |resp| and leaves it empty.
OPENSSL_EXPORT int OCSP_basic_sign(OCSP_BASICRESP *resp, X509 *signer,
                                   EVP_PKEY *key, const EVP_MD *dgst,
                                   STACK_OF(X509) *certs, unsigned long flags);

// OCSP_response_create creates an |OCSP_RESPONSE| and encodes an optional |bs|
// within it.
OPENSSL_EXPORT OCSP_RESPONSE *OCSP_response_create(int status,
                                                   OCSP_BASICRESP *bs);

// OCSP_SINGLERESP_get0_id returns the |OCSP_CERTID| within |x|.
OPENSSL_EXPORT const OCSP_CERTID *OCSP_SINGLERESP_get0_id(
    const OCSP_SINGLERESP *x);

// OCSP_response_status_str returns the OCSP response status of |status_code| as
// a string.
OPENSSL_EXPORT const char *OCSP_response_status_str(long status_code);

// OCSP_cert_status_str returns the OCSP cert status of |status_code| as
// a string.
OPENSSL_EXPORT const char *OCSP_cert_status_str(long status_code);

// OCSP_crl_reason_str returns the OCSP CRL reason of |status_code| as a string.
// |OCSP_resp_find_status| can be used to retrieve the reason status code
// if an OCSP response is revoked.
OPENSSL_EXPORT const char *OCSP_crl_reason_str(long status_code);

// OCSP_REQUEST_print prints the contents of an OCSP request to |bp|. |flags| is
// used to configure printing of the |req|'s extensions (See
// |X509V3_extensions_print| for more information).
// This is typically used for debugging or diagnostic purposes.
OPENSSL_EXPORT int OCSP_REQUEST_print(BIO *bp, OCSP_REQUEST *req,
                                      unsigned long flags);

// OCSP_RESPONSE_print prints the contents of an OCSP response to |bp|. |flags|
// is used to configure printing of the |resp|'s extensions (See
// |X509V3_extensions_print| for more information).
// This is typically used for debugging or diagnostic purposes.
OPENSSL_EXPORT int OCSP_RESPONSE_print(BIO *bp, OCSP_RESPONSE *resp,
                                       unsigned long flags);

// OCSP_BASICRESP_get_ext_by_NID returns the index of an extension |bs| by its
// NID. Returns -1 if not found.
OPENSSL_EXPORT int OCSP_BASICRESP_get_ext_by_NID(OCSP_BASICRESP *bs, int nid,
                                                 int lastpos);

// OCSP_BASICRESP_get_ext returns the |X509_EXTENSION| in |bs| at index |loc|,
// or NULL if |loc| is out of bounds.
OPENSSL_EXPORT X509_EXTENSION *OCSP_BASICRESP_get_ext(OCSP_BASICRESP *bs,
                                                      int loc);


// OCSP |X509_EXTENSION| Functions

// OCSP_BASICRESP_delete_ext removes the extension in |x| at index |loc| and
// returns the removed extension, or NULL if |loc| was out of bounds. If an
// extension was returned, the caller must release it with
// |X509_EXTENSION_free|.
OPENSSL_EXPORT X509_EXTENSION *OCSP_BASICRESP_delete_ext(OCSP_BASICRESP *x,
                                                         int loc);

// OCSP_SINGLERESP_add_ext adds a copy of |ex| to the extension list in
// |*sresp|. It returns 1 on success and 0 on error. The new extension is
// inserted at index |loc|, shifting extensions to the right. If |loc| is -1 or
// out of bounds, the new extension is appended to the list.
OPENSSL_EXPORT int OCSP_SINGLERESP_add_ext(OCSP_SINGLERESP *sresp,
                                           X509_EXTENSION *ex, int loc);

// OCSP_SINGLERESP_get_ext_count returns the number of |X509_EXTENSION|s in
// |sresp|.
OPENSSL_EXPORT int OCSP_SINGLERESP_get_ext_count(OCSP_SINGLERESP *sresp);

// OCSP_SINGLERESP_get_ext returns the |X509_EXTENSION| in |sresp|
// at index |loc|, or NULL if |loc| is out of bounds.
OPENSSL_EXPORT X509_EXTENSION *OCSP_SINGLERESP_get_ext(OCSP_SINGLERESP *sresp,
                                                       int loc);


// OCSP no-op flags [Deprecated].

// OCSP_NOSIGS does nothing. In OpenSSL, this skips signature verification in
// |OCSP_basic_verify| and |OCSP_request_verify|.
#define OCSP_NOSIGS 0

// OCSP_NOCASIGN does nothing. It's a legacy OCSP flag deprecated since OpenSSL
// 1.0.1g.
#define OCSP_NOCASIGN 0

// OCSP_NODELEGATED does nothing. It's a legacy OCSP flag deprecated since
// OpenSSL 1.0.1g.
#define OCSP_NODELEGATED 0

// OCSP_NOCHECKS does nothing. In OpenSSL, this disables verifying that the
// signer certificate has met the OCSP issuer criteria or any potential
// delegation in |OCSP_basic_verify|.
#define OCSP_NOCHECKS 0


#if defined(__cplusplus)
}  // extern C
#endif

#if !defined(BORINGSSL_NO_CXX)
extern "C++" {

BSSL_NAMESPACE_BEGIN

BORINGSSL_MAKE_DELETER(OCSP_REQUEST, OCSP_REQUEST_free)
BORINGSSL_MAKE_DELETER(OCSP_REQ_CTX, OCSP_REQ_CTX_free)
BORINGSSL_MAKE_DELETER(OCSP_RESPONSE, OCSP_RESPONSE_free)
BORINGSSL_MAKE_DELETER(OCSP_BASICRESP, OCSP_BASICRESP_free)
BORINGSSL_MAKE_DELETER(OCSP_CERTID, OCSP_CERTID_free)
BORINGSSL_MAKE_DELETER(OCSP_SINGLERESP, OCSP_SINGLERESP_free)

BSSL_NAMESPACE_END

}  // extern C++
#endif  // !BORINGSSL_NO_CXX

#define OCSP_RESPONSE_STATUS_SUCCESSFUL 0
#define OCSP_RESPONSE_STATUS_MALFORMEDREQUEST 1
#define OCSP_RESPONSE_STATUS_INTERNALERROR 2
#define OCSP_RESPONSE_STATUS_TRYLATER 3
#define OCSP_RESPONSE_STATUS_SIGREQUIRED 5
#define OCSP_RESPONSE_STATUS_UNAUTHORIZED 6

#define V_OCSP_RESPID_NAME 0
#define V_OCSP_RESPID_KEY 1

#define V_OCSP_CERTSTATUS_GOOD 0
#define V_OCSP_CERTSTATUS_REVOKED 1
#define V_OCSP_CERTSTATUS_UNKNOWN 2

#define OCSP_NONCE_EQUAL 1
#define OCSP_NONCE_BOTH_ABSENT 2
#define OCSP_NONCE_RESPONSE_ONLY 3
#define OCSP_NONCE_NOT_EQUAL 0
#define OCSP_NONCE_REQUEST_ONLY -1

#define OCSP_R_CERTIFICATE_VERIFY_ERROR 101
#define OCSP_R_DIGEST_ERR 102
#define OCSP_R_MISSING_OCSPSIGNING_USAGE 103
#define OCSP_R_NOT_BASIC_RESPONSE 104
#define OCSP_R_NO_CERTIFICATES_IN_CHAIN 105
#define OCSP_R_NO_RESPONSE_DATA 108
#define OCSP_R_NO_REVOKED_TIME 109
#define OCSP_R_PRIVATE_KEY_DOES_NOT_MATCH_CERTIFICATE 110
#define OCSP_R_RESPONSE_CONTAINS_NO_REVOCATION_DATA 111
#define OCSP_R_ROOT_CA_NOT_TRUSTED 112
#define OCSP_R_SERVER_RESPONSE_PARSE_ERROR 115
#define OCSP_R_SIGNATURE_FAILURE 117
#define OCSP_R_SIGNER_CERTIFICATE_NOT_FOUND 118
#define OCSP_R_UNKNOWN_MESSAGE_DIGEST 119
#define OCSP_R_UNKNOWN_NID 120
#define OCSP_R_ERROR_PARSING_URL 121
#define OCSP_R_ERROR_IN_NEXTUPDATE_FIELD 122
#define OCSP_R_ERROR_IN_THISUPDATE_FIELD 123
#define OCSP_R_NEXTUPDATE_BEFORE_THISUPDATE 124
#define OCSP_R_STATUS_EXPIRED 125
#define OCSP_R_STATUS_NOT_YET_VALID 126
#define OCSP_R_STATUS_TOO_OLD 127
#define OCSP_R_REQUEST_NOT_SIGNED 128
#define OCSP_R_UNSUPPORTED_REQUESTORNAME_TYPE 129
#define OCSP_R_NO_SIGNER_KEY 130
#define OCSP_R_OCSP_REQUEST_DUPLICATE_SIGNATURE 131
#define OCSP_R_UNKNOWN_FIELD_VALUE 132

#endif  // AWSLC_OCSP_H
