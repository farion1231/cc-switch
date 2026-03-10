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
 * Copyright (c) 1998-2007 The OpenSSL Project.  All rights reserved.
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
 *    for use in the OpenSSL Toolkit. (http://www.openssl.org/)"
 *
 * 4. The names "OpenSSL Toolkit" and "OpenSSL Project" must not be used to
 *    endorse or promote products derived from this software without
 *    prior written permission. For written permission, please contact
 *    openssl-core@openssl.org.
 *
 * 5. Products derived from this software may not be called "OpenSSL"
 *    nor may "OpenSSL" appear in their names without prior written
 *    permission of the OpenSSL Project.
 *
 * 6. Redistributions of any form whatsoever must retain the following
 *    acknowledgment:
 *    "This product includes software developed by the OpenSSL Project
 *    for use in the OpenSSL Toolkit (http://www.openssl.org/)"
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
 * Hudson (tjh@cryptsoft.com).
 *
 */
/* ====================================================================
 * Copyright 2002 Sun Microsystems, Inc. ALL RIGHTS RESERVED.
 * ECC cipher suite support in OpenSSL originally developed by
 * SUN MICROSYSTEMS, INC., and contributed to the OpenSSL project.
 */
/* ====================================================================
 * Copyright 2005 Nokia. All rights reserved.
 *
 * The portions of the attached software ("Contribution") is developed by
 * Nokia Corporation and is licensed pursuant to the OpenSSL open source
 * license.
 *
 * The Contribution, originally written by Mika Kousa and Pasi Eronen of
 * Nokia Corporation, consists of the "PSK" (Pre-Shared Key) ciphersuites
 * support (see RFC 4279) to OpenSSL.
 *
 * No patent licenses or other rights except those expressly stated in
 * the OpenSSL open source license shall be deemed granted or received
 * expressly, by implication, estoppel, or otherwise.
 *
 * No assurances are provided by Nokia that the Contribution does not
 * infringe the patent or other intellectual property rights of any third
 * party or that the license provides you with all the necessary rights
 * to make use of the Contribution.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" WITHOUT WARRANTY OF ANY KIND. IN
 * ADDITION TO THE DISCLAIMERS INCLUDED IN THE LICENSE, NOKIA
 * SPECIFICALLY DISCLAIMS ANY LIABILITY FOR CLAIMS BROUGHT BY YOU OR ANY
 * OTHER ENTITY BASED ON INFRINGEMENT OF INTELLECTUAL PROPERTY RIGHTS OR
 * OTHERWISE.
 */

#ifndef OPENSSL_HEADER_SSL_H
#define OPENSSL_HEADER_SSL_H

#include <openssl/base.h>

#include <openssl/bio.h>
#include <openssl/buf.h>
#include <openssl/pem.h>
#include <openssl/span.h>
#include <openssl/ssl3.h>
#include <openssl/thread.h>
#include <openssl/tls1.h>
#include <openssl/x509.h>

#if !defined(OPENSSL_WINDOWS)
#include <sys/time.h>
#endif

// Forward-declare struct timeval. On Windows, it is defined in winsock2.h and
// Windows headers define too many macros to be included in public headers.
// However, only a forward declaration is needed.
struct timeval;

#if defined(__cplusplus)
extern "C" {
#endif


// SSL implementation.


// SSL contexts.
//
// |SSL_CTX| objects manage shared state and configuration between multiple TLS
// or DTLS connections. Whether the connections are TLS or DTLS is selected by
// an |SSL_METHOD| on creation.
//
// |SSL_CTX| are reference-counted and may be shared by connections across
// multiple threads. Once shared, functions which change the |SSL_CTX|'s
// configuration may not be used.

// TLS_method is the |SSL_METHOD| used for TLS connections.
OPENSSL_EXPORT const SSL_METHOD *TLS_method(void);

// DTLS_method is the |SSL_METHOD| used for DTLS connections.
OPENSSL_EXPORT const SSL_METHOD *DTLS_method(void);

// TLS_with_buffers_method is like |TLS_method|, but avoids all use of
// crypto/x509. All client connections created with |TLS_with_buffers_method|
// will fail unless a certificate verifier is installed with
// |SSL_set_custom_verify| or |SSL_CTX_set_custom_verify|.
OPENSSL_EXPORT const SSL_METHOD *TLS_with_buffers_method(void);

// DTLS_with_buffers_method is like |DTLS_method|, but avoids all use of
// crypto/x509.
OPENSSL_EXPORT const SSL_METHOD *DTLS_with_buffers_method(void);

// SSL_CTX_new returns a newly-allocated |SSL_CTX| with default settings or NULL
// on error.
OPENSSL_EXPORT SSL_CTX *SSL_CTX_new(const SSL_METHOD *method);

// SSL_CTX_up_ref increments the reference count of |ctx|. It returns one.
OPENSSL_EXPORT int SSL_CTX_up_ref(SSL_CTX *ctx);

// SSL_CTX_free releases memory associated with |ctx|.
OPENSSL_EXPORT void SSL_CTX_free(SSL_CTX *ctx);


// SSL connections.
//
// An |SSL| object represents a single TLS or DTLS connection. Although the
// shared |SSL_CTX| is thread-safe, an |SSL| is not thread-safe and may only be
// used on one thread at a time.

// SSL_new returns a newly-allocated |SSL| using |ctx| or NULL on error. The new
// connection inherits settings from |ctx| at the time of creation. Settings may
// also be individually configured on the connection.
//
// On creation, an |SSL| is not configured to be either a client or server. Call
// |SSL_set_connect_state| or |SSL_set_accept_state| to set this.
OPENSSL_EXPORT SSL *SSL_new(SSL_CTX *ctx);

// SSL_free releases memory associated with |ssl|.
OPENSSL_EXPORT void SSL_free(SSL *ssl);

// SSL_get_SSL_CTX returns the |SSL_CTX| associated with |ssl|. If
// |SSL_set_SSL_CTX| is called, it returns the new |SSL_CTX|, not the initial
// one.
OPENSSL_EXPORT SSL_CTX *SSL_get_SSL_CTX(const SSL *ssl);

// SSL_set_connect_state configures |ssl| to be a client.
OPENSSL_EXPORT void SSL_set_connect_state(SSL *ssl);

// SSL_set_accept_state configures |ssl| to be a server.
OPENSSL_EXPORT void SSL_set_accept_state(SSL *ssl);

// SSL_is_server returns one if |ssl| is configured as a server and zero
// otherwise.
OPENSSL_EXPORT int SSL_is_server(const SSL *ssl);

// SSL_is_dtls returns one if |ssl| is a DTLS connection and zero otherwise.
OPENSSL_EXPORT int SSL_is_dtls(const SSL *ssl);

// SSL_set_bio configures |ssl| to read from |rbio| and write to |wbio|. |ssl|
// takes ownership of the two |BIO|s. If |rbio| and |wbio| are the same, |ssl|
// only takes ownership of one reference.
//
// In DTLS, |rbio| must be non-blocking to properly handle timeouts and
// retransmits.
//
// If |rbio| is the same as the currently configured |BIO| for reading, that
// side is left untouched and is not freed.
//
// If |wbio| is the same as the currently configured |BIO| for writing AND |ssl|
// is not currently configured to read from and write to the same |BIO|, that
// side is left untouched and is not freed. This asymmetry is present for
// historical reasons.
//
// Due to the very complex historical behavior of this function, calling this
// function if |ssl| already has |BIO|s configured is deprecated. Prefer
// |SSL_set0_rbio| and |SSL_set0_wbio| instead.
OPENSSL_EXPORT void SSL_set_bio(SSL *ssl, BIO *rbio, BIO *wbio);

// SSL_set0_rbio configures |ssl| to read from |rbio|. It takes ownership of
// |rbio|.
//
// Note that, although this function and |SSL_set0_wbio| may be called on the
// same |BIO|, each call takes a reference. Use |BIO_up_ref| to balance this.
OPENSSL_EXPORT void SSL_set0_rbio(SSL *ssl, BIO *rbio);

// SSL_set0_wbio configures |ssl| to write to |wbio|. It takes ownership of
// |wbio|.
//
// Note that, although this function and |SSL_set0_rbio| may be called on the
// same |BIO|, each call takes a reference. Use |BIO_up_ref| to balance this.
OPENSSL_EXPORT void SSL_set0_wbio(SSL *ssl, BIO *wbio);

// SSL_get_rbio returns the |BIO| that |ssl| reads from.
OPENSSL_EXPORT BIO *SSL_get_rbio(const SSL *ssl);

// SSL_get_wbio returns the |BIO| that |ssl| writes to.
OPENSSL_EXPORT BIO *SSL_get_wbio(const SSL *ssl);

// SSL_get_fd calls |SSL_get_rfd|.
OPENSSL_EXPORT int SSL_get_fd(const SSL *ssl);

// SSL_get_rfd returns the file descriptor that |ssl| is configured to read
// from. If |ssl|'s read |BIO| is not configured or doesn't wrap a file
// descriptor then it returns -1.
//
// Note: On Windows, this may return either a file descriptor or a socket (cast
// to int), depending on whether |ssl| was configured with a file descriptor or
// socket |BIO|.
OPENSSL_EXPORT int SSL_get_rfd(const SSL *ssl);

// SSL_get_wfd returns the file descriptor that |ssl| is configured to write
// to. If |ssl|'s write |BIO| is not configured or doesn't wrap a file
// descriptor then it returns -1.
//
// Note: On Windows, this may return either a file descriptor or a socket (cast
// to int), depending on whether |ssl| was configured with a file descriptor or
// socket |BIO|.
OPENSSL_EXPORT int SSL_get_wfd(const SSL *ssl);

#if !defined(OPENSSL_NO_SOCK)
// SSL_set_fd configures |ssl| to read from and write to |fd|. It returns one
// on success and zero on allocation error. The caller retains ownership of
// |fd|.
//
// On Windows, |fd| is cast to a |SOCKET| and used with Winsock APIs.
OPENSSL_EXPORT int SSL_set_fd(SSL *ssl, int fd);

// SSL_set_rfd configures |ssl| to read from |fd|. It returns one on success and
// zero on allocation error. The caller retains ownership of |fd|.
//
// On Windows, |fd| is cast to a |SOCKET| and used with Winsock APIs.
OPENSSL_EXPORT int SSL_set_rfd(SSL *ssl, int fd);

// SSL_set_wfd configures |ssl| to write to |fd|. It returns one on success and
// zero on allocation error. The caller retains ownership of |fd|.
//
// On Windows, |fd| is cast to a |SOCKET| and used with Winsock APIs.
OPENSSL_EXPORT int SSL_set_wfd(SSL *ssl, int fd);
#endif  // !OPENSSL_NO_SOCK

// SSL_do_handshake continues the current handshake. If there is none or the
// handshake has completed or False Started, it returns one. Otherwise, it
// returns <= 0. The caller should pass the value into |SSL_get_error| to
// determine how to proceed.
//
// In DTLS, the caller must drive retransmissions. Whenever |SSL_get_error|
// signals |SSL_ERROR_WANT_READ|, use |DTLSv1_get_timeout| to determine the
// current timeout. If it expires before the next retry, call
// |DTLSv1_handle_timeout|. Note that DTLS handshake retransmissions use fresh
// sequence numbers, so it is not sufficient to replay packets at the transport.
//
// TODO(davidben): Ensure 0 is only returned on transport EOF.
// https://crbug.com/466303.
OPENSSL_EXPORT int SSL_do_handshake(SSL *ssl);

// SSL_connect configures |ssl| as a client, if unconfigured, and calls
// |SSL_do_handshake|.
OPENSSL_EXPORT int SSL_connect(SSL *ssl);

// SSL_accept configures |ssl| as a server, if unconfigured, and calls
// |SSL_do_handshake|.
OPENSSL_EXPORT int SSL_accept(SSL *ssl);

// SSL_read reads up to |num| bytes from |ssl| into |buf|. It implicitly runs
// any pending handshakes, including renegotiations when enabled. On success, it
// returns the number of bytes read. Otherwise, it returns <= 0. The caller
// should pass the value into |SSL_get_error| to determine how to proceed.
//
// TODO(davidben): Ensure 0 is only returned on transport EOF.
// https://crbug.com/466303.
OPENSSL_EXPORT int SSL_read(SSL *ssl, void *buf, int num);

// SSL_read_ex reads up to |num| bytes from |ssl| into |buf|. It is similar to
// |SSL_read|, but instead of returning the number of bytes read, it returns
// 1 on success or 0 for failure. The number of bytes actually read is stored in
// |read_bytes|.
//
// This is only maintained for OpenSSL compatibility. Use |SSL_read| instead.
OPENSSL_EXPORT int SSL_read_ex(SSL *ssl, void *buf, size_t num,
                               size_t *read_bytes);

// SSL_peek behaves like |SSL_read| but does not consume any bytes returned.
OPENSSL_EXPORT int SSL_peek(SSL *ssl, void *buf, int num);

// SSL_peek_ex reads up to |num| bytes from |ssl| into |buf|. It is similar to
// |SSL_peek|, but instead of returning the number of bytes read, it returns
// 1 on success or 0 for failure. The number of bytes actually read is stored in
// |read_bytes|.
//
// This is only maintained for OpenSSL compatibility. Use |SSL_peek| instead.
OPENSSL_EXPORT int SSL_peek_ex(SSL *ssl, void *buf, size_t num,
                               size_t *read_bytes);

// SSL_pending returns the number of buffered, decrypted bytes available for
// read in |ssl|. It does not read from the transport.
//
// In DTLS, it is possible for this function to return zero while there is
// buffered, undecrypted data from the transport in |ssl|. For example,
// |SSL_read| may read a datagram with two records, decrypt the first, and leave
// the second buffered for a subsequent call to |SSL_read|. Callers that wish to
// detect this case can use |SSL_has_pending|.
OPENSSL_EXPORT int SSL_pending(const SSL *ssl);

// SSL_has_pending returns one if |ssl| has buffered, decrypted bytes available
// for read, or if |ssl| has buffered data from the transport that has not yet
// been decrypted. If |ssl| has neither, this function returns zero.
//
// If read-ahead has been enabled with |SSL_CTX_set_read_ahead| or
// |SSL_set_read_ahead|, the behavior of |SSL_pending| will change, it may return
// 1 and a call to |SSL_read| to return no data. This can happen when a partial
// record has been read but can not be decrypted without more data from the read
// BIO.
//
// In DTLS, it is possible for this function to return one while |SSL_pending|
// returns zero. For example, |SSL_read| may read a datagram with two records,
// decrypt the first, and leave the second buffered for a subsequent call to
// |SSL_read|.
//
// As a result, if this function returns one, the next call to |SSL_read| may
// still fail, read from the transport, or both. The buffered, undecrypted data
// may be invalid or incomplete.
OPENSSL_EXPORT int SSL_has_pending(const SSL *ssl);

// SSL_write writes up to |num| bytes from |buf| into |ssl|. It implicitly runs
// any pending handshakes, including renegotiations when enabled. On success, it
// returns the number of bytes written. Otherwise, it returns <= 0. The caller
// should pass the value into |SSL_get_error| to determine how to proceed.
//
// In TLS, a non-blocking |SSL_write| differs from non-blocking |write| in that
// a failed |SSL_write| still commits to the data passed in. When retrying, the
// caller must supply the original write buffer (or a larger one containing the
// original as a prefix). By default, retries will fail if they also do not
// reuse the same |buf| pointer. This may be relaxed with
// |SSL_MODE_ACCEPT_MOVING_WRITE_BUFFER|, but the buffer contents still must be
// unchanged.
//
// By default, in TLS, |SSL_write| will not return success until all |num| bytes
// are written. This may be relaxed with |SSL_MODE_ENABLE_PARTIAL_WRITE|. It
// allows |SSL_write| to complete with a partial result when only part of the
// input was written in a single record.
//
// In DTLS, neither |SSL_MODE_ACCEPT_MOVING_WRITE_BUFFER| and
// |SSL_MODE_ENABLE_PARTIAL_WRITE| do anything. The caller may retry with a
// different buffer freely. A single call to |SSL_write| only ever writes a
// single record in a single packet, so |num| must be at most
// |SSL3_RT_MAX_PLAIN_LENGTH|.
//
// TODO(davidben): Ensure 0 is only returned on transport EOF.
// https://crbug.com/466303.
OPENSSL_EXPORT int SSL_write(SSL *ssl, const void *buf, int num);

// SSL_write_ex writes up to |num| bytes from |buf| into |ssl|. It is similar to
// |SSL_write|, but instead of returning the number of bytes written, it returns
// 1 on success or 0 for failure. The number bytes actually written is stored in
// |written|.
//
// This is only maintained for OpenSSL compatibility. Use |SSL_write| instead.
OPENSSL_EXPORT int SSL_write_ex(SSL *s, const void *buf, size_t num,
                                size_t *written);

// SSL_KEY_UPDATE_REQUESTED indicates that the peer should reply to a KeyUpdate
// message with its own, thus updating traffic secrets for both directions on
// the connection.
#define SSL_KEY_UPDATE_REQUESTED 1

// SSL_KEY_UPDATE_NOT_REQUESTED indicates that the peer should not reply with
// its own KeyUpdate message.
#define SSL_KEY_UPDATE_NOT_REQUESTED 0

// SSL_KEY_UPDATE_NONE should not be set by the user and is only used to
// indicate that there isn't a pending key operation. OpenSSL indicates that -1
// is used, so that it will be an invalid value for the on-the-wire protocol
// when calling |SSL_key_update|.
#define SSL_KEY_UPDATE_NONE -1

// SSL_key_update queues a TLS 1.3 KeyUpdate message to be sent on |ssl|
// if one is not already queued. The |request_type| argument must be either
// |SSL_KEY_UPDATE_REQUESTED| or |SSL_KEY_UPDATE_NOT_REQUESTED|. This function
// requires that |ssl| have completed a TLS >= 1.3 handshake. It returns one on
// success or zero on error.
//
// If |request_type| is set to |SSL_KEY_UPDATE_NOT_REQUESTED|, then the sending
// keys for this connection will be updated and the peer will be informed of the
// change.
// If |request_type| is set to |SSL_KEY_UPDATE_REQUESTED|, then the sending keys
// for this connection will be updated and the peer will be informed of the
// change along with a request for the peer to additionally update its sending
// keys. RFC: https://datatracker.ietf.org/doc/html/rfc8446#section-4.6.3
//
// Note that this function does not _send_ the message itself. The next call to
// |SSL_write| will cause the message to be sent. |SSL_write| may be called with
// a zero length to flush a KeyUpdate message when no application data is
// pending.
OPENSSL_EXPORT int SSL_key_update(SSL *ssl, int request_type);

// SSL_get_key_update_type returns the state of the pending key operation in
// |ssl|. The type of pending key operation will be either
// |SSL_KEY_UPDATE_REQUESTED| or |SSL_KEY_UPDATE_NOT_REQUESTED| if there is one,
// and |SSL_KEY_UPDATE_NONE| otherwise. This can be used to indicate whether
// a key update operation has been scheduled but not yet performed.
OPENSSL_EXPORT int SSL_get_key_update_type(const SSL *ssl);

// SSL_shutdown shuts down |ssl|. It runs in two stages. First, it sends
// close_notify and returns zero or one on success or -1 on failure. Zero
// indicates that close_notify was sent, but not received, and one additionally
// indicates that the peer's close_notify had already been received.
//
// To then wait for the peer's close_notify, run |SSL_shutdown| to completion a
// second time. This returns 1 on success and -1 on failure. Application data
// is considered a fatal error at this point. To process or discard it, read
// until close_notify with |SSL_read| instead.
//
// In both cases, on failure, pass the return value into |SSL_get_error| to
// determine how to proceed.
//
// Most callers should stop at the first stage. Reading for close_notify is
// primarily used for uncommon protocols where the underlying transport is
// reused after TLS completes. Additionally, DTLS uses an unordered transport
// and is unordered, so the second stage is a no-op in DTLS.
OPENSSL_EXPORT int SSL_shutdown(SSL *ssl);

// SSL_CTX_set_quiet_shutdown sets quiet shutdown on |ctx| to |mode|. If
// enabled, |SSL_shutdown| will not send a close_notify alert or wait for one
// from the peer. It will instead synchronously return one.
OPENSSL_EXPORT void SSL_CTX_set_quiet_shutdown(SSL_CTX *ctx, int mode);

// SSL_CTX_get_quiet_shutdown returns whether quiet shutdown is enabled for
// |ctx|.
OPENSSL_EXPORT int SSL_CTX_get_quiet_shutdown(const SSL_CTX *ctx);

// SSL_set_quiet_shutdown sets quiet shutdown on |ssl| to |mode|. If enabled,
// |SSL_shutdown| will not send a close_notify alert or wait for one from the
// peer. It will instead synchronously return one.
OPENSSL_EXPORT void SSL_set_quiet_shutdown(SSL *ssl, int mode);

// SSL_get_quiet_shutdown returns whether quiet shutdown is enabled for
// |ssl|.
OPENSSL_EXPORT int SSL_get_quiet_shutdown(const SSL *ssl);

// SSL_get_error returns a |SSL_ERROR_*| value for the most recent operation on
// |ssl|. It should be called after an operation failed to determine whether the
// error was fatal and, if not, when to retry.
OPENSSL_EXPORT int SSL_get_error(const SSL *ssl, int ret_code);

// SSL_ERROR_NONE indicates the operation succeeded.
#define SSL_ERROR_NONE 0

// SSL_ERROR_SSL indicates the operation failed within the library. The caller
// may inspect the error queue for more information.
#define SSL_ERROR_SSL 1

// SSL_ERROR_WANT_READ indicates the operation failed attempting to read from
// the transport. The caller may retry the operation when the transport is ready
// for reading.
//
// If signaled by a DTLS handshake, the caller must also call
// |DTLSv1_get_timeout| and |DTLSv1_handle_timeout| as appropriate. See
// |SSL_do_handshake|.
#define SSL_ERROR_WANT_READ 2

// SSL_ERROR_WANT_WRITE indicates the operation failed attempting to write to
// the transport. The caller may retry the operation when the transport is ready
// for writing.
#define SSL_ERROR_WANT_WRITE 3

// SSL_ERROR_WANT_X509_LOOKUP indicates the operation failed in calling the
// |cert_cb| or |client_cert_cb|. The caller may retry the operation when the
// callback is ready to return a certificate or one has been configured
// externally.
//
// See also |SSL_CTX_set_cert_cb| and |SSL_CTX_set_client_cert_cb|.
#define SSL_ERROR_WANT_X509_LOOKUP 4

// SSL_ERROR_SYSCALL indicates the operation failed externally to the library.
// The caller should consult the system-specific error mechanism. This is
// typically |errno| but may be something custom if using a custom |BIO|. It
// may also be signaled if the transport returned EOF, in which case the
// operation's return value will be zero.
#define SSL_ERROR_SYSCALL 5

// SSL_ERROR_ZERO_RETURN indicates the operation failed because the connection
// was cleanly shut down with a close_notify alert.
#define SSL_ERROR_ZERO_RETURN 6

// SSL_ERROR_WANT_CONNECT indicates the operation failed attempting to connect
// the transport (the |BIO| signaled |BIO_RR_CONNECT|). The caller may retry the
// operation when the transport is ready.
#define SSL_ERROR_WANT_CONNECT 7

// SSL_ERROR_WANT_ACCEPT indicates the operation failed attempting to accept a
// connection from the transport (the |BIO| signaled |BIO_RR_ACCEPT|). The
// caller may retry the operation when the transport is ready.
//
// TODO(davidben): Remove this. It's used by accept BIOs which are bizarre.
#define SSL_ERROR_WANT_ACCEPT 8

// SSL_ERROR_WANT_CHANNEL_ID_LOOKUP is never used.
//
// TODO(davidben): Remove this. Some callers reference it when stringifying
// errors. They should use |SSL_error_description| instead.
#define SSL_ERROR_WANT_CHANNEL_ID_LOOKUP 9

// SSL_ERROR_PENDING_SESSION indicates the operation failed because the session
// lookup callback indicated the session was unavailable. The caller may retry
// the operation when lookup has completed.
//
// See also |SSL_CTX_sess_set_get_cb| and |SSL_magic_pending_session_ptr|.
#define SSL_ERROR_PENDING_SESSION 11

// SSL_ERROR_PENDING_CERTIFICATE indicates the operation failed because the
// early callback indicated certificate lookup was incomplete. The caller may
// retry the operation when lookup has completed.
//
// See also |SSL_CTX_set_select_certificate_cb|.
#define SSL_ERROR_PENDING_CERTIFICATE 12

// SSL_ERROR_WANT_PRIVATE_KEY_OPERATION indicates the operation failed because
// a private key operation was unfinished. The caller may retry the operation
// when the private key operation is complete.
//
// See also |SSL_set_private_key_method| and
// |SSL_CTX_set_private_key_method|.
#define SSL_ERROR_WANT_PRIVATE_KEY_OPERATION 13

// SSL_ERROR_PENDING_TICKET indicates that a ticket decryption is pending. The
// caller may retry the operation when the decryption is ready.
//
// See also |SSL_CTX_set_ticket_aead_method|.
#define SSL_ERROR_PENDING_TICKET 14

// SSL_ERROR_EARLY_DATA_REJECTED indicates that early data was rejected. The
// caller should treat this as a connection failure and retry any operations
// associated with the rejected early data. |SSL_reset_early_data_reject| may be
// used to reuse the underlying connection for the retry.
#define SSL_ERROR_EARLY_DATA_REJECTED 15

// SSL_ERROR_WANT_CERTIFICATE_VERIFY indicates the operation failed because
// certificate verification was incomplete. The caller may retry the operation
// when certificate verification is complete.
//
// See also |SSL_CTX_set_custom_verify|.
#define SSL_ERROR_WANT_CERTIFICATE_VERIFY 16

#define SSL_ERROR_HANDOFF 17
#define SSL_ERROR_HANDBACK 18

// SSL_ERROR_WANT_RENEGOTIATE indicates the operation is pending a response to
// a renegotiation request from the server. The caller may call
// |SSL_renegotiate| to schedule a renegotiation and retry the operation.
//
// See also |ssl_renegotiate_explicit|.
#define SSL_ERROR_WANT_RENEGOTIATE 19

// SSL_ERROR_HANDSHAKE_HINTS_READY indicates the handshake has progressed enough
// for |SSL_serialize_handshake_hints| to be called. See also
// |SSL_request_handshake_hints|.
#define SSL_ERROR_HANDSHAKE_HINTS_READY 20

// SSL_error_description returns a string representation of |err|, where |err|
// is one of the |SSL_ERROR_*| constants returned by |SSL_get_error|, or NULL
// if the value is unrecognized.
OPENSSL_EXPORT const char *SSL_error_description(int err);

// SSL_set_mtu sets the |ssl|'s MTU in DTLS to |mtu|. It returns one on success
// and zero on failure.
OPENSSL_EXPORT int SSL_set_mtu(SSL *ssl, unsigned mtu);

// DTLSv1_set_initial_timeout_duration sets the initial duration for a DTLS
// handshake timeout.
//
// This duration overrides the default of 1 second, which is the strong
// recommendation of RFC 6347 (see section 4.2.4.1). However, there may exist
// situations where a shorter timeout would be beneficial, such as for
// time-sensitive applications.
OPENSSL_EXPORT void DTLSv1_set_initial_timeout_duration(SSL *ssl,
                                                        unsigned duration_ms);

// DTLSv1_get_timeout queries the next DTLS handshake timeout. If there is a
// timeout in progress, it sets |*out| to the time remaining and returns one.
// Otherwise, it returns zero.
//
// When the timeout expires, call |DTLSv1_handle_timeout| to handle the
// retransmit behavior.
//
// NOTE: This function must be queried again whenever the handshake state
// machine changes, including when |DTLSv1_handle_timeout| is called.
OPENSSL_EXPORT int DTLSv1_get_timeout(const SSL *ssl, struct timeval *out);

// DTLSv1_handle_timeout is called when a DTLS handshake timeout expires. If no
// timeout had expired, it returns 0. Otherwise, it retransmits the previous
// flight of handshake messages and returns 1. If too many timeouts had expired
// without progress or an error occurs, it returns -1.
//
// The caller's external timer should be compatible with the one |ssl| queries
// within some fudge factor. Otherwise, the call will be a no-op, but
// |DTLSv1_get_timeout| will return an updated timeout.
//
// If the function returns -1, checking if |SSL_get_error| returns
// |SSL_ERROR_WANT_WRITE| may be used to determine if the retransmit failed due
// to a non-fatal error at the write |BIO|. However, the operation may not be
// retried until the next timeout fires.
//
// WARNING: This function breaks the usual return value convention.
//
// TODO(davidben): This |SSL_ERROR_WANT_WRITE| behavior is kind of bizarre.
OPENSSL_EXPORT int DTLSv1_handle_timeout(SSL *ssl);


// Protocol versions.

#define DTLS1_VERSION_MAJOR 0xfe
#define SSL3_VERSION_MAJOR 0x03

#define SSL3_VERSION 0x0300
#define TLS1_VERSION 0x0301
#define TLS1_1_VERSION 0x0302
#define TLS1_2_VERSION 0x0303
#define TLS1_3_VERSION 0x0304

#define DTLS1_VERSION 0xfeff
#define DTLS1_2_VERSION 0xfefd

// SSL_CTX_set_min_proto_version sets the minimum protocol version for |ctx| to
// |version|. If |version| is zero, the default minimum version is used. It
// returns one on success and zero if |version| is invalid.
OPENSSL_EXPORT int SSL_CTX_set_min_proto_version(SSL_CTX *ctx,
                                                 uint16_t version);

// SSL_CTX_set_max_proto_version sets the maximum protocol version for |ctx| to
// |version|. If |version| is zero, the default maximum version is used. It
// returns one on success and zero if |version| is invalid.
OPENSSL_EXPORT int SSL_CTX_set_max_proto_version(SSL_CTX *ctx,
                                                 uint16_t version);

// SSL_CTX_get_min_proto_version returns the minimum protocol version for |ctx|.
// If |ctx| is configured to use the default minimum version, 0 is returned.
OPENSSL_EXPORT uint16_t SSL_CTX_get_min_proto_version(const SSL_CTX *ctx);

// SSL_CTX_get_max_proto_version returns the maximum protocol version for |ctx|.
// If |ctx| is configured to use the default maximum version, 0 is returned.
OPENSSL_EXPORT uint16_t SSL_CTX_get_max_proto_version(const SSL_CTX *ctx);

// SSL_set_min_proto_version sets the minimum protocol version for |ssl| to
// |version|. If |version| is zero, the default minimum version is used. It
// returns one on success and zero if |version| is invalid.
OPENSSL_EXPORT int SSL_set_min_proto_version(SSL *ssl, uint16_t version);

// SSL_set_max_proto_version sets the maximum protocol version for |ssl| to
// |version|. If |version| is zero, the default maximum version is used. It
// returns one on success and zero if |version| is invalid.
OPENSSL_EXPORT int SSL_set_max_proto_version(SSL *ssl, uint16_t version);

// SSL_get_min_proto_version returns the minimum protocol version for |ssl|. If
// the connection's configuration has been shed or |ssl| is configured to use
// the default min version, 0 is returned.
OPENSSL_EXPORT uint16_t SSL_get_min_proto_version(const SSL *ssl);

// SSL_get_max_proto_version returns the maximum protocol version for |ssl|. If
// the connection's configuration has been shed or |ssl| is configured to use
// the default max version, 0 is returned.
OPENSSL_EXPORT uint16_t SSL_get_max_proto_version(const SSL *ssl);

// SSL_version returns the TLS or DTLS protocol version used by |ssl|, which is
// one of the |*_VERSION| values. (E.g. |TLS1_2_VERSION|.) Before the version
// is negotiated, the result is undefined.
OPENSSL_EXPORT int SSL_version(const SSL *ssl);


// Options.
//
// Options configure protocol behavior.

// SSL_OP_NO_QUERY_MTU, in DTLS, disables querying the MTU from the underlying
// |BIO|. Instead, the MTU is configured with |SSL_set_mtu|.
#define SSL_OP_NO_QUERY_MTU 0x00001000L

// SSL_OP_NO_TICKET disables session ticket support (RFC 5077).
#define SSL_OP_NO_TICKET 0x00004000L

// SSL_OP_CIPHER_SERVER_PREFERENCE configures servers to select ciphers and
// ECDHE curves according to the server's preferences instead of the
// client's.
#define SSL_OP_CIPHER_SERVER_PREFERENCE 0x00400000L

// The following flags toggle individual protocol versions. This is deprecated.
// Use |SSL_CTX_set_min_proto_version| and |SSL_CTX_set_max_proto_version|
// instead.
#define SSL_OP_NO_TLSv1 0x04000000L
#define SSL_OP_NO_TLSv1_2 0x08000000L
#define SSL_OP_NO_TLSv1_1 0x10000000L
#define SSL_OP_NO_TLSv1_3 0x20000000L
#define SSL_OP_NO_DTLSv1 SSL_OP_NO_TLSv1
#define SSL_OP_NO_DTLSv1_2 SSL_OP_NO_TLSv1_2

// SSL_CTX_set_options enables all options set in |options| (which should be one
// or more of the |SSL_OP_*| values, ORed together) in |ctx|. It returns a
// bitmask representing the resulting enabled options.
OPENSSL_EXPORT uint32_t SSL_CTX_set_options(SSL_CTX *ctx, uint32_t options);

// SSL_CTX_clear_options disables all options set in |options| (which should be
// one or more of the |SSL_OP_*| values, ORed together) in |ctx|. It returns a
// bitmask representing the resulting enabled options.
OPENSSL_EXPORT uint32_t SSL_CTX_clear_options(SSL_CTX *ctx, uint32_t options);

// SSL_CTX_get_options returns a bitmask of |SSL_OP_*| values that represent all
// the options enabled for |ctx|.
OPENSSL_EXPORT uint32_t SSL_CTX_get_options(const SSL_CTX *ctx);

// SSL_set_options enables all options set in |options| (which should be one or
// more of the |SSL_OP_*| values, ORed together) in |ssl|. It returns a bitmask
// representing the resulting enabled options.
OPENSSL_EXPORT uint32_t SSL_set_options(SSL *ssl, uint32_t options);

// SSL_clear_options disables all options set in |options| (which should be one
// or more of the |SSL_OP_*| values, ORed together) in |ssl|. It returns a
// bitmask representing the resulting enabled options.
OPENSSL_EXPORT uint32_t SSL_clear_options(SSL *ssl, uint32_t options);

// SSL_get_options returns a bitmask of |SSL_OP_*| values that represent all the
// options enabled for |ssl|.
OPENSSL_EXPORT uint32_t SSL_get_options(const SSL *ssl);


// Modes.
//
// Modes configure API behavior.

// SSL_MODE_ENABLE_PARTIAL_WRITE, in TLS, allows |SSL_write| to complete with a
// partial result when the only part of the input was written in a single
// record. In DTLS, it does nothing.
#define SSL_MODE_ENABLE_PARTIAL_WRITE 0x00000001L

// SSL_MODE_ACCEPT_MOVING_WRITE_BUFFER, in TLS, allows retrying an incomplete
// |SSL_write| with a different buffer. However, |SSL_write| still assumes the
// buffer contents are unchanged. This is not the default to avoid the
// misconception that non-blocking |SSL_write| behaves like non-blocking
// |write|. In DTLS, it does nothing.
#define SSL_MODE_ACCEPT_MOVING_WRITE_BUFFER 0x00000002L

// SSL_MODE_NO_AUTO_CHAIN disables automatically building a certificate chain
// before sending certificates to the peer. This flag is set (and the feature
// disabled) by default.
// By default, OpenSSL automatically builds a certificate chain on the fly if
// there is no certificate chain explicitly provided. This feature is called
// Auto-Chaining. Auto-Chaining can be turned off in OpenSSL by setting the
// |SSL_MODE_NO_AUTO_CHAIN| flag for the SSL connection. AWS-LC has this flag
// turned on (auto-chaining off) by default. This forces the certificate chain
// to be explicit, and no longer results in unexpected certificate chains being
// sent back to clients. This may cause issues for services migrating to AWS-LC,
// if the service had been reliant on the default behavior. Services should
// restructure their certificate chains to not use the default auto-chaining
// behavior from OpenSSL when porting to AWS-LC. We highly recommend not to
// re-enable Auto-Chaining, but if a consumer had been relying on this default
// behavior, they can temporarily revert back with |SSL_[CTX_]clear_mode|.
// However, it is generally expected of AWS-LC consumers to structure their
// certificate chains to not rely on auto-chaining.
#define SSL_MODE_NO_AUTO_CHAIN 0x00000008L

// SSL_MODE_ENABLE_FALSE_START allows clients to send application data before
// receipt of ChangeCipherSpec and Finished. This mode enables full handshakes
// to 'complete' in one RTT. See RFC 7918.
//
// When False Start is enabled, |SSL_do_handshake| may succeed before the
// handshake has completely finished. |SSL_write| will function at this point,
// and |SSL_read| will transparently wait for the final handshake leg before
// returning application data. To determine if False Start occurred or when the
// handshake is completely finished, see |SSL_in_false_start|, |SSL_in_init|,
// and |SSL_CB_HANDSHAKE_DONE| from |SSL_CTX_set_info_callback|.
#define SSL_MODE_ENABLE_FALSE_START 0x00000080L

// SSL_MODE_CBC_RECORD_SPLITTING causes multi-byte CBC records in TLS 1.0 to be
// split in two: the first record will contain a single byte and the second will
// contain the remainder. This effectively randomises the IV and prevents BEAST
// attacks.
#define SSL_MODE_CBC_RECORD_SPLITTING 0x00000100L

// SSL_MODE_NO_SESSION_CREATION will cause any attempts to create a session to
// fail with SSL_R_SESSION_MAY_NOT_BE_CREATED. This can be used to enforce that
// session resumption is used for a given SSL*.
#define SSL_MODE_NO_SESSION_CREATION 0x00000200L

// SSL_MODE_SEND_FALLBACK_SCSV sends TLS_FALLBACK_SCSV in the ClientHello.
// To be set only by applications that reconnect with a downgraded protocol
// version; see RFC 7507 for details.
//
// DO NOT ENABLE THIS if your application attempts a normal handshake. Only use
// this in explicit fallback retries, following the guidance in RFC 7507.
#define SSL_MODE_SEND_FALLBACK_SCSV 0x00000400L

// SSL_CTX_set_mode enables all modes set in |mode| (which should be one or more
// of the |SSL_MODE_*| values, ORed together) in |ctx|. It returns a bitmask
// representing the resulting enabled modes.
OPENSSL_EXPORT uint32_t SSL_CTX_set_mode(SSL_CTX *ctx, uint32_t mode);

// SSL_CTX_clear_mode disables all modes set in |mode| (which should be one or
// more of the |SSL_MODE_*| values, ORed together) in |ctx|. It returns a
// bitmask representing the resulting enabled modes.
OPENSSL_EXPORT uint32_t SSL_CTX_clear_mode(SSL_CTX *ctx, uint32_t mode);

// SSL_CTX_get_mode returns a bitmask of |SSL_MODE_*| values that represent all
// the modes enabled for |ssl|.
OPENSSL_EXPORT uint32_t SSL_CTX_get_mode(const SSL_CTX *ctx);

// SSL_set_mode enables all modes set in |mode| (which should be one or more of
// the |SSL_MODE_*| values, ORed together) in |ssl|. It returns a bitmask
// representing the resulting enabled modes.
OPENSSL_EXPORT uint32_t SSL_set_mode(SSL *ssl, uint32_t mode);

// SSL_clear_mode disables all modes set in |mode| (which should be one or more
// of the |SSL_MODE_*| values, ORed together) in |ssl|. It returns a bitmask
// representing the resulting enabled modes.
OPENSSL_EXPORT uint32_t SSL_clear_mode(SSL *ssl, uint32_t mode);

// SSL_get_mode returns a bitmask of |SSL_MODE_*| values that represent all the
// modes enabled for |ssl|.
OPENSSL_EXPORT uint32_t SSL_get_mode(const SSL *ssl);

// SSL_CTX_set0_buffer_pool sets a |CRYPTO_BUFFER_POOL| that will be used to
// store certificates. This can allow multiple connections to share
// certificates and thus save memory.
//
// The SSL_CTX does not take ownership of |pool| and the caller must ensure
// that |pool| outlives |ctx| and all objects linked to it, including |SSL|,
// |X509| and |SSL_SESSION| objects. Basically, don't ever free |pool|.
OPENSSL_EXPORT void SSL_CTX_set0_buffer_pool(SSL_CTX *ctx,
                                             CRYPTO_BUFFER_POOL *pool);


// Configuring certificates and private keys.
//
// These functions configure the connection's leaf certificate, private key, and
// certificate chain. The certificate chain is ordered leaf to root (as sent on
// the wire) but does not include the leaf. Both client and server certificates
// use these functions.
//
// Certificates and keys may be configured before the handshake or dynamically
// in the early callback and certificate callback.

// SSL_CTX_use_certificate sets |ctx|'s leaf certificate to |x509|. It returns
// one on success and zero on failure.
OPENSSL_EXPORT int SSL_CTX_use_certificate(SSL_CTX *ctx, X509 *x509);

// SSL_use_certificate sets |ssl|'s leaf certificate to |x509|. It returns one
// on success and zero on failure.
OPENSSL_EXPORT int SSL_use_certificate(SSL *ssl, X509 *x509);

// SSL_CTX_use_PrivateKey sets |ctx|'s private key to |pkey|. It returns one on
// success and zero on failure.
OPENSSL_EXPORT int SSL_CTX_use_PrivateKey(SSL_CTX *ctx, EVP_PKEY *pkey);

// SSL_use_PrivateKey sets |ssl|'s private key to |pkey|. It returns one on
// success and zero on failure.
OPENSSL_EXPORT int SSL_use_PrivateKey(SSL *ssl, EVP_PKEY *pkey);

// SSL_CTX_set0_chain sets |ctx|'s certificate chain, excluding the leaf, to
// |chain|. On success, it returns one and takes ownership of |chain|.
// Otherwise, it returns zero.
OPENSSL_EXPORT int SSL_CTX_set0_chain(SSL_CTX *ctx, STACK_OF(X509) *chain);

// SSL_CTX_set1_chain sets |ctx|'s certificate chain, excluding the leaf, to
// |chain|. It returns one on success and zero on failure. The caller retains
// ownership of |chain| and may release it freely.
OPENSSL_EXPORT int SSL_CTX_set1_chain(SSL_CTX *ctx, STACK_OF(X509) *chain);

// SSL_set0_chain sets |ssl|'s certificate chain, excluding the leaf, to
// |chain|. On success, it returns one and takes ownership of |chain|.
// Otherwise, it returns zero.
OPENSSL_EXPORT int SSL_set0_chain(SSL *ssl, STACK_OF(X509) *chain);

// SSL_set1_chain sets |ssl|'s certificate chain, excluding the leaf, to
// |chain|. It returns one on success and zero on failure. The caller retains
// ownership of |chain| and may release it freely.
OPENSSL_EXPORT int SSL_set1_chain(SSL *ssl, STACK_OF(X509) *chain);

// SSL_CTX_add0_chain_cert appends |x509| to |ctx|'s certificate chain. On
// success, it returns one and takes ownership of |x509|. Otherwise, it returns
// zero.
OPENSSL_EXPORT int SSL_CTX_add0_chain_cert(SSL_CTX *ctx, X509 *x509);

// SSL_CTX_add1_chain_cert appends |x509| to |ctx|'s certificate chain. It
// returns one on success and zero on failure. The caller retains ownership of
// |x509| and may release it freely.
OPENSSL_EXPORT int SSL_CTX_add1_chain_cert(SSL_CTX *ctx, X509 *x509);

// SSL_add0_chain_cert appends |x509| to |ctx|'s certificate chain. On success,
// it returns one and takes ownership of |x509|. Otherwise, it returns zero.
OPENSSL_EXPORT int SSL_add0_chain_cert(SSL *ssl, X509 *x509);

// SSL_CTX_add_extra_chain_cert calls |SSL_CTX_add0_chain_cert|.
OPENSSL_EXPORT int SSL_CTX_add_extra_chain_cert(SSL_CTX *ctx, X509 *x509);

// SSL_add1_chain_cert appends |x509| to |ctx|'s certificate chain. It returns
// one on success and zero on failure. The caller retains ownership of |x509|
// and may release it freely.
OPENSSL_EXPORT int SSL_add1_chain_cert(SSL *ssl, X509 *x509);


// The following flags are used for configuring |SSL_CTX_build_cert_chain| and
// |SSL_build_cert_chain|.

// SSL_BUILD_CHAIN_FLAG_UNTRUSTED treats any existing certificates as untrusted
// CAs. This is mutually exclusive to |SSL_BUILD_CHAIN_FLAG_CHECK| and
// |SSL_BUILD_CHAIN_FLAG_CHECK| will be prioritized to use the existing chain
// certificates instead.
#define SSL_BUILD_CHAIN_FLAG_UNTRUSTED 0x1

// SSL_BUILD_CHAIN_FLAG_NO_ROOT omits the root CA from the built chain.
#define SSL_BUILD_CHAIN_FLAG_NO_ROOT 0x2

// SSL_BUILD_CHAIN_FLAG_CHECK uses only existing chain certificates to build the
// chain (effectively sanity checking and rearranging them if necessary). This
// is mutually exclusive to |SSL_BUILD_CHAIN_FLAG_UNTRUSTED| and
// |SSL_BUILD_CHAIN_FLAG_CHECK| will be prioritized.
#define SSL_BUILD_CHAIN_FLAG_CHECK 0x4

// SSL_BUILD_CHAIN_FLAG_IGNORE_ERROR ignores errors during certificate
// verification, but still pushes the error onto the error queue.
#define SSL_BUILD_CHAIN_FLAG_IGNORE_ERROR 0x8

// SSL_BUILD_CHAIN_FLAG_CLEAR_ERROR is only used when
// |SSL_BUILD_CHAIN_FLAG_IGNORE_ERROR| is defined. It clears the error from the
// error queue if certificate verification has failed.
#define SSL_BUILD_CHAIN_FLAG_CLEAR_ERROR 0x10

// SSL_CTX_build_cert_chain builds the certificate chain for |ctx|.
// Normally this uses the chain store or the verify store if the chain store is
// not set. If the function is successful, the built chain will replace any
// existing chain in |ctx|.
// This function returns 1 on success and -1 on failure, unless
// |SSL_BUILD_CHAIN_FLAG_IGNORE_ERROR| is set. If
// |SSL_BUILD_CHAIN_FLAG_IGNORE_ERROR| is set, then this function returns 1 on
// success, 2 if certificate verification has failed, and -1 on all other types
// of failures.
OPENSSL_EXPORT int SSL_CTX_build_cert_chain(SSL_CTX *ctx, int flags);

// SSL_build_cert_chain is similar to |SSL_CTX_build_cert_chain|, but the
// certificate chain is built for |ssl| instead.
OPENSSL_EXPORT int SSL_build_cert_chain(SSL *ssl, int flags);

// SSL_CTX_clear_chain_certs clears |ctx|'s certificate chain and returns
// one.
OPENSSL_EXPORT int SSL_CTX_clear_chain_certs(SSL_CTX *ctx);

// SSL_CTX_clear_extra_chain_certs calls |SSL_CTX_clear_chain_certs|.
OPENSSL_EXPORT int SSL_CTX_clear_extra_chain_certs(SSL_CTX *ctx);

// SSL_clear_chain_certs clears |ssl|'s certificate chain and returns one.
OPENSSL_EXPORT int SSL_clear_chain_certs(SSL *ssl);

// SSL_CTX_set_cert_cb sets a callback that is called to select a certificate.
// The callback returns one on success, zero on internal error, and a negative
// number on failure or to pause the handshake. If the handshake is paused,
// |SSL_get_error| will return |SSL_ERROR_WANT_X509_LOOKUP|.
//
// On the client, the callback may call |SSL_get0_certificate_types| and
// |SSL_get_client_CA_list| for information on the server's certificate
// request.
//
// On the server, the callback will be called after extensions have been
// processed, but before the resumption decision has been made. This differs
// from OpenSSL which handles resumption before selecting the certificate.
OPENSSL_EXPORT void SSL_CTX_set_cert_cb(SSL_CTX *ctx,
                                        int (*cb)(SSL *ssl, void *arg),
                                        void *arg);

// SSL_set_cert_cb sets a callback that is called to select a certificate. The
// callback returns one on success, zero on internal error, and a negative
// number on failure or to pause the handshake. If the handshake is paused,
// |SSL_get_error| will return |SSL_ERROR_WANT_X509_LOOKUP|.
//
// On the client, the callback may call |SSL_get0_certificate_types| and
// |SSL_get_client_CA_list| for information on the server's certificate
// request.
//
// On the server, the callback will be called after extensions have been
// processed, but before the resumption decision has been made. This differs
// from OpenSSL which handles resumption before selecting the certificate.
OPENSSL_EXPORT void SSL_set_cert_cb(SSL *ssl, int (*cb)(SSL *ssl, void *arg),
                                    void *arg);

// SSL_get0_certificate_types, for a client, sets |*out_types| to an array
// containing the client certificate types requested by a server. It returns the
// length of the array. Note this list is always empty in TLS 1.3. The server
// will instead send signature algorithms. See
// |SSL_get0_peer_verify_algorithms|.
//
// The behavior of this function is undefined except during the callbacks set by
// by |SSL_CTX_set_cert_cb| and |SSL_CTX_set_client_cert_cb| or when the
// handshake is paused because of them.
OPENSSL_EXPORT size_t SSL_get0_certificate_types(const SSL *ssl,
                                                 const uint8_t **out_types);

// SSL_get0_peer_verify_algorithms sets |*out_sigalgs| to an array containing
// the signature algorithms the peer is able to verify. It returns the length of
// the array. Note these values are only sent starting TLS 1.2 and only
// mandatory starting TLS 1.3. If not sent, the empty array is returned. For the
// historical client certificate types list, see |SSL_get0_certificate_types|.
//
// The behavior of this function is undefined except during the callbacks set by
// by |SSL_CTX_set_cert_cb| and |SSL_CTX_set_client_cert_cb| or when the
// handshake is paused because of them.
OPENSSL_EXPORT size_t
SSL_get0_peer_verify_algorithms(const SSL *ssl, const uint16_t **out_sigalgs);

// SSL_get0_peer_delegation_algorithms sets |*out_sigalgs| to an array
// containing the signature algorithms the peer is willing to use with delegated
// credentials.  It returns the length of the array. If not sent, the empty
// array is returned.
//
// The behavior of this function is undefined except during the callbacks set by
// by |SSL_CTX_set_cert_cb| and |SSL_CTX_set_client_cert_cb| or when the
// handshake is paused because of them.
OPENSSL_EXPORT size_t SSL_get0_peer_delegation_algorithms(
    const SSL *ssl, const uint16_t **out_sigalgs);

// SSL_CLIENT_HELLO_SUCCESS indicates that the ClientHello processing was
// successful. The handshake will continue normally.
# define SSL_CLIENT_HELLO_SUCCESS 1
// SSL_CLIENT_HELLO_ERROR indicates that a fatal error occurred while processing
// the ClientHello. The callback should set |*al| to a TLS alert value to return
// to the client. The handshake will be aborted.
# define SSL_CLIENT_HELLO_ERROR   0
// SSL_CLIENT_HELLO_RETRY indicates that the handshake should be paused and the
// server should retry when it can continue.
//
// WARNING: The current implementation does not properly support
// SSL_CLIENT_HELLO_RETRY and attempting to use it will result in handshake
// failure.
# define SSL_CLIENT_HELLO_RETRY   (-1)

// SSL_client_hello_cb_fn is a callback function that is called when a ClientHello
// is received. The callback may inspect the ClientHello and make decisions 
// about the connection based on its contents.
//
// The callback should return one of:
// - SSL_CLIENT_HELLO_SUCCESS to continue the handshake
// - SSL_CLIENT_HELLO_ERROR to abort the handshake with alert |*al|
// - SSL_CLIENT_HELLO_RETRY (not supported) is handled like SSL_CLIENT_HELLO_ERROR
typedef int (*SSL_client_hello_cb_fn)(SSL *s, int *al, void *arg);

// SSL_client_hello_get1_extensions_present iterates over the extensions in the
// ClientHello. If any are found, it allocates an array of int and sets |*out|
// to point to this array and |*outlen| to the number of extensions. The ints
// in the array correspond to the type of each extension. The caller is
// responsible for releasing the array with OPENSSL_free. If no extensions are
// found, it sets |*out| to NULL and |*outlen| to 0. The function returns 1 on
// success and returns 0 on error.
//
// This function can only be called from within a client hello callback (see
// |SSL_CTX_set_client_hello_cb|) or during server certificate selection (see
// |SSL_CTX_set_select_certificate_cb|).
OPENSSL_EXPORT int SSL_client_hello_get1_extensions_present(SSL *s, int **out, size_t *outlen);

// SSL_client_hello_get_extension_order iterates over the extensions in the
// ClientHello. If |exts| is not null, the type for each extension will be
// stored in |exts| and |*num_exts| should be the size of storage
// allocated for |exts|; the function will return an error if |*num_exts| is
// too small. On success, the function will return 1 and will set |*num_exts| to
// the number of extensions. The caller may pass |exts| as null to obtain the
// number of extensions. If no ClientHello extensions are found, the
// function returns 1 and sets |*num_exts| to 0. The functions returns 0 on
// error.
//
// This function can only be called from within a client hello callback (see
// |SSL_CTX_set_client_hello_cb|) or during server certificate selection (see
// |SSL_CTX_set_select_certificate_cb|).
OPENSSL_EXPORT int SSL_client_hello_get_extension_order(SSL *s, uint16_t *exts, size_t *num_exts);

// SSL_CTX_set_client_hello_cb configures a callback that is called when a
// ClientHello message is received. This can be used to select certificates,
// adjust settings, or otherwise make decisions about the connection before
// the handshake proceeds.
//
// The callback is invoked before most ClientHello processing and before the
// decision whether to resume a session is made. It may inspect the ClientHello
// and configure the connection accordingly.
//
// The callback function is passed the |SSL| object, a pointer to an alert value,
// and the argument provided when the callback was set. It should return either
// SSL_CLIENT_HELLO_SUCCESS, SSL_CLIENT_HELLO_ERROR, or SSL_CLIENT_HELLO_RETRY.
// Note that SSL_CLIENT_HELLO_RETRY is not fully supported; it is treated the
// same as SSL_CLIENT_HELLO_ERROR.
OPENSSL_EXPORT void SSL_CTX_set_client_hello_cb(SSL_CTX *c, SSL_client_hello_cb_fn cb,
                                 void *arg);

// SSL_client_hello_isv2 always returns zero as SSLv2 is not supported.
OPENSSL_EXPORT int SSL_client_hello_isv2(SSL *s);


// SSL_client_hello_get0_legacy_version provides the value of the
// "legacy_version" field in the client hello.
//
// This function can only be called from within a client hello callback (see
// |SSL_CTX_set_client_hello_cb|) or during server certificate selection (see
// |SSL_CTX_set_select_certificate_cb|).
OPENSSL_EXPORT unsigned int SSL_client_hello_get0_legacy_version(SSL *s);

// SSL_client_hello_get0_ext searches the extensions in the ClientHello for an
// extension of the given type. If found, it sets |*out| to point to the
// extension contents (not including the type and length bytes) and |*outlen|
// to the length of the extension contents. If not found, it returns zero.
//
// This function can only be called from within a client hello callback (see
// |SSL_CTX_set_client_hello_cb|) or during server certificate selection (see
// |SSL_CTX_set_select_certificate_cb|).
OPENSSL_EXPORT int SSL_client_hello_get0_ext(SSL *s, unsigned int type, const unsigned char **out,
                              size_t *outlen);

// SSL_certs_clear resets the private key, leaf certificate, and certificate
// chain of |ssl|.
OPENSSL_EXPORT void SSL_certs_clear(SSL *ssl);

// SSL_CTX_check_private_key returns one if the certificate and private key
// configured in |ctx| are consistent and zero otherwise.
OPENSSL_EXPORT int SSL_CTX_check_private_key(const SSL_CTX *ctx);

// SSL_check_private_key returns one if the certificate and private key
// configured in |ssl| are consistent and zero otherwise.
OPENSSL_EXPORT int SSL_check_private_key(const SSL *ssl);

// SSL_CTX_get0_certificate returns |ctx|'s leaf certificate.
OPENSSL_EXPORT X509 *SSL_CTX_get0_certificate(const SSL_CTX *ctx);

// SSL_get_certificate returns |ssl|'s leaf certificate.
OPENSSL_EXPORT X509 *SSL_get_certificate(const SSL *ssl);

// SSL_CTX_get0_privatekey returns |ctx|'s private key.
OPENSSL_EXPORT EVP_PKEY *SSL_CTX_get0_privatekey(const SSL_CTX *ctx);

// SSL_get_privatekey returns |ssl|'s private key.
OPENSSL_EXPORT EVP_PKEY *SSL_get_privatekey(const SSL *ssl);

// SSL_CTX_get0_chain_certs sets |*out_chain| to |ctx|'s certificate chain and
// returns one.
OPENSSL_EXPORT int SSL_CTX_get0_chain_certs(const SSL_CTX *ctx,
                                            STACK_OF(X509) **out_chain);

// SSL_CTX_get_extra_chain_certs calls |SSL_CTX_get0_chain_certs|.
OPENSSL_EXPORT int SSL_CTX_get_extra_chain_certs(const SSL_CTX *ctx,
                                                 STACK_OF(X509) **out_chain);

// SSL_get0_chain_certs sets |*out_chain| to |ssl|'s certificate chain and
// returns one.
OPENSSL_EXPORT int SSL_get0_chain_certs(const SSL *ssl,
                                        STACK_OF(X509) **out_chain);

// SSL_CTX_set_signed_cert_timestamp_list sets the list of signed certificate
// timestamps that is sent to clients that request it. The |list| argument must
// contain one or more SCT structures serialised as a SignedCertificateTimestamp
// List (see https://tools.ietf.org/html/rfc6962#section-3.3) – i.e. each SCT
// is prefixed by a big-endian, uint16 length and the concatenation of one or
// more such prefixed SCTs are themselves also prefixed by a uint16 length. It
// returns one on success and zero on error. The caller retains ownership of
// |list|.
OPENSSL_EXPORT int SSL_CTX_set_signed_cert_timestamp_list(SSL_CTX *ctx,
                                                          const uint8_t *list,
                                                          size_t list_len);

// SSL_set_signed_cert_timestamp_list sets the list of signed certificate
// timestamps that is sent to clients that request is. The same format as the
// one used for |SSL_CTX_set_signed_cert_timestamp_list| applies. The caller
// retains ownership of |list|.
OPENSSL_EXPORT int SSL_set_signed_cert_timestamp_list(SSL *ctx,
                                                      const uint8_t *list,
                                                      size_t list_len);

// SSL_CTX_set_ocsp_response sets the OCSP response that is sent to clients
// which request it. It returns one on success and zero on error. The caller
// retains ownership of |response|.
OPENSSL_EXPORT int SSL_CTX_set_ocsp_response(SSL_CTX *ctx,
                                             const uint8_t *response,
                                             size_t response_len);

// SSL_set_ocsp_response sets the OCSP response that is sent to clients which
// request it. It returns one on success and zero on error. The caller retains
// ownership of |response|.
OPENSSL_EXPORT int SSL_set_ocsp_response(SSL *ssl, const uint8_t *response,
                                         size_t response_len);

// SSL_SIGN_* are signature algorithm values as defined in TLS 1.3.
#define SSL_SIGN_RSA_PKCS1_SHA1 0x0201
#define SSL_SIGN_RSA_PKCS1_SHA256 0x0401
#define SSL_SIGN_RSA_PKCS1_SHA384 0x0501
#define SSL_SIGN_RSA_PKCS1_SHA512 0x0601
#define SSL_SIGN_ECDSA_SHA1 0x0203
#define SSL_SIGN_ECDSA_SECP256R1_SHA256 0x0403
#define SSL_SIGN_ECDSA_SECP384R1_SHA384 0x0503
#define SSL_SIGN_ECDSA_SECP521R1_SHA512 0x0603
#define SSL_SIGN_RSA_PSS_RSAE_SHA256 0x0804
#define SSL_SIGN_RSA_PSS_RSAE_SHA384 0x0805
#define SSL_SIGN_RSA_PSS_RSAE_SHA512 0x0806
#define SSL_SIGN_ED25519 0x0807

// SSL_SIGN_RSA_PKCS1_MD5_SHA1 is an internal signature algorithm used to
// specify raw RSASSA-PKCS1-v1_5 with an MD5/SHA-1 concatenation, as used in TLS
// before TLS 1.2.
#define SSL_SIGN_RSA_PKCS1_MD5_SHA1 0xff01

// SSL_get_signature_algorithm_name returns a human-readable name for |sigalg|,
// or NULL if unknown. If |include_curve| is one, the curve for ECDSA algorithms
// is included as in TLS 1.3. Otherwise, it is excluded as in TLS 1.2.
OPENSSL_EXPORT const char *SSL_get_signature_algorithm_name(uint16_t sigalg,
                                                            int include_curve);

// SSL_get_all_signature_algorithm_names outputs a list of possible strings
// |SSL_get_signature_algorithm_name| may return in this version of BoringSSL.
// It writes at most |max_out| entries to |out| and returns the total number it
// would have written, if |max_out| had been large enough. |max_out| may be
// initially set to zero to size the output.
//
// This function is only intended to help initialize tables in callers that want
// possible strings pre-declared. This list would not be suitable to set a list
// of supported features. It is in no particular order, and may contain
// placeholder, experimental, or deprecated values that do not apply to every
// caller. Future versions of BoringSSL may also return strings not in this
// list, so this does not apply if, say, sending strings across services.
OPENSSL_EXPORT size_t SSL_get_all_signature_algorithm_names(const char **out,
                                                            size_t max_out);

// SSL_get_signature_algorithm_key_type returns the key type associated with
// |sigalg| as an |EVP_PKEY_*| constant or |EVP_PKEY_NONE| if unknown.
OPENSSL_EXPORT int SSL_get_signature_algorithm_key_type(uint16_t sigalg);

// SSL_get_signature_algorithm_digest returns the digest function associated
// with |sigalg| or |NULL| if |sigalg| has no prehash (Ed25519) or is unknown.
OPENSSL_EXPORT const EVP_MD *SSL_get_signature_algorithm_digest(
    uint16_t sigalg);

// SSL_is_signature_algorithm_rsa_pss returns one if |sigalg| is an RSA-PSS
// signature algorithm and zero otherwise.
OPENSSL_EXPORT int SSL_is_signature_algorithm_rsa_pss(uint16_t sigalg);

// SSL_CTX_set_signing_algorithm_prefs configures |ctx| to use |prefs| as the
// preference list when signing with |ctx|'s private key. It returns one on
// success and zero on error. |prefs| should not include the internal-only value
// |SSL_SIGN_RSA_PKCS1_MD5_SHA1|.
OPENSSL_EXPORT int SSL_CTX_set_signing_algorithm_prefs(SSL_CTX *ctx,
                                                       const uint16_t *prefs,
                                                       size_t num_prefs);

// SSL_set_signing_algorithm_prefs configures |ssl| to use |prefs| as the
// preference list when signing with |ssl|'s private key. It returns one on
// success and zero on error. |prefs| should not include the internal-only value
// |SSL_SIGN_RSA_PKCS1_MD5_SHA1|.
OPENSSL_EXPORT int SSL_set_signing_algorithm_prefs(SSL *ssl,
                                                   const uint16_t *prefs,
                                                   size_t num_prefs);

// SSL_CTX_use_cert_and_key sets |x509|, |privatekey|, and |chain| on |ctx|.
// The |privatekey| argument must be the private key of the certificate |x509|.
// If the override argument is 0, then |x509|, |privatekey|, and |chain| are
// set only if all were not previously set. If override is non-0, then the
// certificate, private key and chain certs are always set. |privatekey| and
// |x509| are not copied or duplicated, their reference counts are incremented.
// In OpenSSL, a shallow copy of |chain| is stored with a reference count
// increment for all |X509| objects in the chain. In AWS-LC,
// we represent X509 chains as a CRYPTO_BUFFER stack. Therefore, we create a
// an internal copy and leave the |chain| parameter untouched. This means,
// changes to |chain| after this function is called will not update in |ctx|.
// This is different from OpenSSL which stores a reference to the X509
// certificates in the |chain| object.
//
// Returns one on success and zero on error.
OPENSSL_EXPORT int SSL_CTX_use_cert_and_key(SSL_CTX *ctx, X509 *x509,
                                            EVP_PKEY *privatekey,
                                            STACK_OF(X509) *chain, int override);

// Certificate and private key convenience functions.

// SSL_CTX_set_chain_and_key sets the certificate chain and private key for a
// TLS client or server. References to the given |CRYPTO_BUFFER| and |EVP_PKEY|
// objects are added as needed. Exactly one of |privkey| or |privkey_method|
// may be non-NULL. Returns one on success and zero on error.
OPENSSL_EXPORT int SSL_CTX_set_chain_and_key(
    SSL_CTX *ctx, CRYPTO_BUFFER *const *certs, size_t num_certs,
    EVP_PKEY *privkey, const SSL_PRIVATE_KEY_METHOD *privkey_method);

// SSL_set_chain_and_key sets the certificate chain and private key for a TLS
// client or server. References to the given |CRYPTO_BUFFER| and |EVP_PKEY|
// objects are added as needed. Exactly one of |privkey| or |privkey_method|
// may be non-NULL. Returns one on success and zero on error.
OPENSSL_EXPORT int SSL_set_chain_and_key(
    SSL *ssl, CRYPTO_BUFFER *const *certs, size_t num_certs, EVP_PKEY *privkey,
    const SSL_PRIVATE_KEY_METHOD *privkey_method);

// SSL_CTX_get0_chain returns the list of |CRYPTO_BUFFER|s that were set by
// |SSL_set_chain_and_key|, unless they have been discarded. Reference counts
// are not incremented by this call. The return value may be |NULL| if no chain
// has been set.
//
// (Note: if a chain was configured by non-|CRYPTO_BUFFER|-based functions then
// the return value is undefined and, even if not NULL, the stack itself may
// contain nullptrs. Thus you shouldn't mix this function with
// non-|CRYPTO_BUFFER| functions for manipulating the chain.)
//
// There is no |SSL*| version of this function because connections discard
// configuration after handshaking, thus making it of questionable utility.
OPENSSL_EXPORT const STACK_OF(CRYPTO_BUFFER) *SSL_CTX_get0_chain(
    const SSL_CTX *ctx);

// SSL_CTX_use_RSAPrivateKey sets |ctx|'s private key to |rsa|. It returns one
// on success and zero on failure.
OPENSSL_EXPORT int SSL_CTX_use_RSAPrivateKey(SSL_CTX *ctx, RSA *rsa);

// SSL_use_RSAPrivateKey sets |ctx|'s private key to |rsa|. It returns one on
// success and zero on failure.
OPENSSL_EXPORT int SSL_use_RSAPrivateKey(SSL *ssl, RSA *rsa);

// The following functions configure certificates or private keys but take as
// input DER-encoded structures. They return one on success and zero on
// failure.

OPENSSL_EXPORT int SSL_CTX_use_certificate_ASN1(SSL_CTX *ctx, size_t der_len,
                                                const uint8_t *der);
OPENSSL_EXPORT int SSL_use_certificate_ASN1(SSL *ssl, const uint8_t *der,
                                            size_t der_len);

OPENSSL_EXPORT int SSL_CTX_use_PrivateKey_ASN1(int pk, SSL_CTX *ctx,
                                               const uint8_t *der,
                                               size_t der_len);
OPENSSL_EXPORT int SSL_use_PrivateKey_ASN1(int type, SSL *ssl,
                                           const uint8_t *der, size_t der_len);

OPENSSL_EXPORT int SSL_CTX_use_RSAPrivateKey_ASN1(SSL_CTX *ctx,
                                                  const uint8_t *der,
                                                  size_t der_len);
OPENSSL_EXPORT int SSL_use_RSAPrivateKey_ASN1(SSL *ssl, const uint8_t *der,
                                              size_t der_len);

// The following functions configure certificates or private keys but take as
// input files to read from. They return one on success and zero on failure. The
// |type| parameter is one of the |SSL_FILETYPE_*| values and determines whether
// the file's contents are read as PEM or DER.

#define SSL_FILETYPE_PEM 1
#define SSL_FILETYPE_ASN1 2

OPENSSL_EXPORT int SSL_CTX_use_RSAPrivateKey_file(SSL_CTX *ctx,
                                                  const char *file, int type);
OPENSSL_EXPORT int SSL_use_RSAPrivateKey_file(SSL *ssl, const char *file,
                                              int type);

OPENSSL_EXPORT int SSL_CTX_use_certificate_file(SSL_CTX *ctx, const char *file,
                                                int type);
OPENSSL_EXPORT int SSL_use_certificate_file(SSL *ssl, const char *file,
                                            int type);

OPENSSL_EXPORT int SSL_CTX_use_PrivateKey_file(SSL_CTX *ctx, const char *file,
                                               int type);
OPENSSL_EXPORT int SSL_use_PrivateKey_file(SSL *ssl, const char *file,
                                           int type);

// SSL_CTX_use_certificate_chain_file configures certificates for |ctx|. It
// reads the contents of |file| as a PEM-encoded leaf certificate followed
// optionally by the certificate chain to send to the peer. It returns one on
// success and zero on failure.
//
// WARNING: If the input contains "TRUSTED CERTIFICATE" PEM blocks, this
// function parses auxiliary properties as in |d2i_X509_AUX|. Passing untrusted
// input to this function allows an attacker to influence those properties. See
// |d2i_X509_AUX| for details.
OPENSSL_EXPORT int SSL_CTX_use_certificate_chain_file(SSL_CTX *ctx,
                                                      const char *file);

// SSL_use_certificate_chain_file configures certificates for |ssl|. It
// reads the contents of |file| as a PEM-encoded leaf certificate followed
// optionally by the certificate chain to send to the peer. It returns one on
// success and zero on failure.
OPENSSL_EXPORT int SSL_use_certificate_chain_file(SSL *ssl, const char *file);

// SSL_CTX_set_default_passwd_cb sets the password callback for PEM-based
// convenience functions called on |ctx|.
OPENSSL_EXPORT void SSL_CTX_set_default_passwd_cb(SSL_CTX *ctx,
                                                  pem_password_cb *cb);

// SSL_CTX_get_default_passwd_cb returns the callback set by
// |SSL_CTX_set_default_passwd_cb|.
OPENSSL_EXPORT pem_password_cb *SSL_CTX_get_default_passwd_cb(
    const SSL_CTX *ctx);

// SSL_CTX_set_default_passwd_cb_userdata sets the userdata parameter for
// |ctx|'s password callback.
OPENSSL_EXPORT void SSL_CTX_set_default_passwd_cb_userdata(SSL_CTX *ctx,
                                                           void *data);

// SSL_CTX_get_default_passwd_cb_userdata returns the userdata parameter set by
// |SSL_CTX_set_default_passwd_cb_userdata|.
OPENSSL_EXPORT void *SSL_CTX_get_default_passwd_cb_userdata(const SSL_CTX *ctx);


// Custom private keys.

enum ssl_private_key_result_t {
  ssl_private_key_success,
  ssl_private_key_retry,
  ssl_private_key_failure
};

// ssl_private_key_method_st (aka |SSL_PRIVATE_KEY_METHOD|) describes private
// key hooks. This is used to off-load signing operations to a custom,
// potentially asynchronous, backend. Metadata about the key such as the type
// and size are parsed out of the certificate.
//
// Callers that use this structure should additionally call
// |SSL_set_signing_algorithm_prefs| or |SSL_CTX_set_signing_algorithm_prefs|
// with the private key's capabilities. This ensures BoringSSL will select a
// suitable signature algorithm for the private key.
struct ssl_private_key_method_st {
  // sign signs the message |in| in using the specified signature algorithm. On
  // success, it returns |ssl_private_key_success| and writes at most |max_out|
  // bytes of signature data to |out| and sets |*out_len| to the number of bytes
  // written. On failure, it returns |ssl_private_key_failure|. If the operation
  // has not completed, it returns |ssl_private_key_retry|. |sign| should
  // arrange for the high-level operation on |ssl| to be retried when the
  // operation is completed. This will result in a call to |complete|.
  //
  // |signature_algorithm| is one of the |SSL_SIGN_*| values, as defined in TLS
  // 1.3. Note that, in TLS 1.2, ECDSA algorithms do not require that curve
  // sizes match hash sizes, so the curve portion of |SSL_SIGN_ECDSA_*| values
  // must be ignored. BoringSSL will internally handle the curve matching logic
  // where appropriate.
  //
  // It is an error to call |sign| while another private key operation is in
  // progress on |ssl|.
  enum ssl_private_key_result_t (*sign)(SSL *ssl, uint8_t *out, size_t *out_len,
                                        size_t max_out,
                                        uint16_t signature_algorithm,
                                        const uint8_t *in, size_t in_len);

  // decrypt decrypts |in_len| bytes of encrypted data from |in|. On success it
  // returns |ssl_private_key_success|, writes at most |max_out| bytes of
  // decrypted data to |out| and sets |*out_len| to the actual number of bytes
  // written. On failure it returns |ssl_private_key_failure|. If the operation
  // has not completed, it returns |ssl_private_key_retry|. The caller should
  // arrange for the high-level operation on |ssl| to be retried when the
  // operation is completed, which will result in a call to |complete|. This
  // function only works with RSA keys and should perform a raw RSA decryption
  // operation with no padding.
  //
  // It is an error to call |decrypt| while another private key operation is in
  // progress on |ssl|.
  enum ssl_private_key_result_t (*decrypt)(SSL *ssl, uint8_t *out,
                                           size_t *out_len, size_t max_out,
                                           const uint8_t *in, size_t in_len);

  // complete completes a pending operation. If the operation has completed, it
  // returns |ssl_private_key_success| and writes the result to |out| as in
  // |sign|. Otherwise, it returns |ssl_private_key_failure| on failure and
  // |ssl_private_key_retry| if the operation is still in progress.
  //
  // |complete| may be called arbitrarily many times before completion, but it
  // is an error to call |complete| if there is no pending operation in progress
  // on |ssl|.
  enum ssl_private_key_result_t (*complete)(SSL *ssl, uint8_t *out,
                                            size_t *out_len, size_t max_out);
};

// SSL_set_private_key_method configures a custom private key on |ssl|.
// |key_method| must remain valid for the lifetime of |ssl|. Using custom
// keys with the multiple certificate slots feature is not supported.
OPENSSL_EXPORT void SSL_set_private_key_method(
    SSL *ssl, const SSL_PRIVATE_KEY_METHOD *key_method);

// SSL_CTX_set_private_key_method configures a custom private key on |ctx|.
// |key_method| must remain valid for the lifetime of |ctx|.
OPENSSL_EXPORT void SSL_CTX_set_private_key_method(
    SSL_CTX *ctx, const SSL_PRIVATE_KEY_METHOD *key_method);

// SSL_can_release_private_key returns one if |ssl| will no longer call into the
// private key and zero otherwise. If the function returns one, the caller can
// release state associated with the private key.
//
// NOTE: This function assumes the caller does not use |SSL_clear| to reuse
// |ssl| for a second connection. If |SSL_clear| is used, BoringSSL may still
// use the private key on the second connection.
OPENSSL_EXPORT int SSL_can_release_private_key(const SSL *ssl);


// Cipher suites.
//
// |SSL_CIPHER| objects represent cipher suites.

DEFINE_CONST_STACK_OF(SSL_CIPHER)

// SSL_get_cipher_by_value returns the structure representing a TLS cipher
// suite based on its assigned number, or NULL if unknown. See
// https://www.iana.org/assignments/tls-parameters/tls-parameters.xhtml#tls-parameters-4.
OPENSSL_EXPORT const SSL_CIPHER *SSL_get_cipher_by_value(uint16_t value);

// SSL_CIPHER_find returns a SSL_CIPHER structure which has the cipher ID stored in ptr or
// NULL if unknown. The ptr parameter is a two element array of char, which stores the
// two-byte TLS cipher ID (as allocated by IANA) in network byte order. SSL_CIPHER_find re-casts
// |ptr| to uint16_t and calls |SSL_get_cipher_by_value| to get the SSL_CIPHER structure.
OPENSSL_EXPORT const SSL_CIPHER *SSL_CIPHER_find(SSL *ssl, const unsigned char *ptr);

// SSL_CIPHER_get_id returns |cipher|'s non-IANA id. This is not its
// IANA-assigned number, which is called the "value" here, although it may be
// cast to a |uint16_t| to get it.
OPENSSL_EXPORT uint32_t SSL_CIPHER_get_id(const SSL_CIPHER *cipher);

// SSL_CIPHER_get_protocol_id returns |cipher|'s IANA-assigned number.
OPENSSL_EXPORT uint16_t SSL_CIPHER_get_protocol_id(const SSL_CIPHER *cipher);

// SSL_CIPHER_is_aead returns one if |cipher| uses an AEAD cipher.
OPENSSL_EXPORT int SSL_CIPHER_is_aead(const SSL_CIPHER *cipher);

// SSL_CIPHER_is_block_cipher returns one if |cipher| is a block cipher.
OPENSSL_EXPORT int SSL_CIPHER_is_block_cipher(const SSL_CIPHER *cipher);

// SSL_CIPHER_get_cipher_nid returns the NID for |cipher|'s bulk
// cipher. Possible values are |NID_aes_128_gcm|, |NID_aes_256_gcm|,
// |NID_chacha20_poly1305|, |NID_aes_128_cbc|, |NID_aes_256_cbc|, and
// |NID_des_ede3_cbc|.
OPENSSL_EXPORT int SSL_CIPHER_get_cipher_nid(const SSL_CIPHER *cipher);

// SSL_CIPHER_get_digest_nid returns the NID for |cipher|'s HMAC if it is a
// legacy cipher suite. For modern AEAD-based ciphers (see
// |SSL_CIPHER_is_aead|), it returns |NID_undef|.
//
// Note this function only returns the legacy HMAC digest, not the PRF hash.
OPENSSL_EXPORT int SSL_CIPHER_get_digest_nid(const SSL_CIPHER *cipher);

// SSL_CIPHER_get_kx_nid returns the NID for |cipher|'s key exchange. This may
// be |NID_kx_rsa|, |NID_kx_ecdhe|, or |NID_kx_psk| for TLS 1.2. In TLS 1.3,
// cipher suites do not specify the key exchange, so this function returns
// |NID_kx_any|.
OPENSSL_EXPORT int SSL_CIPHER_get_kx_nid(const SSL_CIPHER *cipher);

// SSL_CIPHER_get_auth_nid returns the NID for |cipher|'s authentication
// type. This may be |NID_auth_rsa|, |NID_auth_ecdsa|, or |NID_auth_psk| for TLS
// 1.2. In TLS 1.3, cipher suites do not specify authentication, so this
// function returns |NID_auth_any|.
OPENSSL_EXPORT int SSL_CIPHER_get_auth_nid(const SSL_CIPHER *cipher);

// SSL_CIPHER_get_handshake_digest returns |cipher|'s PRF hash. If |cipher|
// is a pre-TLS-1.2 cipher, it returns |EVP_md5_sha1| but note these ciphers use
// SHA-256 in TLS 1.2. Other return values may be treated uniformly in all
// applicable versions.
OPENSSL_EXPORT const EVP_MD *SSL_CIPHER_get_handshake_digest(
    const SSL_CIPHER *cipher);

// SSL_CIPHER_get_prf_nid behaves like |SSL_CIPHER_get_handshake_digest| but
// returns the NID constant. Use |SSL_CIPHER_get_handshake_digest| instead.
OPENSSL_EXPORT int SSL_CIPHER_get_prf_nid(const SSL_CIPHER *cipher);

// SSL_CIPHER_get_min_version returns the minimum protocol version required
// for |cipher|.
OPENSSL_EXPORT uint16_t SSL_CIPHER_get_min_version(const SSL_CIPHER *cipher);

// SSL_CIPHER_get_max_version returns the maximum protocol version that
// supports |cipher|.
OPENSSL_EXPORT uint16_t SSL_CIPHER_get_max_version(const SSL_CIPHER *cipher);

// SSL_CIPHER_standard_name returns the standard IETF name for |cipher|. For
// example, "TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256".
OPENSSL_EXPORT const char *SSL_CIPHER_standard_name(const SSL_CIPHER *cipher);

// SSL_CIPHER_get_name returns the OpenSSL name of |cipher|. For example,
// "ECDHE-RSA-AES128-GCM-SHA256". Callers are recommended to use
// |SSL_CIPHER_standard_name| instead.
OPENSSL_EXPORT const char *SSL_CIPHER_get_name(const SSL_CIPHER *cipher);

// SSL_CIPHER_get_kx_name returns a string that describes the key-exchange
// method used by |cipher|. For example, "ECDHE_ECDSA". TLS 1.3 AEAD-only
// ciphers return the string "GENERIC".
OPENSSL_EXPORT const char *SSL_CIPHER_get_kx_name(const SSL_CIPHER *cipher);

// SSL_CIPHER_get_bits returns the strength, in bits, of |cipher|. If
// |out_alg_bits| is not NULL, it writes the number of bits consumed by the
// symmetric algorithm to |*out_alg_bits|.
OPENSSL_EXPORT int SSL_CIPHER_get_bits(const SSL_CIPHER *cipher,
                                       int *out_alg_bits);

// SSL_get_all_cipher_names outputs a list of possible strings
// |SSL_CIPHER_get_name| may return in this version of BoringSSL. It writes at
// most |max_out| entries to |out| and returns the total number it would have
// written, if |max_out| had been large enough. |max_out| may be initially set
// to zero to size the output.
//
// This function is only intended to help initialize tables in callers that want
// possible strings pre-declared. This list would not be suitable to set a list
// of supported features. It is in no particular order, and may contain
// placeholder, experimental, or deprecated values that do not apply to every
// caller. Future versions of BoringSSL may also return strings not in this
// list, so this does not apply if, say, sending strings across services.
OPENSSL_EXPORT size_t SSL_get_all_cipher_names(const char **out,
                                               size_t max_out);


// SSL_get_all_standard_cipher_names outputs a list of possible strings
// |SSL_CIPHER_standard_name| may return in this version of BoringSSL. It writes
// at most |max_out| entries to |out| and returns the total number it would have
// written, if |max_out| had been large enough. |max_out| may be initially set
// to zero to size the output.
//
// This function is only intended to help initialize tables in callers that want
// possible strings pre-declared. This list would not be suitable to set a list
// of supported features. It is in no particular order, and may contain
// placeholder, experimental, or deprecated values that do not apply to every
// caller. Future versions of BoringSSL may also return strings not in this
// list, so this does not apply if, say, sending strings across services.
OPENSSL_EXPORT size_t SSL_get_all_standard_cipher_names(const char **out,
                                                        size_t max_out);


// Cipher suite configuration.
//
// OpenSSL uses a mini-language to configure cipher suites. The language
// maintains an ordered list of enabled ciphers, along with an ordered list of
// disabled but available ciphers. Initially, all ciphers are disabled with a
// default ordering. The cipher string is then interpreted as a sequence of
// directives, separated by colons, each of which modifies this state.
//
// Most directives consist of a one character or empty opcode followed by a
// selector which matches a subset of available ciphers.
//
// Available opcodes are:
//
// - The empty opcode enables and appends all matching disabled ciphers to the
//   end of the enabled list. The newly appended ciphers are ordered relative to
//   each other matching their order in the disabled list.
//
// - |-| disables all matching enabled ciphers and prepends them to the disabled
//   list, with relative order from the enabled list preserved. This means the
//   most recently disabled ciphers get highest preference relative to other
//   disabled ciphers if re-enabled.
//
// - |+| moves all matching enabled ciphers to the end of the enabled list, with
//   relative order preserved.
//
// - |!| deletes all matching ciphers, enabled or not, from either list. Deleted
//   ciphers will not matched by future operations.
//
// A selector may be a specific cipher (using either the standard or OpenSSL
// name for the cipher) or one or more rules separated by |+|. The final
// selector matches the intersection of each rule. For instance, |AESGCM+aECDSA|
// matches ECDSA-authenticated AES-GCM ciphers.
//
// Available cipher rules are:
//
// - |ALL| matches all ciphers.
//
// - |kRSA|, |kDHE|, |kECDHE|, and |kPSK| match ciphers using plain RSA, DHE,
//   ECDHE, and plain PSK key exchanges, respectively. Note that ECDHE_PSK is
//   matched by |kECDHE| and not |kPSK|.
//
// - |aRSA|, |aECDSA|, and |aPSK| match ciphers authenticated by RSA, ECDSA, and
//   a pre-shared key, respectively.
//
// - |RSA|, |DHE|, |ECDHE|, |PSK|, |ECDSA|, and |PSK| are aliases for the
//   corresponding |k*| or |a*| cipher rule. |RSA| is an alias for |kRSA|, not
//   |aRSA|.
//
// - |3DES|, |AES128|, |AES256|, |AES|, |AESGCM|, |CHACHA20| match ciphers
//   whose bulk cipher use the corresponding encryption scheme. Note that
//   |AES|, |AES128|, and |AES256| match both CBC and GCM ciphers.
//
// - |SHA1|, and its alias |SHA|, match legacy cipher suites using HMAC-SHA1.
//
// Although implemented, authentication-only ciphers match no rules and must be
// explicitly selected by name.
//
// Deprecated cipher rules:
//
// - |kEDH|, |EDH|, |kEECDH|, and |EECDH| are legacy aliases for |kDHE|, |DHE|,
//   |kECDHE|, and |ECDHE|, respectively.
//
// - |HIGH| is an alias for |ALL|.
//
// - |FIPS| is an alias for |HIGH|.
//
// - |SSLv3| and |TLSv1| match ciphers available in TLS 1.1 or earlier.
//   |TLSv1_2| matches ciphers new in TLS 1.2. This is confusing and should not
//   be used.
//
// Unknown rules are silently ignored by legacy APIs, and rejected by APIs with
// "strict" in the name, which should be preferred. Cipher lists can be long
// and it's easy to commit typos. Strict functions will also reject the use of
// spaces, semi-colons and commas as alternative separators.
//
// The special |@STRENGTH| directive will sort all enabled ciphers by strength.
//
// The |DEFAULT| directive, when appearing at the front of the string, expands
// to the default ordering of available ciphers.
//
// For TLS < 1.3, if configuring a server, one may also configure
// equal-preference groups to partially respect the client's preferences when
// |SSL_OP_CIPHER_SERVER_PREFERENCE| is enabled. Ciphers in an equal-preference
// group have equal priority and use the client order. This may be used to
// enforce that AEADs are preferred but select AES-GCM vs. ChaCha20-Poly1305
// based on client preferences. An equal-preference is specified with square
// brackets, combining multiple selectors separated by |. For example:
//
//   [TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256|TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256]
//
// Once an equal-preference group is used, future directives must be
// opcode-less. Inside an equal-preference group, spaces are not allowed.

// SSL_DEFAULT_CIPHER_LIST is the default cipher suite configuration. It is
// substituted when a cipher string starts with 'DEFAULT'.
#define SSL_DEFAULT_CIPHER_LIST "ALL"


// SSL_CTX_set_strict_cipher_list configures the cipher list for |ctx|,
// evaluating |str| as a cipher string and returning error if |str| contains
// anything meaningless. It updates |ctx->cipher_list| with any values in
// |ctx->tls13_cipher_list|.
//
// It returns one on success and zero on failure.
OPENSSL_EXPORT int SSL_CTX_set_strict_cipher_list(SSL_CTX *ctx,
                                                  const char *str);

// SSL_CTX_set_cipher_list configures the cipher list for |ctx|, evaluating
// |str| as a cipher string. It updates |ctx->cipher_list| with any values in
// |ctx->tls13_cipher_list|. It returns one on success and zero on failure.
//
// Prefer to use |SSL_CTX_set_strict_cipher_list|. This function tolerates
// garbage inputs, unless an empty cipher list results. However, an empty
// string which also results in an empty cipher list, is allowed. This
// behavior is strongly advised against and only meant for OpenSSL
// compatibility.
//
// Note: this API only sets the TLSv1.2 and below ciphers.
// Use |SSL_CTX_set_ciphersuites| to configure TLS 1.3 specific ciphers.
OPENSSL_EXPORT int SSL_CTX_set_cipher_list(SSL_CTX *ctx, const char *str);

// SSL_set_strict_cipher_list configures the cipher list for |ssl|, evaluating
// |str| as a cipher string and returning error if |str| contains anything
// meaningless.
// It updates the cipher list |ssl->config->cipher_list| with any configured
// TLS 1.3 cipher suites by first checking |ssl->config->tls13_cipher_list| and
// otherwise falling back to |ssl->ctx->tls13_cipher_list|.
//
// It returns one on success and zero on failure.
OPENSSL_EXPORT int SSL_set_strict_cipher_list(SSL *ssl, const char *str);

// SSL_CTX_set_ciphersuites configures the available TLSv1.3 ciphersuites on
// |ctx|, evaluating |str| as a cipher string. It updates |ctx->cipher_list|
// with any values in |ctx->tls13_cipher_list|. It returns one on success and
// zero on failure.
OPENSSL_EXPORT int SSL_CTX_set_ciphersuites(SSL_CTX *ctx, const char *str);

// SSL_set_ciphersuites configures the available TLSv1.3 ciphersuites on
// |ssl|, evaluating |str| as a cipher string. It updates
// |ssl->config->cipher_list| with any values in
// |ssl->config->tls13_cipher_list|. It returns one on success and zero on
// failure.
OPENSSL_EXPORT int SSL_set_ciphersuites(SSL *ssl, const char *str);

// SSL_set_cipher_list configures the cipher list for |ssl|, evaluating |str| as
// a cipher string. It updates the cipher list |ssl->config->cipher_list| with
// any configured TLS 1.3 cipher suites by first checking
// |ssl->config->tls13_cipher_list| and otherwise falling back to
// |ssl->ctx->tls13_cipher_list|.
//
// Equal-preference groups cannot be configured for TLS 1.3 through these APIs.
//
// It returns one on success and zero on failure.
//
// Prefer to use |SSL_set_strict_cipher_list|. This function tolerates garbage
// inputs, unless an empty cipher list results. However, an empty string which
// also results in an empty cipher list, is allowed. This behavior is strongly
// advised against and only meant for OpenSSL compatibility.
OPENSSL_EXPORT int SSL_set_cipher_list(SSL *ssl, const char *str);

// SSL_CTX_get_ciphers returns the cipher list for |ctx|, in order of
// preference. This includes TLS 1.3 and 1.2 and below cipher suites.
OPENSSL_EXPORT STACK_OF(SSL_CIPHER) *SSL_CTX_get_ciphers(const SSL_CTX *ctx);

// SSL_CTX_cipher_in_group returns one if the |i|th cipher (see
// |SSL_CTX_get_ciphers|) is in the same equipreference group as the one
// following it and zero otherwise.
OPENSSL_EXPORT int SSL_CTX_cipher_in_group(const SSL_CTX *ctx, size_t i);

// SSL_get_ciphers returns the cipher list for |ssl|, in order of preference.
// This includes TLS 1.3 and 1.2 and below cipher suites.
OPENSSL_EXPORT STACK_OF(SSL_CIPHER) *SSL_get_ciphers(const SSL *ssl);


// Connection information.

// SSL_in_connect_init returns 1 if |ssl| is a client and has a pending
// handshake. Otherwise, it returns 0.
OPENSSL_EXPORT int SSL_in_connect_init(const SSL *ssl);

// SSL_in_accept_init returns 1 if |ssl| is a server and has a pending
// handshake. Otherwise, it returns 0.
OPENSSL_EXPORT int SSL_in_accept_init(const SSL *ssl);

// SSL_is_init_finished returns one if |ssl| has completed its initial handshake
// and has no pending handshake. It returns zero otherwise.
OPENSSL_EXPORT int SSL_is_init_finished(const SSL *ssl);

// SSL_in_init returns one if |ssl| has a pending handshake and zero
// otherwise.
OPENSSL_EXPORT int SSL_in_init(const SSL *ssl);

// SSL_in_false_start returns one if |ssl| has a pending handshake that is in
// False Start. |SSL_write| may be called at this point without waiting for the
// peer, but |SSL_read| will complete the handshake before accepting application
// data.
//
// See also |SSL_MODE_ENABLE_FALSE_START|.
OPENSSL_EXPORT int SSL_in_false_start(const SSL *ssl);

// SSL_get_peer_certificate returns the peer's leaf certificate or NULL if the
// peer did not use certificates. The caller must call |X509_free| on the
// result to release it.
OPENSSL_EXPORT X509 *SSL_get_peer_certificate(const SSL *ssl);

// SSL_get_peer_cert_chain returns the peer's certificate chain or NULL if
// unavailable or the peer did not use certificates. This is the unverified list
// of certificates as sent by the peer, not the final chain built during
// verification. The caller does not take ownership of the result.
//
// WARNING: This function behaves differently between client and server. If
// |ssl| is a server, the returned chain does not include the leaf certificate.
// If a client, it does.
OPENSSL_EXPORT STACK_OF(X509) *SSL_get_peer_cert_chain(const SSL *ssl);

// SSL_get_peer_full_cert_chain returns the peer's certificate chain, or NULL if
// unavailable or the peer did not use certificates. This is the unverified list
// of certificates as sent by the peer, not the final chain built during
// verification. The caller does not take ownership of the result.
//
// This is the same as |SSL_get_peer_cert_chain| except that this function
// always returns the full chain, i.e. the first element of the return value
// (if any) will be the leaf certificate. In contrast,
// |SSL_get_peer_cert_chain| returns only the intermediate certificates if the
// |ssl| is a server.
OPENSSL_EXPORT STACK_OF(X509) *SSL_get_peer_full_cert_chain(const SSL *ssl);

// SSL_get0_verified_chain returns the verified certificate chain of the peer
// including the peer's end entity certificate. It must be called after a
// session has been successfully established. If peer verification was not
// successful (as indicated by |SSL_get_verify_result| not returning
// |X509_V_OK|) the result will be null. If a verification callback was set with
// |SSL_CTX_set_cert_verify_callback| or |SSL_set_custom_verify|
// this function's behavior is undefined.
OPENSSL_EXPORT STACK_OF(X509) *SSL_get0_verified_chain(const SSL *ssl);

// SSL_get0_peer_certificates returns the peer's certificate chain, or NULL if
// unavailable or the peer did not use certificates. This is the unverified list
// of certificates as sent by the peer, not the final chain built during
// verification. The caller does not take ownership of the result.
//
// This is the |CRYPTO_BUFFER| variant of |SSL_get_peer_full_cert_chain|.
OPENSSL_EXPORT const STACK_OF(CRYPTO_BUFFER) *SSL_get0_peer_certificates(
    const SSL *ssl);

// SSL_get0_signed_cert_timestamp_list sets |*out| and |*out_len| to point to
// |*out_len| bytes of SCT information from the server. This is only valid if
// |ssl| is a client. The SCT information is a SignedCertificateTimestampList
// (including the two leading length bytes).
// See https://tools.ietf.org/html/rfc6962#section-3.3
// If no SCT was received then |*out_len| will be zero on return.
//
// WARNING: the returned data is not guaranteed to be well formed.
OPENSSL_EXPORT void SSL_get0_signed_cert_timestamp_list(const SSL *ssl,
                                                        const uint8_t **out,
                                                        size_t *out_len);

// SSL_get0_ocsp_response sets |*out| and |*out_len| to point to |*out_len|
// bytes of an OCSP response from the server. This is the DER encoding of an
// OCSPResponse type as defined in RFC 2560.
//
// WARNING: the returned data is not guaranteed to be well formed.
OPENSSL_EXPORT void SSL_get0_ocsp_response(const SSL *ssl, const uint8_t **out,
                                           size_t *out_len);

// SSL_get_tls_unique writes at most |max_out| bytes of the tls-unique value
// for |ssl| to |out| and sets |*out_len| to the number of bytes written. It
// returns one on success or zero on error. In general |max_out| should be at
// least 12.
//
// This function will always fail if the initial handshake has not completed.
// The tls-unique value will change after a renegotiation but, since
// renegotiations can be initiated by the server at any point, the higher-level
// protocol must either leave them disabled or define states in which the
// tls-unique value can be read.
//
// The tls-unique value is defined by
// https://tools.ietf.org/html/rfc5929#section-3.1. Due to a weakness in the
// TLS protocol, tls-unique is broken for resumed connections unless the
// Extended Master Secret extension is negotiated. Thus this function will
// return zero if |ssl| performed session resumption unless EMS was used when
// negotiating the original session.
OPENSSL_EXPORT int SSL_get_tls_unique(const SSL *ssl, uint8_t *out,
                                      size_t *out_len, size_t max_out);

// SSL_get_extms_support returns one if the Extended Master Secret extension or
// TLS 1.3 was negotiated. Otherwise, it returns zero.
OPENSSL_EXPORT int SSL_get_extms_support(const SSL *ssl);

// SSL_get_current_cipher returns cipher suite used by |ssl|, or NULL if it has
// not been negotiated yet.
OPENSSL_EXPORT const SSL_CIPHER *SSL_get_current_cipher(const SSL *ssl);

// SSL_get_client_ciphers returns the stack of ciphers offered by the client
// during a handshake and that are supported by this library. If |ssl| is a
// client or the handshake hasn't occurred yet, NULL is returned. The stack of
// ciphers IS NOT de/serialized, so NULL will also be returned for deserialized
// or transported |ssl|'s that haven't yet performed a new handshake.
OPENSSL_EXPORT STACK_OF(SSL_CIPHER) *SSL_get_client_ciphers(const SSL *ssl);

// SSL_client_hello_get0_ciphers provides access to the client ciphers field from the
// Client Hello, optionally writing the result to an out pointer. It returns the field
// length if successful, or 0 if |ssl| is a client or the handshake hasn't occurred yet.
// |out| points to the raw bytes from the client hello message so it may contain invalid
// or unsupported Cipher IDs.
OPENSSL_EXPORT size_t SSL_client_hello_get0_ciphers(SSL *ssl, const unsigned char **out);

// SSL_session_reused returns one if |ssl| performed an abbreviated handshake
// and zero otherwise.
//
// TODO(davidben): Hammer down the semantics of this API while a handshake,
// initial or renego, is in progress.
OPENSSL_EXPORT int SSL_session_reused(const SSL *ssl);

// SSL_get_secure_renegotiation_support returns one if the peer supports secure
// renegotiation (RFC 5746) or TLS 1.3. Otherwise, it returns zero.
OPENSSL_EXPORT int SSL_get_secure_renegotiation_support(const SSL *ssl);

// SSL_export_keying_material exports a value derived from the master secret, as
// specified in RFC 5705. It writes |out_len| bytes to |out| given a label and
// optional context. (Since a zero length context is allowed, the |use_context|
// flag controls whether a context is included.)
//
// It returns one on success and zero otherwise.
OPENSSL_EXPORT int SSL_export_keying_material(
    SSL *ssl, uint8_t *out, size_t out_len, const char *label, size_t label_len,
    const uint8_t *context, size_t context_len, int use_context);


// Custom extensions.
//
// The custom extension functions allow TLS extensions to be added to
// ClientHello and ServerHello messages.

// SSL_custom_ext_add_cb is a callback function that is called when the
// ClientHello (for clients) or ServerHello (for servers) is constructed. In
// the case of a server, this callback will only be called for a given
// extension if the ClientHello contained that extension – it's not possible to
// inject extensions into a ServerHello that the client didn't request.
//
// When called, |extension_value| will contain the extension number that is
// being considered for addition (so that a single callback can handle multiple
// extensions). If the callback wishes to include the extension, it must set
// |*out| to point to |*out_len| bytes of extension contents and return one. In
// this case, the corresponding |SSL_custom_ext_free_cb| callback will later be
// called with the value of |*out| once that data has been copied.
//
// If the callback does not wish to add an extension it must return zero.
//
// Alternatively, the callback can abort the connection by setting
// |*out_alert_value| to a TLS alert number and returning -1.
typedef int (*SSL_custom_ext_add_cb)(SSL *ssl, unsigned extension_value,
                                     const uint8_t **out, size_t *out_len,
                                     int *out_alert_value, void *add_arg);

// SSL_custom_ext_free_cb is a callback function that is called by AWS-LC iff
// an |SSL_custom_ext_add_cb| callback previously returned one. In that case,
// this callback is called and passed the |out| pointer that was returned by
// the add callback. This is to free any dynamically allocated data created by
// the add callback.
typedef void (*SSL_custom_ext_free_cb)(SSL *ssl, unsigned extension_value,
                                       const uint8_t *out, void *add_arg);

// SSL_custom_ext_parse_cb is a callback function that is called by AWS-LC to
// parse an extension from the peer: that is from the ServerHello for a client
// and from the ClientHello for a server.
//
// When called, |extension_value| will contain the extension number and the
// contents of the extension are |contents_len| bytes at |contents|.
//
// The callback must return one to continue the handshake. Otherwise, if it
// returns zero, a fatal alert with value |*out_alert_value| is sent and the
// handshake is aborted.
typedef int (*SSL_custom_ext_parse_cb)(SSL *ssl, unsigned extension_value,
                                       const uint8_t *contents,
                                       size_t contents_len,
                                       int *out_alert_value, void *parse_arg);

// SSL_extension_supported returns one iff AWS-LC internally handles
// extensions of type |extension_value|. This can be used to avoid registering
// custom extension handlers for extensions that a future version of AWS-LC
// may handle internally.
OPENSSL_EXPORT int SSL_extension_supported(unsigned extension_value);

// SSL_CTX_add_client_custom_ext registers callback functions for handling
// custom TLS extensions for client connections.
//
// If |add_cb| is NULL then an empty extension will be added in each
// ClientHello. Otherwise, see the comment for |SSL_custom_ext_add_cb| about
// this callback.
//
// The |free_cb| may be NULL if |add_cb| doesn't dynamically allocate data that
// needs to be freed.
//
// It returns one on success or zero on error. It's always an error to register
// callbacks for the same extension twice, or to register callbacks for an
// extension that AWS-LC handles internally. See |SSL_extension_supported| to
// discover, at runtime, which extensions AWS-LC handles internally.
OPENSSL_EXPORT int SSL_CTX_add_client_custom_ext(
    SSL_CTX *ctx, unsigned extension_value, SSL_custom_ext_add_cb add_cb,
    SSL_custom_ext_free_cb free_cb, void *add_arg,
    SSL_custom_ext_parse_cb parse_cb, void *parse_arg);

// SSL_CTX_add_server_custom_ext is the same as
// |SSL_CTX_add_client_custom_ext|, but for server connections.
//
// Unlike on the client side, if |add_cb| is NULL no extension will be added.
// The |add_cb|, if any, will only be called if the ClientHello contained a
// matching extension.
OPENSSL_EXPORT int SSL_CTX_add_server_custom_ext(
    SSL_CTX *ctx, unsigned extension_value, SSL_custom_ext_add_cb add_cb,
    SSL_custom_ext_free_cb free_cb, void *add_arg,
    SSL_custom_ext_parse_cb parse_cb, void *parse_arg);


// Sessions.
//
// An |SSL_SESSION| represents an SSL session that may be resumed in an
// abbreviated handshake. It is reference-counted and immutable. Once
// established, an |SSL_SESSION| may be shared by multiple |SSL| objects on
// different threads and must not be modified.
//
// Note the TLS notion of "session" is not suitable for application-level
// session state. It is an optional caching mechanism for the handshake. Not all
// connections within an application-level session will reuse TLS sessions. TLS
// sessions may be dropped by the client or ignored by the server at any time.

DECLARE_PEM_rw(SSL_SESSION, SSL_SESSION)

// SSL_SESSION_new returns a newly-allocated blank |SSL_SESSION| or NULL on
// error. This may be useful when writing tests but should otherwise not be
// used.
OPENSSL_EXPORT SSL_SESSION *SSL_SESSION_new(const SSL_CTX *ctx);

// SSL_SESSION_up_ref increments the reference count of |session| and returns
// one.
OPENSSL_EXPORT int SSL_SESSION_up_ref(SSL_SESSION *session);

// SSL_SESSION_free decrements the reference count of |session|. If it reaches
// zero, all data referenced by |session| and |session| itself are released.
OPENSSL_EXPORT void SSL_SESSION_free(SSL_SESSION *session);

// SSL_SESSION_to_bytes serializes |in| into a newly allocated buffer and sets
// |*out_data| to that buffer and |*out_len| to its length. The caller takes
// ownership of the buffer and must call |OPENSSL_free| when done. It returns
// one on success and zero on error.
OPENSSL_EXPORT int SSL_SESSION_to_bytes(const SSL_SESSION *in,
                                        uint8_t **out_data, size_t *out_len);

// SSL_SESSION_to_bytes_for_ticket serializes |in|, but excludes the session
// identification information, namely the session ID and ticket.
OPENSSL_EXPORT int SSL_SESSION_to_bytes_for_ticket(const SSL_SESSION *in,
                                                   uint8_t **out_data,
                                                   size_t *out_len);

// SSL_SESSION_from_bytes parses |in_len| bytes from |in| as an SSL_SESSION. It
// returns a newly-allocated |SSL_SESSION| on success or NULL on error.
OPENSSL_EXPORT SSL_SESSION *SSL_SESSION_from_bytes(const uint8_t *in,
                                                   size_t in_len,
                                                   const SSL_CTX *ctx);

// SSL_SESSION_get_version returns a string describing the TLS or DTLS version
// |session| was established at. For example, "TLSv1.2" or "DTLSv1".
OPENSSL_EXPORT const char *SSL_SESSION_get_version(const SSL_SESSION *session);

// SSL_SESSION_get_protocol_version returns the TLS or DTLS version |session|
// was established at.
OPENSSL_EXPORT uint16_t
SSL_SESSION_get_protocol_version(const SSL_SESSION *session);

// SSL_SESSION_set_protocol_version sets |session|'s TLS or DTLS version to
// |version|. This may be useful when writing tests but should otherwise not be
// used. It returns one on success and zero on error.
OPENSSL_EXPORT int SSL_SESSION_set_protocol_version(SSL_SESSION *session,
                                                    uint16_t version);

// SSL_MAX_SSL_SESSION_ID_LENGTH is the maximum length of an SSL session ID.
#define SSL_MAX_SSL_SESSION_ID_LENGTH 32

// SSL_SESSION_get_id returns a pointer to a buffer containing |session|'s
// session ID and sets |*out_len| to its length.
//
// This function should only be used for implementing a TLS session cache. TLS
// sessions are not suitable for application-level session state, and a session
// ID is an implementation detail of the TLS resumption handshake mechanism. Not
// all resumption flows use session IDs, and not all connections within an
// application-level session will reuse TLS sessions.
//
// To determine if resumption occurred, use |SSL_session_reused| instead.
// Comparing session IDs will not give the right result in all cases.
//
// As a workaround for some broken applications, BoringSSL sometimes synthesizes
// arbitrary session IDs for non-ID-based sessions. This behavior may be
// removed in the future.
OPENSSL_EXPORT const uint8_t *SSL_SESSION_get_id(const SSL_SESSION *session,
                                                 unsigned *out_len);

// SSL_SESSION_set1_id sets |session|'s session ID to |sid|, It returns one on
// success and zero on error. This function may be useful in writing tests but
// otherwise should not be used.
OPENSSL_EXPORT int SSL_SESSION_set1_id(SSL_SESSION *session, const uint8_t *sid,
                                       size_t sid_len);

// SSL_SESSION_get_time returns the time at which |session| was established in
// seconds since the UNIX epoch.
OPENSSL_EXPORT uint64_t SSL_SESSION_get_time(const SSL_SESSION *session);

// SSL_SESSION_get_timeout returns the lifetime of |session| in seconds.
OPENSSL_EXPORT uint32_t SSL_SESSION_get_timeout(const SSL_SESSION *session);

// SSL_SESSION_get0_peer returns the peer leaf certificate stored in
// |session|.
//
// TODO(davidben): This should return a const X509 *.
OPENSSL_EXPORT X509 *SSL_SESSION_get0_peer(const SSL_SESSION *session);

// SSL_SESSION_get0_peer_certificates returns the peer certificate chain stored
// in |session|, or NULL if the peer did not use certificates. This is the
// unverified list of certificates as sent by the peer, not the final chain
// built during verification. The caller does not take ownership of the result.
OPENSSL_EXPORT const STACK_OF(CRYPTO_BUFFER) *
SSL_SESSION_get0_peer_certificates(const SSL_SESSION *session);

// SSL_SESSION_get0_signed_cert_timestamp_list sets |*out| and |*out_len| to
// point to |*out_len| bytes of SCT information stored in |session|. This is
// only valid for client sessions. The SCT information is a
// SignedCertificateTimestampList (including the two leading length bytes). See
// https://tools.ietf.org/html/rfc6962#section-3.3 If no SCT was received then
// |*out_len| will be zero on return.
//
// WARNING: the returned data is not guaranteed to be well formed.
OPENSSL_EXPORT void SSL_SESSION_get0_signed_cert_timestamp_list(
    const SSL_SESSION *session, const uint8_t **out, size_t *out_len);

// SSL_SESSION_get0_ocsp_response sets |*out| and |*out_len| to point to
// |*out_len| bytes of an OCSP response from the server. This is the DER
// encoding of an OCSPResponse type as defined in RFC 2560.
//
// WARNING: the returned data is not guaranteed to be well formed.
OPENSSL_EXPORT void SSL_SESSION_get0_ocsp_response(const SSL_SESSION *session,
                                                   const uint8_t **out,
                                                   size_t *out_len);

// SSL_MAX_MASTER_KEY_LENGTH is the maximum length of a master secret.
#define SSL_MAX_MASTER_KEY_LENGTH 48

// SSL_SESSION_get_master_key writes up to |max_out| bytes of |session|'s secret
// to |out| and returns the number of bytes written. If |max_out| is zero, it
// returns the size of the secret.
OPENSSL_EXPORT size_t SSL_SESSION_get_master_key(const SSL_SESSION *session,
                                                 uint8_t *out, size_t max_out);

// SSL_SESSION_set_time sets |session|'s creation time to |time| and returns
// |time|. This function may be useful in writing tests but otherwise should not
// be used.
OPENSSL_EXPORT uint64_t SSL_SESSION_set_time(SSL_SESSION *session,
                                             uint64_t time);

// SSL_SESSION_set_timeout sets |session|'s timeout to |timeout| and returns
// one. This function may be useful in writing tests but otherwise should not
// be used.
OPENSSL_EXPORT uint32_t SSL_SESSION_set_timeout(SSL_SESSION *session,
                                                uint32_t timeout);

// SSL_SESSION_get0_id_context returns a pointer to a buffer containing
// |session|'s session ID context (see |SSL_CTX_set_session_id_context|) and
// sets |*out_len| to its length.
OPENSSL_EXPORT const uint8_t *SSL_SESSION_get0_id_context(
    const SSL_SESSION *session, unsigned *out_len);

// SSL_SESSION_set1_id_context sets |session|'s session ID context (see
// |SSL_CTX_set_session_id_context|) to |sid_ctx|. It returns one on success and
// zero on error. This function may be useful in writing tests but otherwise
// should not be used.
OPENSSL_EXPORT int SSL_SESSION_set1_id_context(SSL_SESSION *session,
                                               const uint8_t *sid_ctx,
                                               size_t sid_ctx_len);

// SSL_SESSION_should_be_single_use returns one if |session| should be
// single-use (TLS 1.3 and later) and zero otherwise.
//
// If this function returns one, clients retain multiple sessions and use each
// only once. This prevents passive observers from correlating connections with
// tickets. See RFC 8446, appendix C.4. If it returns zero, |session| cannot be
// used without leaking a correlator.
OPENSSL_EXPORT int SSL_SESSION_should_be_single_use(const SSL_SESSION *session);

// SSL_SESSION_is_resumable returns one if |session| is complete and contains a
// session ID or ticket. It returns zero otherwise. Note this function does not
// ensure |session| will be resumed. It may be expired, dropped by the server,
// or associated with incompatible parameters.
OPENSSL_EXPORT int SSL_SESSION_is_resumable(const SSL_SESSION *session);

// SSL_SESSION_has_ticket returns one if |session| has a ticket and zero
// otherwise.
OPENSSL_EXPORT int SSL_SESSION_has_ticket(const SSL_SESSION *session);

// SSL_SESSION_get0_ticket sets |*out_ticket| and |*out_len| to |session|'s
// ticket, or NULL and zero if it does not have one. |out_ticket| may be NULL
// if only the ticket length is needed.
OPENSSL_EXPORT void SSL_SESSION_get0_ticket(const SSL_SESSION *session,
                                            const uint8_t **out_ticket,
                                            size_t *out_len);

// SSL_SESSION_set_ticket sets |session|'s ticket to |ticket|. It returns one on
// success and zero on error. This function may be useful in writing tests but
// otherwise should not be used.
OPENSSL_EXPORT int SSL_SESSION_set_ticket(SSL_SESSION *session,
                                          const uint8_t *ticket,
                                          size_t ticket_len);

// SSL_SESSION_get_ticket_lifetime_hint returns ticket lifetime hint of
// |session| in seconds or zero if none was set.
OPENSSL_EXPORT uint32_t
SSL_SESSION_get_ticket_lifetime_hint(const SSL_SESSION *session);

// SSL_SESSION_get0_cipher returns the cipher negotiated by the connection which
// established |session|.
//
// Note that, in TLS 1.3, there is no guarantee that resumptions with |session|
// will use that cipher. Prefer calling |SSL_get_current_cipher| on the |SSL|
// instead.
OPENSSL_EXPORT const SSL_CIPHER *SSL_SESSION_get0_cipher(
    const SSL_SESSION *session);

// SSL_SESSION_has_peer_sha256 returns one if |session| has a SHA-256 hash of
// the peer's certificate retained and zero if the peer did not present a
// certificate or if this was not enabled when |session| was created. See also
// |SSL_CTX_set_retain_only_sha256_of_client_certs|.
OPENSSL_EXPORT int SSL_SESSION_has_peer_sha256(const SSL_SESSION *session);

// SSL_SESSION_get0_peer_sha256 sets |*out_ptr| and |*out_len| to the SHA-256
// hash of the peer certificate retained in |session|, or NULL and zero if it
// does not have one. See also |SSL_CTX_set_retain_only_sha256_of_client_certs|.
OPENSSL_EXPORT void SSL_SESSION_get0_peer_sha256(const SSL_SESSION *session,
                                                 const uint8_t **out_ptr,
                                                 size_t *out_len);


// Session caching.
//
// Session caching allows connections to be established more efficiently based
// on saved parameters from a previous connection, called a session (see
// |SSL_SESSION|). The client offers a saved session, using an opaque identifier
// from a previous connection. The server may accept the session, if it has the
// parameters available. Otherwise, it will decline and continue with a full
// handshake.
//
// This requires both the client and the server to retain session state. A
// client does so with a stateful session cache. A server may do the same or, if
// supported by both sides, statelessly using session tickets. For more
// information on the latter, see the next section.
//
// For a server, the library implements a built-in internal session cache as an
// in-memory hash table. Servers may also use |SSL_CTX_sess_set_get_cb| and
// |SSL_CTX_sess_set_new_cb| to implement a custom external session cache. In
// particular, this may be used to share a session cache between multiple
// servers in a large deployment. An external cache may be used in addition to
// or instead of the internal one. Use |SSL_CTX_set_session_cache_mode| to
// toggle the internal cache.
//
// For a client, the only option is an external session cache. Clients may use
// |SSL_CTX_sess_set_new_cb| to register a callback for when new sessions are
// available. These may be cached and, in subsequent compatible connections,
// configured with |SSL_set_session|.
//
// Note that offering or accepting a session short-circuits certificate
// verification and most parameter negotiation. Resuming sessions across
// different contexts may result in security failures and surprising
// behavior. For a typical client, this means sessions for different hosts must
// be cached under different keys. A client that connects to the same host with,
// e.g., different cipher suite settings or client certificates should also use
// separate session caches between those contexts. Servers should also partition
// session caches between SNI hosts with |SSL_CTX_set_session_id_context|.
//
// Note also, in TLS 1.2 and earlier, offering sessions allows passive observers
// to correlate different client connections. TLS 1.3 and later fix this,
// provided clients use sessions at most once. Session caches are managed by the
// caller in BoringSSL, so this must be implemented externally. See
// |SSL_SESSION_should_be_single_use| for details.

// SSL_SESS_CACHE_OFF disables all session caching.
#define SSL_SESS_CACHE_OFF 0x0000

// SSL_SESS_CACHE_CLIENT enables session caching for a client. The internal
// cache is never used on a client, so this only enables the callbacks.
#define SSL_SESS_CACHE_CLIENT 0x0001

// SSL_SESS_CACHE_SERVER enables session caching for a server.
#define SSL_SESS_CACHE_SERVER 0x0002

// SSL_SESS_CACHE_BOTH enables session caching for both client and server.
#define SSL_SESS_CACHE_BOTH (SSL_SESS_CACHE_CLIENT | SSL_SESS_CACHE_SERVER)

// SSL_SESS_CACHE_NO_AUTO_CLEAR disables automatically calling
// |SSL_CTX_flush_sessions| every 255 connections.
#define SSL_SESS_CACHE_NO_AUTO_CLEAR 0x0080

// SSL_SESS_CACHE_NO_INTERNAL_LOOKUP, on a server, disables looking up a session
// from the internal session cache.
#define SSL_SESS_CACHE_NO_INTERNAL_LOOKUP 0x0100

// SSL_SESS_CACHE_NO_INTERNAL_STORE, on a server, disables storing sessions in
// the internal session cache.
#define SSL_SESS_CACHE_NO_INTERNAL_STORE 0x0200

// SSL_SESS_CACHE_NO_INTERNAL, on a server, disables the internal session
// cache.
#define SSL_SESS_CACHE_NO_INTERNAL \
  (SSL_SESS_CACHE_NO_INTERNAL_LOOKUP | SSL_SESS_CACHE_NO_INTERNAL_STORE)

// SSL_CTX_set_session_cache_mode sets the session cache mode bits for |ctx| to
// |mode|. It returns the previous value.
OPENSSL_EXPORT int SSL_CTX_set_session_cache_mode(SSL_CTX *ctx, int mode);

// SSL_CTX_get_session_cache_mode returns the session cache mode bits for
// |ctx|
OPENSSL_EXPORT int SSL_CTX_get_session_cache_mode(const SSL_CTX *ctx);

// SSL_set_session, for a client, configures |ssl| to offer to resume |session|
// in the initial handshake and returns one. The caller retains ownership of
// |session|. Note that configuring a session assumes the authentication in the
// session is valid. For callers that wish to revalidate the session before
// offering, see |SSL_SESSION_get0_peer_certificates|,
// |SSL_SESSION_get0_signed_cert_timestamp_list|, and
// |SSL_SESSION_get0_ocsp_response|.
//
// It is an error to call this function after the handshake has begun.
OPENSSL_EXPORT int SSL_set_session(SSL *ssl, SSL_SESSION *session);

// SSL_DEFAULT_SESSION_TIMEOUT is the default lifetime, in seconds, of a
// session in TLS 1.2 or earlier. This is how long we are willing to use the
// secret to encrypt traffic without fresh key material.
#define SSL_DEFAULT_SESSION_TIMEOUT (2 * 60 * 60)

// SSL_DEFAULT_SESSION_PSK_DHE_TIMEOUT is the default lifetime, in seconds, of a
// session for TLS 1.3 psk_dhe_ke. This is how long we are willing to use the
// secret as an authenticator.
#define SSL_DEFAULT_SESSION_PSK_DHE_TIMEOUT (2 * 24 * 60 * 60)

// SSL_DEFAULT_SESSION_AUTH_TIMEOUT is the default non-renewable lifetime, in
// seconds, of a TLS 1.3 session. This is how long we are willing to trust the
// signature in the initial handshake.
#define SSL_DEFAULT_SESSION_AUTH_TIMEOUT (7 * 24 * 60 * 60)

// SSL_CTX_set_timeout sets the lifetime, in seconds, of TLS 1.2 (or earlier)
// sessions created in |ctx| to |timeout|.
OPENSSL_EXPORT uint32_t SSL_CTX_set_timeout(SSL_CTX *ctx, uint32_t timeout);

// SSL_CTX_set_session_psk_dhe_timeout sets the lifetime, in seconds, of TLS 1.3
// sessions created in |ctx| to |timeout|.
OPENSSL_EXPORT void SSL_CTX_set_session_psk_dhe_timeout(SSL_CTX *ctx,
                                                        uint32_t timeout);

// SSL_CTX_get_timeout returns the lifetime, in seconds, of TLS 1.2 (or earlier)
// sessions created in |ctx|.
OPENSSL_EXPORT uint32_t SSL_CTX_get_timeout(const SSL_CTX *ctx);

// SSL_MAX_SID_CTX_LENGTH is the maximum length of a session ID context.
#define SSL_MAX_SID_CTX_LENGTH 32

// SSL_CTX_set_session_id_context sets |ctx|'s session ID context to |sid_ctx|.
// It returns one on success and zero on error. The session ID context is an
// application-defined opaque byte string. A session will not be used in a
// connection without a matching session ID context.
//
// For a server, if |SSL_VERIFY_PEER| is enabled, it is an error to not set a
// session ID context.
OPENSSL_EXPORT int SSL_CTX_set_session_id_context(SSL_CTX *ctx,
                                                  const uint8_t *sid_ctx,
                                                  size_t sid_ctx_len);

// SSL_set_session_id_context sets |ssl|'s session ID context to |sid_ctx|. It
// returns one on success and zero on error. See also
// |SSL_CTX_set_session_id_context|.
OPENSSL_EXPORT int SSL_set_session_id_context(SSL *ssl, const uint8_t *sid_ctx,
                                              size_t sid_ctx_len);

// SSL_get0_session_id_context returns a pointer to |ssl|'s session ID context
// and sets |*out_len| to its length.  It returns NULL on error.
OPENSSL_EXPORT const uint8_t *SSL_get0_session_id_context(const SSL *ssl,
                                                          size_t *out_len);

// SSL_SESSION_CACHE_MAX_SIZE_DEFAULT is the default maximum size of a session
// cache.
#define SSL_SESSION_CACHE_MAX_SIZE_DEFAULT (1024 * 20)

// SSL_CTX_sess_set_cache_size sets the maximum size of |ctx|'s internal session
// cache to |size|. It returns the previous value.
OPENSSL_EXPORT unsigned long SSL_CTX_sess_set_cache_size(SSL_CTX *ctx,
                                                         unsigned long size);

// SSL_CTX_sess_get_cache_size returns the maximum size of |ctx|'s internal
// session cache.
OPENSSL_EXPORT unsigned long SSL_CTX_sess_get_cache_size(const SSL_CTX *ctx);

// SSL_CTX_sess_number returns the number of sessions in |ctx|'s internal
// session cache.
OPENSSL_EXPORT size_t SSL_CTX_sess_number(const SSL_CTX *ctx);

// SSL_CTX_add_session inserts |session| into |ctx|'s internal session cache. It
// returns one on success and zero on error or if |session| is already in the
// cache. The caller retains its reference to |session|.
OPENSSL_EXPORT int SSL_CTX_add_session(SSL_CTX *ctx, SSL_SESSION *session);

// SSL_CTX_remove_session removes |session| from |ctx|'s internal session cache.
// It returns one on success and zero if |session| was not in the cache.
OPENSSL_EXPORT int SSL_CTX_remove_session(SSL_CTX *ctx, SSL_SESSION *session);

// SSL_CTX_flush_sessions removes all sessions from |ctx| which have expired as
// of time |time|. If |time| is zero, all sessions are removed.
OPENSSL_EXPORT void SSL_CTX_flush_sessions(SSL_CTX *ctx, uint64_t time);

// SSL_CTX_sess_set_new_cb sets the callback to be called when a new session is
// established and ready to be cached. If the session cache is disabled (the
// appropriate one of |SSL_SESS_CACHE_CLIENT| or |SSL_SESS_CACHE_SERVER| is
// unset), the callback is not called.
//
// The callback is passed a reference to |session|. It returns one if it takes
// ownership (and then calls |SSL_SESSION_free| when done) and zero otherwise. A
// consumer which places |session| into an in-memory cache will likely return
// one, with the cache calling |SSL_SESSION_free|. A consumer which serializes
// |session| with |SSL_SESSION_to_bytes| may not need to retain |session| and
// will likely return zero. Returning one is equivalent to calling
// |SSL_SESSION_up_ref| and then returning zero.
//
// Note: For a client, the callback may be called on abbreviated handshakes if a
// ticket is renewed. Further, it may not be called until some time after
// |SSL_do_handshake| or |SSL_connect| completes if False Start is enabled. Thus
// it's recommended to use this callback over calling |SSL_get_session| on
// handshake completion.
OPENSSL_EXPORT void SSL_CTX_sess_set_new_cb(
    SSL_CTX *ctx, int (*new_session_cb)(SSL *ssl, SSL_SESSION *session));

// SSL_CTX_sess_get_new_cb returns the callback set by
// |SSL_CTX_sess_set_new_cb|.
OPENSSL_EXPORT int (*SSL_CTX_sess_get_new_cb(SSL_CTX *ctx))(
    SSL *ssl, SSL_SESSION *session);

// SSL_CTX_sess_set_remove_cb sets a callback which is called when a session is
// removed from the internal session cache.
//
// TODO(davidben): What is the point of this callback? It seems useless since it
// only fires on sessions in the internal cache.
OPENSSL_EXPORT void SSL_CTX_sess_set_remove_cb(
    SSL_CTX *ctx,
    void (*remove_session_cb)(SSL_CTX *ctx, SSL_SESSION *session));

// SSL_CTX_sess_get_remove_cb returns the callback set by
// |SSL_CTX_sess_set_remove_cb|.
OPENSSL_EXPORT void (*SSL_CTX_sess_get_remove_cb(SSL_CTX *ctx))(
    SSL_CTX *ctx, SSL_SESSION *session);

// SSL_CTX_sess_set_get_cb sets a callback to look up a session by ID for a
// server. The callback is passed the session ID and should return a matching
// |SSL_SESSION| or NULL if not found. It should set |*out_copy| to zero and
// return a new reference to the session. This callback is not used for a
// client.
//
// For historical reasons, if |*out_copy| is set to one (default), the SSL
// library will take a new reference to the returned |SSL_SESSION|, expecting
// the callback to return a non-owning pointer. This is not recommended. If
// |ctx| and thus the callback is used on multiple threads, the session may be
// removed and invalidated before the SSL library calls |SSL_SESSION_up_ref|,
// whereas the callback may synchronize internally.
//
// To look up a session asynchronously, the callback may return
// |SSL_magic_pending_session_ptr|. See the documentation for that function and
// |SSL_ERROR_PENDING_SESSION|.
//
// If the internal session cache is enabled, the callback is only consulted if
// the internal cache does not return a match.
OPENSSL_EXPORT void SSL_CTX_sess_set_get_cb(
    SSL_CTX *ctx, SSL_SESSION *(*get_session_cb)(SSL *ssl, const uint8_t *id,
                                                 int id_len, int *out_copy));

// SSL_CTX_sess_get_get_cb returns the callback set by
// |SSL_CTX_sess_set_get_cb|.
OPENSSL_EXPORT SSL_SESSION *(*SSL_CTX_sess_get_get_cb(SSL_CTX *ctx))(
    SSL *ssl, const uint8_t *id, int id_len, int *out_copy);

// SSL_magic_pending_session_ptr returns a magic |SSL_SESSION|* which indicates
// that the session isn't currently unavailable. |SSL_get_error| will then
// return |SSL_ERROR_PENDING_SESSION| and the handshake can be retried later
// when the lookup has completed.
OPENSSL_EXPORT SSL_SESSION *SSL_magic_pending_session_ptr(void);


// Session tickets.
//
// Session tickets, from RFC 5077, allow session resumption without server-side
// state. The server maintains a secret ticket key and sends the client opaque
// encrypted session parameters, called a ticket. When offering the session, the
// client sends the ticket which the server decrypts to recover session state.
// Session tickets are enabled by default but may be disabled with
// |SSL_OP_NO_TICKET|.
//
// On the client, ticket-based sessions use the same APIs as ID-based tickets.
// Callers do not need to handle them differently.
//
// On the server, tickets are encrypted and authenticated with a secret key.
// By default, an |SSL_CTX| will manage session ticket encryption keys by
// generating them internally and rotating every 48 hours. Tickets are minted
// and processed transparently. The following functions may be used to configure
// a persistent key or implement more custom behavior, including key rotation
// and sharing keys between multiple servers in a large deployment. There are
// three levels of customisation possible:
//
// 1) One can simply set the keys with |SSL_CTX_set_tlsext_ticket_keys|.
// 2) One can configure an |EVP_CIPHER_CTX| and |HMAC_CTX| directly for
//    encryption and authentication.
// 3) One can configure an |SSL_TICKET_AEAD_METHOD| to have more control
//    and the option of asynchronous decryption.
//
// An attacker that compromises a server's session ticket key can impersonate
// the server and, prior to TLS 1.3, retroactively decrypt all application
// traffic from sessions using that ticket key. Thus ticket keys must be
// regularly rotated for forward secrecy. Note the default key is rotated
// automatically once every 48 hours but manually configured keys are not.

// SSL_DEFAULT_TICKET_KEY_ROTATION_INTERVAL is the interval with which the
// default session ticket encryption key is rotated, if in use. If any
// non-default ticket encryption mechanism is configured, automatic rotation is
// disabled.
#define SSL_DEFAULT_TICKET_KEY_ROTATION_INTERVAL (2 * 24 * 60 * 60)

// SSL_CTX_get_tlsext_ticket_keys writes |ctx|'s session ticket key material to
// |len| bytes of |out|. It returns one on success and zero if |len| is not
// 48. If |out| is NULL, it returns 48 instead.
OPENSSL_EXPORT int SSL_CTX_get_tlsext_ticket_keys(SSL_CTX *ctx, void *out,
                                                  size_t len);

// SSL_CTX_set_tlsext_ticket_keys sets |ctx|'s session ticket key material to
// |len| bytes of |in|. It returns one on success and zero if |len| is not
// 48. If |in| is NULL, it returns 48 instead.
OPENSSL_EXPORT int SSL_CTX_set_tlsext_ticket_keys(SSL_CTX *ctx, const void *in,
                                                  size_t len);

// SSL_TICKET_KEY_NAME_LEN is the length of the key name prefix of a session
// ticket.
#define SSL_TICKET_KEY_NAME_LEN 16

// SSL_CTX_set_tlsext_ticket_key_cb sets the ticket callback to |callback| and
// returns one. |callback| will be called when encrypting a new ticket and when
// decrypting a ticket from the client.
//
// In both modes, |ctx| and |hmac_ctx| will already have been initialized with
// |EVP_CIPHER_CTX_init| and |HMAC_CTX_init|, respectively. |callback|
// configures |hmac_ctx| with an HMAC digest and key, and configures |ctx|
// for encryption or decryption, based on the mode.
//
// When encrypting a new ticket, |encrypt| will be one. It writes a public
// 16-byte key name to |key_name| and a fresh IV to |iv|. The output IV length
// must match |EVP_CIPHER_CTX_iv_length| of the cipher selected. In this mode,
// |callback| returns 1 on success and -1 on error.
//
// When decrypting a ticket, |encrypt| will be zero. |key_name| will point to a
// 16-byte key name and |iv| points to an IV. The length of the IV consumed must
// match |EVP_CIPHER_CTX_iv_length| of the cipher selected. In this mode,
// |callback| returns -1 to abort the handshake, 0 if decrypting the ticket
// failed, and 1 or 2 on success. If it returns 2, the ticket will be renewed.
// This may be used to re-key the ticket.
//
// WARNING: |callback| wildly breaks the usual return value convention and is
// called in two different modes.
OPENSSL_EXPORT int SSL_CTX_set_tlsext_ticket_key_cb(
    SSL_CTX *ctx,
    int (*callback)(SSL *ssl, uint8_t *key_name, uint8_t *iv,
                    EVP_CIPHER_CTX *ctx, HMAC_CTX *hmac_ctx, int encrypt));

// ssl_ticket_aead_result_t enumerates the possible results from decrypting a
// ticket with an |SSL_TICKET_AEAD_METHOD|.
enum ssl_ticket_aead_result_t {
  // ssl_ticket_aead_success indicates that the ticket was successfully
  // decrypted.
  ssl_ticket_aead_success,
  // ssl_ticket_aead_retry indicates that the operation could not be
  // immediately completed and must be reattempted, via |open|, at a later
  // point.
  ssl_ticket_aead_retry,
  // ssl_ticket_aead_ignore_ticket indicates that the ticket should be ignored
  // (i.e. is corrupt or otherwise undecryptable).
  ssl_ticket_aead_ignore_ticket,
  // ssl_ticket_aead_error indicates that a fatal error occured and the
  // handshake should be terminated.
  ssl_ticket_aead_error
};

// ssl_ticket_aead_method_st (aka |SSL_TICKET_AEAD_METHOD|) contains methods
// for encrypting and decrypting session tickets.
struct ssl_ticket_aead_method_st {
  // max_overhead returns the maximum number of bytes of overhead that |seal|
  // may add.
  size_t (*max_overhead)(SSL *ssl);

  // seal encrypts and authenticates |in_len| bytes from |in|, writes, at most,
  // |max_out_len| bytes to |out|, and puts the number of bytes written in
  // |*out_len|. The |in| and |out| buffers may be equal but will not otherwise
  // alias. It returns one on success or zero on error.
  int (*seal)(SSL *ssl, uint8_t *out, size_t *out_len, size_t max_out_len,
              const uint8_t *in, size_t in_len);

  // open authenticates and decrypts |in_len| bytes from |in|, writes, at most,
  // |max_out_len| bytes of plaintext to |out|, and puts the number of bytes
  // written in |*out_len|. The |in| and |out| buffers may be equal but will
  // not otherwise alias. See |ssl_ticket_aead_result_t| for details of the
  // return values. In the case that a retry is indicated, the caller should
  // arrange for the high-level operation on |ssl| to be retried when the
  // operation is completed, which will result in another call to |open|.
  enum ssl_ticket_aead_result_t (*open)(SSL *ssl, uint8_t *out, size_t *out_len,
                                        size_t max_out_len, const uint8_t *in,
                                        size_t in_len);
};

// SSL_CTX_set_ticket_aead_method configures a custom ticket AEAD method table
// on |ctx|. |aead_method| must remain valid for the lifetime of |ctx|.
OPENSSL_EXPORT void SSL_CTX_set_ticket_aead_method(
    SSL_CTX *ctx, const SSL_TICKET_AEAD_METHOD *aead_method);

// SSL_process_tls13_new_session_ticket processes an unencrypted TLS 1.3
// NewSessionTicket message from |buf| and returns a resumable |SSL_SESSION|,
// or NULL on error. The caller takes ownership of the returned session and
// must call |SSL_SESSION_free| to free it.
//
// |buf| contains |buf_len| bytes that represents a complete NewSessionTicket
// message including its header, i.e., one byte for the type (0x04) and three
// bytes for the length. |buf| must contain only one such message.
//
// This function may be used to process NewSessionTicket messages in TLS 1.3
// clients that are handling the record layer externally.
OPENSSL_EXPORT SSL_SESSION *SSL_process_tls13_new_session_ticket(
    SSL *ssl, const uint8_t *buf, size_t buf_len);

// SSL_CTX_set_num_tickets configures |ctx| to send |num_tickets| immediately
// after a successful TLS 1.3 handshake as a server. It returns one. Large
// values of |num_tickets| will be capped within the library.
//
// By default, BoringSSL sends two tickets.
OPENSSL_EXPORT int SSL_CTX_set_num_tickets(SSL_CTX *ctx, size_t num_tickets);

// SSL_CTX_get_num_tickets returns the number of tickets |ctx| will send
// immediately after a successful TLS 1.3 handshake as a server.
OPENSSL_EXPORT size_t SSL_CTX_get_num_tickets(const SSL_CTX *ctx);


// Diffie-Hellman groups and ephemeral key exchanges.
//
// Most TLS handshakes (ECDHE cipher suites in TLS 1.2, and all supported TLS
// 1.3 modes) incorporate an ephemeral key exchange, most commonly using
// Elliptic Curve Diffie-Hellman (ECDH), as described in RFC 8422. The key
// exchange algorithm is negotiated separately from the cipher suite, using
// NamedGroup values, which define Diffie-Hellman groups.
//
// Historically, these values were known as "curves", in reference to ECDH, and
// some APIs refer to the original name. RFC 7919 renamed them to "groups" in
// reference to Diffie-Hellman in general. These values are also used to select
// experimental post-quantum KEMs. Though not Diffie-Hellman groups, KEMs can
// fill a similar role in TLS, so they use the same codepoints.
//
// In TLS 1.2, the ECDH values also negotiate elliptic curves used in ECDSA. In
// TLS 1.3 and later, ECDSA curves are part of the signature algorithm. See
// |SSL_SIGN_*|.

// SSL_CTX_set1_groups sets the preferred groups for |ctx| to be |groups|. Each
// element of |groups| should be a |NID_*| constant from nid.h. It returns one
// on success and zero on failure.
//
// Note that this API does not use the |SSL_GROUP_*| values defined below.
OPENSSL_EXPORT int SSL_CTX_set1_groups(SSL_CTX *ctx, const int *groups,
                                       size_t num_groups);

// SSL_set1_groups sets the preferred groups for |ssl| to be |groups|. Each
// element of |groups| should be a |NID_*| constant from nid.h. It returns one
// on success and zero on failure.
//
// Note that this API does not use the |SSL_GROUP_*| values defined below.
OPENSSL_EXPORT int SSL_set1_groups(SSL *ssl, const int *groups,
                                   size_t num_groups);

// SSL_CTX_set1_groups_list sets the preferred groups for |ctx| to be the
// colon-separated list |groups|. Each element of |groups| should be a curve
// name (e.g. P-256, X25519, ...). It returns one on success and zero on
// failure.
OPENSSL_EXPORT int SSL_CTX_set1_groups_list(SSL_CTX *ctx, const char *groups);

// SSL_set1_groups_list sets the preferred groups for |ssl| to be the
// colon-separated list |groups|. Each element of |groups| should be a curve
// name (e.g. P-256, X25519, ...). It returns one on success and zero on
// failure.
OPENSSL_EXPORT int SSL_set1_groups_list(SSL *ssl, const char *groups);

// SSL_get_negotiated_group returns the NID of the group used by |ssl|'s most
// recently completed handshake, or |NID_undef| if not applicable.
OPENSSL_EXPORT int SSL_get_negotiated_group(const SSL *ssl);

// SSL_GROUP_* define TLS group IDs.
#define SSL_GROUP_SECP224R1 21
#define SSL_GROUP_SECP256R1 23
#define SSL_GROUP_SECP384R1 24
#define SSL_GROUP_SECP521R1 25
#define SSL_GROUP_X25519 29

// The following are defined at
// https://datatracker.ietf.org/doc/html/draft-kwiatkowski-tls-ecdhe-mlkem.html
#define SSL_GROUP_SECP256R1_MLKEM768 0x11EB
#define SSL_GROUP_X25519_MLKEM768 0x11EC
#define SSL_GROUP_SECP384R1_MLKEM1024 0x11ED

// The following are defined at
// https://datatracker.ietf.org/doc/html/draft-connolly-tls-mlkem-key-agreement.html
#define SSL_GROUP_MLKEM512  0x0200
#define SSL_GROUP_MLKEM768  0x0201
#define SSL_GROUP_MLKEM1024 0x0202

// SSL_get_group_id returns the ID of the group used by |ssl|'s most recently
// completed handshake, or 0 if not applicable.
OPENSSL_EXPORT uint16_t SSL_get_group_id(const SSL *ssl);

// SSL_get_group_name returns a human-readable name for the group specified by
// the given TLS group ID, or NULL if the group is unknown.
OPENSSL_EXPORT const char *SSL_get_group_name(uint16_t group_id);

// SSL_get_peer_tmp_key sets |*out_key| to the temporary key provided by the
// peer that was during the key exchange. If |ssl| is the server, the client's
// temporary key is returned; if |ssl| is the client, the server's temporary key
// is returned. It returns 1 on success and 0 if otherwise.
OPENSSL_EXPORT int SSL_get_peer_tmp_key(SSL *ssl, EVP_PKEY **out_key);

// SSL_get_server_tmp_key is a backwards compatible alias to
// |SSL_get_peer_tmp_key| in OpenSSL. Note that this means the client's
// temporary key is being set to |*out_key| instead, if |ssl| is the server.
OPENSSL_EXPORT int SSL_get_server_tmp_key(SSL *ssl, EVP_PKEY **out_key);

// *** EXPERIMENTAL — DO NOT USE WITHOUT CHECKING ***
//
// |SSL_to_bytes| and |SSL_from_bytes| are developed to support SSL transfer
// across processes after handshake finished.
//
// SSL transfer allows the TLS connection to be used in a different
// process (or on a different machine). This only applies to servers.
// 1. Before termination, the process 1 encodes the |SSL| by calling
// |SSL_to_bytes|.
// 2. Another process resumes the |SSL| by calling |SSL_from_bytes|.
//
// WARNING: The serialisation formats are not yet stable: version skew may be
//     fatal.
// WARNING: The encoded data contains sensitive key material and must be
//     protected.

// SSL_to_bytes serializes |in| into a newly allocated buffer and sets
// |*out_data| to that buffer and |*out_len| to its length. The caller takes
// ownership of the buffer and must call |OPENSSL_free| when done. It returns
// one on success and zero on error.
//
// CAUTION: This function will serialize an established TLS 1.2/1.3
// connection, which includes sensitive security parameters established during
// the connection handshake, and memory buffers that may contain
// sensitive in-flight application data. It is the callers responsibility for
// ensuring the confidentiality and data integrity of the serialized encoding.
// Minimally a caller must encrypt the returned bytes using an AEAD cipher, such
// as AES-128-GCM before persisting the bytes to storage.
//
// WARNING: Currently only works with TLS 1.2 or TLS 1.3 after handshake has
// finished.
// WARNING: Currently only supports |SSL| as server.
// WARNING: CRYPTO_EX_DATA |ssl->ex_data| is not encoded. Remember set |ex_data|
// back after decode.
// WARNING: BIO |ssl->rbio| and |ssl->wbio| are not encoded.
// WARNING: STACK_OF(SSL_CIPHER) |ssl->client_cipher_suites| is not encoded.
//
// Initial implementation of this API is made by Evgeny Potemkin.
OPENSSL_EXPORT int SSL_to_bytes(const SSL *in, uint8_t **out_data,
                                size_t *out_len);

// SSL_from_bytes parses |in_len| bytes from |in| as an SSL. It
// returns a newly-allocated |SSL| on success or NULL on error.
// The |SSL| is marked with handshake finished. |in| and |in_len| should
// come from |out_data| and |out_len| of |SSL_to_bytes|. In other words,
// |SSL_from_bytes| should be called after |SSL_to_bytes|.
//
// CAUTION: This function deserializes an encoded TLS 1.2/1.3 established
// connection so that the communication may continue on the previously
// established channel. It is the callers responsibility for maintaining
// confidentiality and integrity of serialized bytes between the time of
// serialization and invoking this function.
// See |SSL_to_bytes| for more details.
//
// WARNING: Do not decode the same bytes |in| for different connections.
//          Otherwise, the connections use the same key material.
// WARNING: Remember set |ssl->rbio| and |ssl->wbio| before using |ssl|.
// WARNING: Remember set callback functions and |ex_data| back if needed.
// WARNING: STACK_OF(SSL_CIPHER) |ssl->client_cipher_suites| is not encoded and
//          will be repopulated on next handshake.
// WARNING: To ensure behavior unchange, |ctx| setting should be the same.
//
// Initial implementation of this API is made by Evgeny Potemkin.
OPENSSL_EXPORT SSL *SSL_from_bytes(const uint8_t *in, size_t in_len,
                                   SSL_CTX *ctx);

// SSL_get_all_group_names outputs a list of possible strings
// |SSL_get_group_name| may return in this version of BoringSSL. It writes at
// most |max_out| entries to |out| and returns the total number it would have
// written, if |max_out| had been large enough. |max_out| may be initially set
// to zero to size the output.
//
// This function is only intended to help initialize tables in callers that want
// possible strings pre-declared. This list would not be suitable to set a list
// of supported features. It is in no particular order, and may contain
// placeholder, experimental, or deprecated values that do not apply to every
// caller. Future versions of BoringSSL may also return strings not in this
// list, so this does not apply if, say, sending strings across services.
OPENSSL_EXPORT size_t SSL_get_all_group_names(const char **out, size_t max_out);


// Certificate verification.
//
// SSL may authenticate either endpoint with an X.509 certificate. Typically
// this is used to authenticate the server to the client. These functions
// configure certificate verification.
//
// WARNING: By default, certificate verification errors on a client are not
// fatal. See |SSL_VERIFY_NONE| This may be configured with
// |SSL_CTX_set_verify|.
//
// By default clients are anonymous but a server may request a certificate from
// the client by setting |SSL_VERIFY_PEER|.
//
// Many of these functions use OpenSSL's legacy X.509 stack which is
// underdocumented and deprecated, but the replacement isn't ready yet. For
// now, consumers may use the existing stack or bypass it by performing
// certificate verification externally. This may be done with
// |SSL_CTX_set_cert_verify_callback| or by extracting the chain with
// |SSL_get_peer_cert_chain| after the handshake. In the future, functions will
// be added to use the SSL stack without dependency on any part of the legacy
// X.509 and ASN.1 stack.
//
// To augment certificate verification, a client may also enable OCSP stapling
// (RFC 6066) and Certificate Transparency (RFC 6962) extensions.

// SSL_VERIFY_NONE, on a client, verifies the server certificate but does not
// make errors fatal. The result may be checked with |SSL_get_verify_result|. On
// a server it does not request a client certificate. This is the default.
#define SSL_VERIFY_NONE 0x00

// SSL_VERIFY_PEER, on a client, makes server certificate errors fatal. On a
// server it requests a client certificate and makes errors fatal. However,
// anonymous clients are still allowed. See
// |SSL_VERIFY_FAIL_IF_NO_PEER_CERT|.
#define SSL_VERIFY_PEER 0x01

// SSL_VERIFY_FAIL_IF_NO_PEER_CERT configures a server to reject connections if
// the client declines to send a certificate. This flag must be used together
// with |SSL_VERIFY_PEER|, otherwise it won't work.
#define SSL_VERIFY_FAIL_IF_NO_PEER_CERT 0x02

// SSL_VERIFY_PEER_IF_NO_OBC configures a server to request a client certificate
// if and only if Channel ID is not negotiated.
#define SSL_VERIFY_PEER_IF_NO_OBC 0x04

// SSL_CTX_set_verify configures certificate verification behavior. |mode| is
// one of the |SSL_VERIFY_*| values defined above. |callback| should be NULL.
//
// If |callback| is non-NULL, it is called as in |X509_STORE_CTX_set_verify_cb|,
// which is a deprecated and fragile mechanism to run the default certificate
// verification process, but suppress individual errors in it. See
// |X509_STORE_CTX_set_verify_cb| for details, If set, the callback may use
// |SSL_get_ex_data_X509_STORE_CTX_idx| with |X509_STORE_CTX_get_ex_data| to
// look up the |SSL| from |store_ctx|.
//
// WARNING: |callback| is not suitable for implementing custom certificate
// check, accepting all certificates, or extracting the certificate after
// verification. It does not replace the default process and is called multiple
// times throughout that process. It is also very difficult to implement this
// callback safely, without inadvertently relying on implementation details or
// making incorrect assumptions about when the callback is called.
//
// Instead, use |SSL_CTX_set_custom_verify| or
// |SSL_CTX_set_cert_verify_callback| to customize certificate verification.
// Those callbacks can inspect the peer-sent chain, call |X509_verify_cert| and
// inspect the result, or perform other operations more straightforwardly.
OPENSSL_EXPORT void SSL_CTX_set_verify(
    SSL_CTX *ctx, int mode, int (*callback)(int ok, X509_STORE_CTX *store_ctx));

// SSL_set_verify configures certificate verification behavior. |mode| is one of
// the |SSL_VERIFY_*| values defined above. |callback| should be NULL.
//
// If |callback| is non-NULL, it is called as in |X509_STORE_CTX_set_verify_cb|,
// which is a deprecated and fragile mechanism to run the default certificate
// verification process, but suppress individual errors in it. See
// |X509_STORE_CTX_set_verify_cb| for details, If set, the callback may use
// |SSL_get_ex_data_X509_STORE_CTX_idx| with |X509_STORE_CTX_get_ex_data| to
// look up the |SSL| from |store_ctx|.
//
// WARNING: |callback| is not suitable for implementing custom certificate
// check, accepting all certificates, or extracting the certificate after
// verification. It does not replace the default process and is called multiple
// times throughout that process. It is also very difficult to implement this
// callback safely, without inadvertently relying on implementation details or
// making incorrect assumptions about when the callback is called.
//
// Instead, use |SSL_set_custom_verify| or |SSL_set_cert_verify_callback| to
// customize certificate verification. Those callbacks can inspect the peer-sent
// chain, call |X509_verify_cert| and inspect the result, or perform other
// operations more straightforwardly.
OPENSSL_EXPORT void SSL_set_verify(SSL *ssl, int mode,
                                   int (*callback)(int ok,
                                                   X509_STORE_CTX *store_ctx));

enum ssl_verify_result_t {
  ssl_verify_ok,
  ssl_verify_invalid,
  ssl_verify_retry
};

// SSL_CTX_set_custom_verify configures certificate verification. |mode| is one
// of the |SSL_VERIFY_*| values defined above. |callback| performs the
// certificate verification.
//
// The callback may call |SSL_get0_peer_certificates| for the certificate chain
// to validate. The callback should return |ssl_verify_ok| if the certificate is
// valid. If the certificate is invalid, the callback should return
// |ssl_verify_invalid| and optionally set |*out_alert| to an alert to send to
// the peer. Some useful alerts include |SSL_AD_CERTIFICATE_EXPIRED|,
// |SSL_AD_CERTIFICATE_REVOKED|, |SSL_AD_UNKNOWN_CA|, |SSL_AD_BAD_CERTIFICATE|,
// |SSL_AD_CERTIFICATE_UNKNOWN|, and |SSL_AD_INTERNAL_ERROR|. See RFC 5246
// section 7.2.2 for their precise meanings. If unspecified,
// |SSL_AD_CERTIFICATE_UNKNOWN| will be sent by default.
//
// To verify a certificate asynchronously, the callback may return
// |ssl_verify_retry|. The handshake will then pause with |SSL_get_error|
// returning |SSL_ERROR_WANT_CERTIFICATE_VERIFY|.
OPENSSL_EXPORT void SSL_CTX_set_custom_verify(
    SSL_CTX *ctx, int mode,
    enum ssl_verify_result_t (*callback)(SSL *ssl, uint8_t *out_alert));

// SSL_set_custom_verify behaves like |SSL_CTX_set_custom_verify| but configures
// an individual |SSL|.
OPENSSL_EXPORT void SSL_set_custom_verify(
    SSL *ssl, int mode,
    enum ssl_verify_result_t (*callback)(SSL *ssl, uint8_t *out_alert));

// SSL_CTX_get_verify_mode returns |ctx|'s verify mode, set by
// |SSL_CTX_set_verify|.
OPENSSL_EXPORT int SSL_CTX_get_verify_mode(const SSL_CTX *ctx);

// SSL_get_verify_mode returns |ssl|'s verify mode, set by |SSL_CTX_set_verify|
// or |SSL_set_verify|.  It returns -1 on error.
OPENSSL_EXPORT int SSL_get_verify_mode(const SSL *ssl);

// SSL_CTX_get_verify_callback returns the callback set by
// |SSL_CTX_set_verify|.
OPENSSL_EXPORT int (*SSL_CTX_get_verify_callback(const SSL_CTX *ctx))(
    int ok, X509_STORE_CTX *store_ctx);

// SSL_get_verify_callback returns the callback set by |SSL_CTX_set_verify| or
// |SSL_set_verify|.
OPENSSL_EXPORT int (*SSL_get_verify_callback(const SSL *ssl))(
    int ok, X509_STORE_CTX *store_ctx);

// SSL_set1_host sets a DNS name that will be required to be present in the
// verified leaf certificate. It returns one on success and zero on error.
//
// Note: unless _some_ name checking is performed, certificate validation is
// ineffective. Simply checking that a host has some certificate from a CA is
// rarely meaningful—you have to check that the CA believed that the host was
// who you expect to be talking to.
//
// By default, both subject alternative names and the subject's common name
// attribute are checked. The latter has long been deprecated, so callers should
// call |SSL_set_hostflags| with |X509_CHECK_FLAG_NEVER_CHECK_SUBJECT| to use
// the standard behavior. https://crbug.com/boringssl/464 tracks fixing the
// default.
OPENSSL_EXPORT int SSL_set1_host(SSL *ssl, const char *hostname);

// SSL_set_hostflags calls |X509_VERIFY_PARAM_set_hostflags| on the
// |X509_VERIFY_PARAM| associated with this |SSL*|. |flags| should be some
// combination of the |X509_CHECK_*| constants.
//
// |X509_V_FLAG_X509_STRICT| is always ON by default and
// |X509_V_FLAG_ALLOW_PROXY_CERTS| is always OFF. Both are non-configurable.
// See |x509.h| for more details.
OPENSSL_EXPORT void SSL_set_hostflags(SSL *ssl, unsigned flags);

// SSL_CTX_set_verify_depth sets the maximum depth of a certificate chain
// accepted in verification. This count excludes both the target certificate and
// the trust anchor (root certificate).
OPENSSL_EXPORT void SSL_CTX_set_verify_depth(SSL_CTX *ctx, int depth);

// SSL_set_verify_depth sets the maximum depth of a certificate chain accepted
// in verification. This count excludes both the target certificate and the
// trust anchor (root certificate).
OPENSSL_EXPORT void SSL_set_verify_depth(SSL *ssl, int depth);

// SSL_CTX_get_verify_depth returns the maximum depth of a certificate accepted
// in verification.
OPENSSL_EXPORT int SSL_CTX_get_verify_depth(const SSL_CTX *ctx);

// SSL_get_verify_depth returns the maximum depth of a certificate accepted in
// verification.
OPENSSL_EXPORT int SSL_get_verify_depth(const SSL *ssl);

// SSL_CTX_set1_param sets verification parameters from |param|. It returns one
// on success and zero on failure. The caller retains ownership of |param|.
OPENSSL_EXPORT int SSL_CTX_set1_param(SSL_CTX *ctx,
                                      const X509_VERIFY_PARAM *param);

// SSL_set1_param sets verification parameters from |param|. It returns one on
// success and zero on failure. The caller retains ownership of |param|.
OPENSSL_EXPORT int SSL_set1_param(SSL *ssl, const X509_VERIFY_PARAM *param);

// SSL_CTX_get0_param returns |ctx|'s |X509_VERIFY_PARAM| for certificate
// verification. The caller must not release the returned pointer but may call
// functions on it to configure it.
OPENSSL_EXPORT X509_VERIFY_PARAM *SSL_CTX_get0_param(SSL_CTX *ctx);

// SSL_get0_param returns |ssl|'s |X509_VERIFY_PARAM| for certificate
// verification. The caller must not release the returned pointer but may call
// functions on it to configure it.
OPENSSL_EXPORT X509_VERIFY_PARAM *SSL_get0_param(SSL *ssl);

// SSL_CTX_set_purpose sets |ctx|'s |X509_VERIFY_PARAM|'s 'purpose' parameter to
// |purpose|. It returns one on success and zero on error.
OPENSSL_EXPORT int SSL_CTX_set_purpose(SSL_CTX *ctx, int purpose);

// SSL_set_purpose sets |ssl|'s |X509_VERIFY_PARAM|'s 'purpose' parameter to
// |purpose|. It returns one on success and zero on error.
OPENSSL_EXPORT int SSL_set_purpose(SSL *ssl, int purpose);

// SSL_CTX_set_trust sets |ctx|'s |X509_VERIFY_PARAM|'s 'trust' parameter to
// |trust|. It returns one on success and zero on error.
OPENSSL_EXPORT int SSL_CTX_set_trust(SSL_CTX *ctx, int trust);

// SSL_set_trust sets |ssl|'s |X509_VERIFY_PARAM|'s 'trust' parameter to
// |trust|. It returns one on success and zero on error.
OPENSSL_EXPORT int SSL_set_trust(SSL *ssl, int trust);

// SSL_CTX_set_cert_store sets |ctx|'s certificate store to |store|. It takes
// ownership of |store|. The store is used for certificate verification.
//
// The store is also used for the auto-chaining feature, but this is deprecated.
// See also |SSL_MODE_NO_AUTO_CHAIN|.
OPENSSL_EXPORT void SSL_CTX_set_cert_store(SSL_CTX *ctx, X509_STORE *store);

// SSL_CTX_set1_cert_store is like |SSL_CTX_set_cert_store|, but does not take
// additional ownership of |store|.
OPENSSL_EXPORT void SSL_CTX_set1_cert_store(SSL_CTX *ctx, X509_STORE *store);

// SSL_CTX_get_cert_store returns |ctx|'s certificate store.
OPENSSL_EXPORT X509_STORE *SSL_CTX_get_cert_store(const SSL_CTX *ctx);

// SSL_CTX_set_default_verify_paths calls |X509_STORE_set_default_paths| on
// |ctx|'s store. See that function for details.
//
// Using this function is not recommended. In OpenSSL, these defaults are
// determined by OpenSSL's install prefix. There is no corresponding concept for
// BoringSSL. Future versions of BoringSSL may change or remove this
// functionality.
OPENSSL_EXPORT int SSL_CTX_set_default_verify_paths(SSL_CTX *ctx);

// SSL_CTX_load_verify_locations calls |X509_STORE_load_locations| on |ctx|'s
// store. See that function for details.
OPENSSL_EXPORT int SSL_CTX_load_verify_locations(SSL_CTX *ctx,
                                                 const char *ca_file,
                                                 const char *ca_dir);

// SSL_get_verify_result returns the result of certificate verification. It is
// either |X509_V_OK| or a |X509_V_ERR_*| value.
OPENSSL_EXPORT long SSL_get_verify_result(const SSL *ssl);

// SSL_set_verify_result sets the result of certificate verification.
OPENSSL_EXPORT void SSL_set_verify_result(SSL *ssl, long arg);

// SSL_alert_from_verify_result returns the SSL alert code, such as
// |SSL_AD_CERTIFICATE_EXPIRED|, that corresponds to an |X509_V_ERR_*| value.
// The return value is always an alert, even when |result| is |X509_V_OK|.
OPENSSL_EXPORT int SSL_alert_from_verify_result(long result);

// SSL_get_ex_data_X509_STORE_CTX_idx returns the ex_data index used to look up
// the |SSL| associated with an |X509_STORE_CTX| in the verify callback.
OPENSSL_EXPORT int SSL_get_ex_data_X509_STORE_CTX_idx(void);

// SSL_CTX_set_cert_verify_callback sets a custom callback to be called on
// certificate verification rather than |X509_verify_cert|. |store_ctx| contains
// the verification parameters. The callback should return one on success and
// zero on fatal error. It may use |X509_STORE_CTX_set_error| to set a
// verification result.
//
// The callback may use |SSL_get_ex_data_X509_STORE_CTX_idx| to recover the
// |SSL| object from |store_ctx|.
OPENSSL_EXPORT void SSL_CTX_set_cert_verify_callback(
    SSL_CTX *ctx, int (*callback)(X509_STORE_CTX *store_ctx, void *arg),
    void *arg);

// SSL_enable_signed_cert_timestamps causes |ssl| (which must be the client end
// of a connection) to request SCTs from the server. See
// https://tools.ietf.org/html/rfc6962.
//
// Call |SSL_get0_signed_cert_timestamp_list| to recover the SCT after the
// handshake.
OPENSSL_EXPORT void SSL_enable_signed_cert_timestamps(SSL *ssl);

// SSL_CTX_enable_signed_cert_timestamps enables SCT requests on all client SSL
// objects created from |ctx|.
//
// Call |SSL_get0_signed_cert_timestamp_list| to recover the SCT after the
// handshake.
OPENSSL_EXPORT void SSL_CTX_enable_signed_cert_timestamps(SSL_CTX *ctx);

// SSL_enable_ocsp_stapling causes |ssl| (which must be the client end of a
// connection) to request a stapled OCSP response from the server.
//
// Call |SSL_get0_ocsp_response| to recover the OCSP response after the
// handshake.
OPENSSL_EXPORT void SSL_enable_ocsp_stapling(SSL *ssl);

// SSL_CTX_enable_ocsp_stapling enables OCSP stapling on all client SSL objects
// created from |ctx|.
//
// Call |SSL_get0_ocsp_response| to recover the OCSP response after the
// handshake.
OPENSSL_EXPORT void SSL_CTX_enable_ocsp_stapling(SSL_CTX *ctx);

// SSL_CTX_set0_verify_cert_store sets an |X509_STORE| that will be used
// exclusively for certificate verification and returns one. Ownership of
// |store| is transferred to the |SSL_CTX|.
OPENSSL_EXPORT int SSL_CTX_set0_verify_cert_store(SSL_CTX *ctx,
                                                  X509_STORE *store);

// SSL_CTX_set1_verify_cert_store sets an |X509_STORE| that will be used
// exclusively for certificate verification and returns one. An additional
// reference to |store| will be taken.
OPENSSL_EXPORT int SSL_CTX_set1_verify_cert_store(SSL_CTX *ctx,
                                                  X509_STORE *store);

// SSL_set0_verify_cert_store sets an |X509_STORE| that will be used
// exclusively for certificate verification and returns one. Ownership of
// |store| is transferred to the |SSL|.
OPENSSL_EXPORT int SSL_set0_verify_cert_store(SSL *ssl, X509_STORE *store);

// SSL_set1_verify_cert_store sets an |X509_STORE| that will be used
// exclusively for certificate verification and returns one. An additional
// reference to |store| will be taken.
OPENSSL_EXPORT int SSL_set1_verify_cert_store(SSL *ssl, X509_STORE *store);

// SSL_CTX_set_verify_algorithm_prefs configures |ctx| to use |prefs| as the
// preference list when verifying signatures from the peer's long-term key. It
// returns one on zero on error. |prefs| should not include the internal-only
// value |SSL_SIGN_RSA_PKCS1_MD5_SHA1|.
OPENSSL_EXPORT int SSL_CTX_set_verify_algorithm_prefs(SSL_CTX *ctx,
                                                      const uint16_t *prefs,
                                                      size_t num_prefs);

// SSL_set_verify_algorithm_prefs configures |ssl| to use |prefs| as the
// preference list when verifying signatures from the peer's long-term key. It
// returns one on zero on error. |prefs| should not include the internal-only
// value |SSL_SIGN_RSA_PKCS1_MD5_SHA1|.
OPENSSL_EXPORT int SSL_set_verify_algorithm_prefs(SSL *ssl,
                                                  const uint16_t *prefs,
                                                  size_t num_prefs);

// Client certificate CA list.
//
// When requesting a client certificate, a server may advertise a list of
// certificate authorities which are accepted. These functions may be used to
// configure this list.

// SSL_set_client_CA_list sets |ssl|'s client certificate CA list to
// |name_list|. It takes ownership of and frees |name_list|.
OPENSSL_EXPORT void SSL_set_client_CA_list(SSL *ssl,
                                           STACK_OF(X509_NAME) *name_list);

// SSL_CTX_set_client_CA_list sets |ctx|'s client certificate CA list to
// |name_list|. It takes ownership of and frees |name_list|.
OPENSSL_EXPORT void SSL_CTX_set_client_CA_list(SSL_CTX *ctx,
                                               STACK_OF(X509_NAME) *name_list);

// SSL_set0_client_CAs sets |ssl|'s client certificate CA list to |name_list|,
// which should contain DER-encoded distinguished names (RFC 5280). It takes
// ownership of |name_list|.
OPENSSL_EXPORT void SSL_set0_client_CAs(SSL *ssl,
                                        STACK_OF(CRYPTO_BUFFER) *name_list);

// SSL_CTX_set0_client_CAs sets |ctx|'s client certificate CA list to
// |name_list|, which should contain DER-encoded distinguished names (RFC 5280).
// It takes ownership of |name_list|.
OPENSSL_EXPORT void SSL_CTX_set0_client_CAs(SSL_CTX *ctx,
                                            STACK_OF(CRYPTO_BUFFER) *name_list);

// SSL_get_client_CA_list returns |ssl|'s client certificate CA list. If |ssl|
// has not been configured as a client, this is the list configured by
// |SSL_CTX_set_client_CA_list|.
//
// If configured as a client, it returns the client certificate CA list sent by
// the server. In this mode, the behavior is undefined except during the
// callbacks set by |SSL_CTX_set_cert_cb| and |SSL_CTX_set_client_cert_cb| or
// when the handshake is paused because of them.
OPENSSL_EXPORT STACK_OF(X509_NAME) *SSL_get_client_CA_list(const SSL *ssl);

// SSL_get0_server_requested_CAs returns the CAs sent by a server to guide a
// client in certificate selection. They are a series of DER-encoded X.509
// names. This function may only be called during a callback set by
// |SSL_CTX_set_cert_cb| or when the handshake is paused because of it.
//
// The returned stack is owned by |ssl|, as are its contents. It should not be
// used past the point where the handshake is restarted after the callback.
OPENSSL_EXPORT const STACK_OF(CRYPTO_BUFFER) *SSL_get0_server_requested_CAs(
    const SSL *ssl);

// SSL_CTX_get_client_CA_list returns |ctx|'s client certificate CA list.
OPENSSL_EXPORT STACK_OF(X509_NAME) *SSL_CTX_get_client_CA_list(
    const SSL_CTX *ctx);

// SSL_add_client_CA appends |x509|'s subject to the client certificate CA list.
// It returns one on success or zero on error. The caller retains ownership of
// |x509|.
OPENSSL_EXPORT int SSL_add_client_CA(SSL *ssl, X509 *x509);

// SSL_CTX_add_client_CA appends |x509|'s subject to the client certificate CA
// list. It returns one on success or zero on error. The caller retains
// ownership of |x509|.
OPENSSL_EXPORT int SSL_CTX_add_client_CA(SSL_CTX *ctx, X509 *x509);

// SSL_load_client_CA_file opens |file| and reads PEM-encoded certificates from
// it. It returns a newly-allocated stack of the certificate subjects or NULL
// on error. Duplicates in |file| are ignored.
OPENSSL_EXPORT STACK_OF(X509_NAME) *SSL_load_client_CA_file(const char *file);

// SSL_dup_CA_list makes a deep copy of |list|. It returns the new list on
// success or NULL on allocation error.
OPENSSL_EXPORT STACK_OF(X509_NAME) *SSL_dup_CA_list(STACK_OF(X509_NAME) *list);

// SSL_add_file_cert_subjects_to_stack behaves like |SSL_load_client_CA_file|
// but appends the result to |out|. It returns one on success or zero on
// error.
OPENSSL_EXPORT int SSL_add_file_cert_subjects_to_stack(STACK_OF(X509_NAME) *out,
                                                       const char *file);

// SSL_add_bio_cert_subjects_to_stack behaves like
// |SSL_add_file_cert_subjects_to_stack| but reads from |bio|.
OPENSSL_EXPORT int SSL_add_bio_cert_subjects_to_stack(STACK_OF(X509_NAME) *out,
                                                      BIO *bio);


// Server name indication.
//
// The server_name extension (RFC 3546) allows the client to advertise the name
// of the server it is connecting to. This is used in virtual hosting
// deployments to select one of a several certificates on a single IP. Only the
// host_name name type is supported.

#define TLSEXT_NAMETYPE_host_name 0

// SSL_set_tlsext_host_name, for a client, configures |ssl| to advertise |name|
// in the server_name extension. It returns one on success and zero on error.
OPENSSL_EXPORT int SSL_set_tlsext_host_name(SSL *ssl, const char *name);

// SSL_get_servername, for a server, returns the hostname supplied by the
// client or NULL if there was none. The |type| argument must be
// |TLSEXT_NAMETYPE_host_name|.
OPENSSL_EXPORT const char *SSL_get_servername(const SSL *ssl, const int type);

// SSL_get_servername_type, for a server, returns |TLSEXT_NAMETYPE_host_name|
// if the client sent a hostname and -1 otherwise.
OPENSSL_EXPORT int SSL_get_servername_type(const SSL *ssl);

// SSL_CTX_set_tlsext_servername_callback configures |callback| to be called on
// the server after ClientHello extensions have been parsed and returns one.
// The callback may use |SSL_get_servername| to examine the server_name
// extension and returns a |SSL_TLSEXT_ERR_*| value. The value of |arg| may be
// set by calling |SSL_CTX_set_tlsext_servername_arg|.
//
// If the callback returns |SSL_TLSEXT_ERR_NOACK|, the server_name extension is
// not acknowledged in the ServerHello. If the return value is
// |SSL_TLSEXT_ERR_ALERT_FATAL|, then |*out_alert| is the alert to send,
// defaulting to |SSL_AD_UNRECOGNIZED_NAME|. |SSL_TLSEXT_ERR_ALERT_WARNING| is
// ignored and treated as |SSL_TLSEXT_ERR_OK|.
OPENSSL_EXPORT int SSL_CTX_set_tlsext_servername_callback(
    SSL_CTX *ctx, int (*callback)(SSL *ssl, int *out_alert, void *arg));

// SSL_CTX_set_tlsext_servername_arg sets the argument to the servername
// callback and returns one. See |SSL_CTX_set_tlsext_servername_callback|.
OPENSSL_EXPORT int SSL_CTX_set_tlsext_servername_arg(SSL_CTX *ctx, void *arg);

// SSL_TLSEXT_ERR_* are values returned by some extension-related callbacks.
#define SSL_TLSEXT_ERR_OK 0
#define SSL_TLSEXT_ERR_ALERT_WARNING 1
#define SSL_TLSEXT_ERR_ALERT_FATAL 2
#define SSL_TLSEXT_ERR_NOACK 3

// SSL_set_SSL_CTX changes |ssl|'s |SSL_CTX|. |ssl| will use the
// certificate-related settings from |ctx|, and |SSL_get_SSL_CTX| will report
// |ctx|. This function may be used during the callbacks registered by
// |SSL_CTX_set_select_certificate_cb|,
// |SSL_CTX_set_tlsext_servername_callback|, and |SSL_CTX_set_cert_cb| or when
// the handshake is paused from them. It is typically used to switch
// certificates based on SNI.
//
// Note the session cache and related settings will continue to use the initial
// |SSL_CTX|. Callers should use |SSL_CTX_set_session_id_context| to partition
// the session cache between different domains.
//
// TODO(davidben): Should other settings change after this call?
OPENSSL_EXPORT SSL_CTX *SSL_set_SSL_CTX(SSL *ssl, SSL_CTX *ctx);


// Application-layer protocol negotiation.
//
// The ALPN extension (RFC 7301) allows negotiating different application-layer
// protocols over a single port. This is used, for example, to negotiate
// HTTP/2.

// SSL_CTX_set_alpn_protos sets the client ALPN protocol list on |ctx| to
// |protos|. |protos| must be in wire-format (i.e. a series of non-empty, 8-bit
// length-prefixed strings), or the empty string to disable ALPN. It returns
// zero on success and one on failure. Configuring a non-empty string enables
// ALPN on a client.
//
// WARNING: this function is dangerous because it breaks the usual return value
// convention.
OPENSSL_EXPORT int SSL_CTX_set_alpn_protos(SSL_CTX *ctx, const uint8_t *protos,
                                           size_t protos_len);

// SSL_set_alpn_protos sets the client ALPN protocol list on |ssl| to |protos|.
// |protos| must be in wire-format (i.e. a series of non-empty, 8-bit
// length-prefixed strings), or the empty string to disable ALPN. It returns
// zero on success and one on failure. Configuring a non-empty string enables
// ALPN on a client.
//
// WARNING: this function is dangerous because it breaks the usual return value
// convention.
OPENSSL_EXPORT int SSL_set_alpn_protos(SSL *ssl, const uint8_t *protos,
                                       size_t protos_len);

// SSL_CTX_set_alpn_select_cb sets a callback function on |ctx| that is called
// during ClientHello processing in order to select an ALPN protocol from the
// client's list of offered protocols. |SSL_select_next_proto| is an optional
// utility function which may be useful in implementing this callback.
//
// The callback is passed a wire-format (i.e. a series of non-empty, 8-bit
// length-prefixed strings) ALPN protocol list in |in|. To select a protocol,
// the callback should set |*out| and |*out_len| to the selected protocol and
// return |SSL_TLSEXT_ERR_OK| on success. It does not pass ownership of the
// buffer, so |*out| should point to a static string, a buffer that outlives the
// callback call, or the corresponding entry in |in|.
//
// If the server supports ALPN, but there are no protocols in common, the
// callback should return |SSL_TLSEXT_ERR_ALERT_FATAL| to abort the connection
// with a no_application_protocol alert.
//
// If the server does not support ALPN, it can return |SSL_TLSEXT_ERR_NOACK| to
// continue the handshake without negotiating a protocol. This may be useful if
// multiple server configurations share an |SSL_CTX|, only some of which have
// ALPN protocols configured.
//
// |SSL_TLSEXT_ERR_ALERT_WARNING| is ignored and will be treated as
// |SSL_TLSEXT_ERR_NOACK|.
//
// The callback will only be called if the client supports ALPN. Callers that
// wish to require ALPN for all clients must check |SSL_get0_alpn_selected|
// after the handshake. In QUIC connections, this is done automatically.
//
// The cipher suite is selected before negotiating ALPN. The callback may use
// |SSL_get_pending_cipher| to query the cipher suite. This may be used to
// implement HTTP/2's cipher suite constraints.
OPENSSL_EXPORT void SSL_CTX_set_alpn_select_cb(
    SSL_CTX *ctx,
    int (*cb)(SSL *ssl, const uint8_t **out, uint8_t *out_len,
              const uint8_t *in, unsigned in_len, void *arg),
    void *arg);

// SSL_get0_alpn_selected gets the selected ALPN protocol (if any) from |ssl|.
// On return it sets |*out_data| to point to |*out_len| bytes of protocol name
// (not including the leading length-prefix byte). If the server didn't respond
// with a negotiated protocol then |*out_len| will be zero.
OPENSSL_EXPORT void SSL_get0_alpn_selected(const SSL *ssl,
                                           const uint8_t **out_data,
                                           unsigned *out_len);

// SSL_CTX_set_allow_unknown_alpn_protos configures client connections on |ctx|
// to allow unknown ALPN protocols from the server. Otherwise, by default, the
// client will require that the protocol be advertised in
// |SSL_CTX_set_alpn_protos|.
OPENSSL_EXPORT void SSL_CTX_set_allow_unknown_alpn_protos(SSL_CTX *ctx,
                                                          int enabled);


// Application-layer protocol settings
//
// The ALPS extension (draft-vvv-tls-alps) allows exchanging application-layer
// settings in the TLS handshake for applications negotiated with ALPN. Note
// that, when ALPS is negotiated, the client and server each advertise their own
// settings, so there are functions to both configure setting to send and query
// received settings.

// SSL_add_application_settings configures |ssl| to enable ALPS with ALPN
// protocol |proto|, sending an ALPS value of |settings|. It returns one on
// success and zero on error. If |proto| is negotiated via ALPN and the peer
// supports ALPS, |settings| will be sent to the peer. The peer's ALPS value can
// be retrieved with |SSL_get0_peer_application_settings|.
//
// On the client, this function should be called before the handshake, once for
// each supported ALPN protocol which uses ALPS. |proto| must be included in the
// client's ALPN configuration (see |SSL_CTX_set_alpn_protos| and
// |SSL_set_alpn_protos|). On the server, ALPS can be preconfigured for each
// protocol as in the client, or configuration can be deferred to the ALPN
// callback (see |SSL_CTX_set_alpn_select_cb|), in which case only the selected
// protocol needs to be configured.
//
// ALPS can be independently configured from 0-RTT, however changes in protocol
// settings will fallback to 1-RTT to negotiate the new value, so it is
// recommended for |settings| to be relatively stable.
OPENSSL_EXPORT int SSL_add_application_settings(SSL *ssl, const uint8_t *proto,
                                                size_t proto_len,
                                                const uint8_t *settings,
                                                size_t settings_len);

// SSL_get0_peer_application_settings sets |*out_data| and |*out_len| to a
// buffer containing the peer's ALPS value, or the empty string if ALPS was not
// negotiated. Note an empty string could also indicate the peer sent an empty
// settings value. Use |SSL_has_application_settings| to check if ALPS was
// negotiated. The output buffer is owned by |ssl| and is valid until the next
// time |ssl| is modified.
OPENSSL_EXPORT void SSL_get0_peer_application_settings(const SSL *ssl,
                                                       const uint8_t **out_data,
                                                       size_t *out_len);

// SSL_has_application_settings returns one if ALPS was negotiated on this
// connection and zero otherwise.
OPENSSL_EXPORT int SSL_has_application_settings(const SSL *ssl);

// SSL_set_alps_use_new_codepoint configures whether to use the new ALPS
// codepoint. By default, the old codepoint is used.
OPENSSL_EXPORT void SSL_set_alps_use_new_codepoint(SSL *ssl, int use_new);


// Certificate compression.
//
// Certificates in TLS 1.3 can be compressed (RFC 8879). BoringSSL supports this
// as both a client and a server, but does not link against any specific
// compression libraries in order to keep dependencies to a minimum. Instead,
// hooks for compression and decompression can be installed in an |SSL_CTX| to
// enable support.

// ssl_cert_compression_func_t is a pointer to a function that performs
// compression. It must write the compressed representation of |in| to |out|,
// returning one on success and zero on error. The results of compressing
// certificates are not cached internally. Implementations may wish to implement
// their own cache if they expect it to be useful given the certificates that
// they serve.
typedef int (*ssl_cert_compression_func_t)(SSL *ssl, CBB *out,
                                           const uint8_t *in, size_t in_len);

// ssl_cert_decompression_func_t is a pointer to a function that performs
// decompression. The compressed data from the peer is passed as |in| and the
// decompressed result must be exactly |uncompressed_len| bytes long. It returns
// one on success, in which case |*out| must be set to the result of
// decompressing |in|, or zero on error. Setting |*out| transfers ownership,
// i.e. |CRYPTO_BUFFER_free| will be called on |*out| at some point in the
// future. The results of decompressions are not cached internally.
// Implementations may wish to implement their own cache if they expect it to be
// useful.
typedef int (*ssl_cert_decompression_func_t)(SSL *ssl, CRYPTO_BUFFER **out,
                                             size_t uncompressed_len,
                                             const uint8_t *in, size_t in_len);

// SSL_CTX_add_cert_compression_alg registers a certificate compression
// algorithm on |ctx| with ID |alg_id|. (The value of |alg_id| should be an IANA
// assigned value and each can only be registered once.)
//
// One of the function pointers may be NULL to avoid having to implement both
// sides of a compression algorithm if you're only going to use it in one
// direction. In this case, the unimplemented direction acts like it was never
// configured.
//
// For a server, algorithms are registered in preference order with the most
// preferable first. It returns one on success or zero on error.
OPENSSL_EXPORT int SSL_CTX_add_cert_compression_alg(
    SSL_CTX *ctx, uint16_t alg_id, ssl_cert_compression_func_t compress,
    ssl_cert_decompression_func_t decompress);


// Next protocol negotiation.
//
// The NPN extension (draft-agl-tls-nextprotoneg-03) is the predecessor to ALPN
// and deprecated in favor of it.

// SSL_CTX_set_next_protos_advertised_cb sets a callback that is called when a
// TLS server needs a list of supported protocols for Next Protocol
// Negotiation. The returned list must be in wire format. The list is returned
// by setting |*out| to point to it and |*out_len| to its length. This memory
// will not be modified, but one should assume that |ssl| keeps a reference to
// it.
//
// The callback should return |SSL_TLSEXT_ERR_OK| if it wishes to advertise.
// Otherwise, no such extension will be included in the ServerHello.
OPENSSL_EXPORT void SSL_CTX_set_next_protos_advertised_cb(
    SSL_CTX *ctx,
    int (*cb)(SSL *ssl, const uint8_t **out, unsigned *out_len, void *arg),
    void *arg);

// SSL_CTX_set_next_proto_select_cb sets a callback that is called when a client
// needs to select a protocol from the server's provided list. |*out| must be
// set to point to the selected protocol (which may be within |in|). The length
// of the protocol name must be written into |*out_len|. The server's advertised
// protocols are provided in |in| and |in_len|. The callback can assume that
// |in| is syntactically valid. |SSL_select_next_proto| is an optional utility
// function which may be useful in implementing this callback.
//
// The client must select a protocol. It is fatal to the connection if this
// callback returns a value other than |SSL_TLSEXT_ERR_OK|.
//
// Configuring this callback enables NPN on a client.
OPENSSL_EXPORT void SSL_CTX_set_next_proto_select_cb(
    SSL_CTX *ctx,
    int (*cb)(SSL *ssl, uint8_t **out, uint8_t *out_len, const uint8_t *in,
              unsigned in_len, void *arg),
    void *arg);

// SSL_get0_next_proto_negotiated sets |*out_data| and |*out_len| to point to
// the client's requested protocol for this connection. If the client didn't
// request any protocol, then |*out_data| is set to NULL.
//
// Note that the client can request any protocol it chooses. The value returned
// from this function need not be a member of the list of supported protocols
// provided by the server.
OPENSSL_EXPORT void SSL_get0_next_proto_negotiated(const SSL *ssl,
                                                   const uint8_t **out_data,
                                                   unsigned *out_len);

// SSL_select_next_proto implements the standard protocol selection for either
// ALPN servers or NPN clients. It is expected that this function is called from
// the callback set by |SSL_CTX_set_alpn_select_cb| or
// |SSL_CTX_set_next_proto_select_cb|.
//
// |peer| and |supported| contain the peer and locally-configured protocols,
// respectively. This function finds the first protocol in |peer| which is also
// in |supported|. If one was found, it sets |*out| and |*out_len| to point to
// it and returns |OPENSSL_NPN_NEGOTIATED|. Otherwise, it returns
// |OPENSSL_NPN_NO_OVERLAP| and sets |*out| and |*out_len| to the first
// supported protocol.
//
// In ALPN, the server should only select protocols among those that the client
// offered. Thus, if this function returns |OPENSSL_NPN_NO_OVERLAP|, the caller
// should ignore |*out| and return |SSL_TLSEXT_ERR_ALERT_FATAL| from
// |SSL_CTX_set_alpn_select_cb|'s callback to indicate there was no match.
//
// In NPN, the client may either select one of the server's protocols, or an
// "opportunistic" protocol as described in Section 6 of
// draft-agl-tls-nextprotoneg-03. When this function returns
// |OPENSSL_NPN_NO_OVERLAP|, |*out| implicitly selects the first supported
// protocol for use as the opportunistic protocol. The caller may use it,
// ignore it and select a different opportunistic protocol, or ignore it and
// select no protocol (empty string).
//
// |peer| and |supported| must be vectors of 8-bit, length-prefixed byte
// strings. The length byte itself is not included in the length. A byte string
// of length 0 is invalid. No byte string may be truncated. |supported| must be
// non-empty; a caller that supports no ALPN/NPN protocols should skip
// negotiating the extension, rather than calling this function. If any of these
// preconditions do not hold, this function will return |OPENSSL_NPN_NO_OVERLAP|
// and set |*out| and |*out_len| to an empty buffer for robustness, but callers
// are not recommended to rely on this. An empty buffer is not a valid output
// for |SSL_CTX_set_alpn_select_cb|'s callback.
//
// WARNING: |*out| and |*out_len| may alias either |peer| or |supported| and may
// not be used after one of those buffers is modified or released. Additionally,
// this function is not const-correct for compatibility reasons. Although |*out|
// is a non-const pointer, callers may not modify the buffer though |*out|.
OPENSSL_EXPORT int SSL_select_next_proto(uint8_t **out, uint8_t *out_len,
                                         const uint8_t *peer, unsigned peer_len,
                                         const uint8_t *supported,
                                         unsigned supported_len);

#define OPENSSL_NPN_UNSUPPORTED 0
#define OPENSSL_NPN_NEGOTIATED 1
#define OPENSSL_NPN_NO_OVERLAP 2


// Channel ID.
//
// See draft-balfanz-tls-channelid-01. This is an old, experimental mechanism
// and should not be used in new code.

// SSL_CTX_set_tls_channel_id_enabled configures whether connections associated
// with |ctx| should enable Channel ID as a server.
OPENSSL_EXPORT void SSL_CTX_set_tls_channel_id_enabled(SSL_CTX *ctx,
                                                       int enabled);

// SSL_set_tls_channel_id_enabled configures whether |ssl| should enable Channel
// ID as a server.
OPENSSL_EXPORT void SSL_set_tls_channel_id_enabled(SSL *ssl, int enabled);

// SSL_CTX_set1_tls_channel_id configures a TLS client to send a TLS Channel ID
// to compatible servers. |private_key| must be a P-256 EC key. It returns one
// on success and zero on error.
OPENSSL_EXPORT int SSL_CTX_set1_tls_channel_id(SSL_CTX *ctx,
                                               EVP_PKEY *private_key);

// SSL_set1_tls_channel_id configures a TLS client to send a TLS Channel ID to
// compatible servers. |private_key| must be a P-256 EC key. It returns one on
// success and zero on error.
OPENSSL_EXPORT int SSL_set1_tls_channel_id(SSL *ssl, EVP_PKEY *private_key);

// SSL_get_tls_channel_id gets the client's TLS Channel ID from a server |SSL|
// and copies up to the first |max_out| bytes into |out|. The Channel ID
// consists of the client's P-256 public key as an (x,y) pair where each is a
// 32-byte, big-endian field element. It returns 0 if the client didn't offer a
// Channel ID and the length of the complete Channel ID otherwise. This function
// always returns zero if |ssl| is a client.
OPENSSL_EXPORT size_t SSL_get_tls_channel_id(SSL *ssl, uint8_t *out,
                                             size_t max_out);


// DTLS-SRTP.
//
// See RFC 5764.

// srtp_protection_profile_st (aka |SRTP_PROTECTION_PROFILE|) is an SRTP
// profile for use with the use_srtp extension.
struct srtp_protection_profile_st {
  const char *name;
  unsigned long id;
} /* SRTP_PROTECTION_PROFILE */;

DEFINE_CONST_STACK_OF(SRTP_PROTECTION_PROFILE)

// SRTP_* define constants for SRTP profiles.
#define SRTP_AES128_CM_SHA1_80 0x0001
#define SRTP_AES128_CM_SHA1_32 0x0002
#define SRTP_AES128_F8_SHA1_80 0x0003
#define SRTP_AES128_F8_SHA1_32 0x0004
#define SRTP_NULL_SHA1_80 0x0005
#define SRTP_NULL_SHA1_32 0x0006
#define SRTP_AEAD_AES_128_GCM 0x0007
#define SRTP_AEAD_AES_256_GCM 0x0008

// SSL_CTX_set_srtp_profiles enables SRTP for all SSL objects created from
// |ctx|. |profile| contains a colon-separated list of profile names. It returns
// one on success and zero on failure.
OPENSSL_EXPORT int SSL_CTX_set_srtp_profiles(SSL_CTX *ctx,
                                             const char *profiles);

// SSL_set_srtp_profiles enables SRTP for |ssl|.  |profile| contains a
// colon-separated list of profile names. It returns one on success and zero on
// failure.
OPENSSL_EXPORT int SSL_set_srtp_profiles(SSL *ssl, const char *profiles);

// SSL_get_srtp_profiles returns the SRTP profiles supported by |ssl|.
OPENSSL_EXPORT const STACK_OF(SRTP_PROTECTION_PROFILE) *SSL_get_srtp_profiles(
    const SSL *ssl);

// SSL_get_selected_srtp_profile returns the selected SRTP profile, or NULL if
// SRTP was not negotiated.
OPENSSL_EXPORT const SRTP_PROTECTION_PROFILE *SSL_get_selected_srtp_profile(
    SSL *ssl);


// Pre-shared keys.
//
// Connections may be configured with PSK (Pre-Shared Key) cipher suites. These
// authenticate using out-of-band pre-shared keys rather than certificates. See
// RFC 4279.
//
// This implementation uses NUL-terminated C strings for identities and identity
// hints, so values with a NUL character are not supported. (RFC 4279 does not
// specify the format of an identity.)

// PSK_MAX_IDENTITY_LEN is the maximum supported length of a PSK identity,
// excluding the NUL terminator.
#define PSK_MAX_IDENTITY_LEN 128

// PSK_MAX_PSK_LEN is the maximum supported length of a pre-shared key.
#define PSK_MAX_PSK_LEN 256

// SSL_psk_client_cb_func defines a function signature for the client callback.
typedef unsigned int (*SSL_psk_client_cb_func)(SSL *ssl,
                                               const char *hint,
                                               char *identity,
                                               unsigned int max_identity_len,
                                               uint8_t *psk,
                                               unsigned max_psk_len);

// SSL_CTX_set_psk_client_callback sets the callback to be called when PSK is
// negotiated on the client. This callback must be set to enable PSK cipher
// suites on the client.
//
// The callback is passed the identity hint in |hint| or NULL if none was
// provided. It should select a PSK identity and write the identity and the
// corresponding PSK to |identity| and |psk|, respectively. The identity is
// written as a NUL-terminated C string of length (excluding the NUL terminator)
// at most |max_identity_len|. The PSK's length must be at most |max_psk_len|.
// The callback returns the length of the PSK or 0 if no suitable identity was
// found.
OPENSSL_EXPORT void SSL_CTX_set_psk_client_callback(
    SSL_CTX *ctx, SSL_psk_client_cb_func cb);

// SSL_set_psk_client_callback sets the callback to be called when PSK is
// negotiated on the client. This callback must be set to enable PSK cipher
// suites on the client. See also |SSL_CTX_set_psk_client_callback|.
OPENSSL_EXPORT void SSL_set_psk_client_callback(
    SSL *ssl, SSL_psk_client_cb_func cb);

// SSL_psk_server_cb_func defines a function signature for the server callback.
typedef unsigned (*SSL_psk_server_cb_func)(SSL *ssl,
                                           const char *identity,
                                           uint8_t *psk,
                                           unsigned max_psk_len);

// SSL_CTX_set_psk_server_callback sets the callback to be called when PSK is
// negotiated on the server. This callback must be set to enable PSK cipher
// suites on the server.
//
// The callback is passed the identity in |identity|. It should write a PSK of
// length at most |max_psk_len| to |psk| and return the number of bytes written
// or zero if the PSK identity is unknown.
OPENSSL_EXPORT void SSL_CTX_set_psk_server_callback(
    SSL_CTX *ctx, SSL_psk_server_cb_func cb);

// SSL_set_psk_server_callback sets the callback to be called when PSK is
// negotiated on the server. This callback must be set to enable PSK cipher
// suites on the server. See also |SSL_CTX_set_psk_server_callback|.
OPENSSL_EXPORT void SSL_set_psk_server_callback(
    SSL *ssl, SSL_psk_server_cb_func cb);

// SSL_CTX_use_psk_identity_hint configures server connections to advertise an
// identity hint of |identity_hint|. It returns one on success and zero on
// error.
OPENSSL_EXPORT int SSL_CTX_use_psk_identity_hint(SSL_CTX *ctx,
                                                 const char *identity_hint);

// SSL_use_psk_identity_hint configures server connections to advertise an
// identity hint of |identity_hint|. It returns one on success and zero on
// error.
OPENSSL_EXPORT int SSL_use_psk_identity_hint(SSL *ssl,
                                             const char *identity_hint);

// SSL_get_psk_identity_hint returns the PSK identity hint advertised for |ssl|
// or NULL if there is none.
OPENSSL_EXPORT const char *SSL_get_psk_identity_hint(const SSL *ssl);

// SSL_get_psk_identity, after the handshake completes, returns the PSK identity
// that was negotiated by |ssl| or NULL if PSK was not used.
OPENSSL_EXPORT const char *SSL_get_psk_identity(const SSL *ssl);


// Delegated credentials.
//
// *** EXPERIMENTAL — PRONE TO CHANGE ***
//
// Delegated credentials (RFC 9345) allow a TLS 1.3 end point to use its
// certificate to issue new credentials for authentication. If the peer
// indicates support for this extension, then this host may use a delegated
// credential to sign the handshake. Once issued, credentials can't be revoked.
// In order to mitigate the damage in case the credential secret key is
// compromised, the credential is only valid for a short time (days, hours, or
// even minutes).
//
// Currently only the server side is implemented.
//
// Servers configure a DC for use in the handshake via
// |SSL_set1_delegated_credential|. It must be signed by the host's end-entity
// certificate as defined in RFC 9345.

// SSL_set1_delegated_credential configures the delegated credential (DC) that
// will be sent to the peer for the current connection. |dc| is the DC in wire
// format, and |pkey| or |key_method| is the corresponding private key.
// Currently, only servers may configure a DC to use in the handshake.
//
// The DC will only be used if the protocol version is correct and the signature
// scheme is supported by the peer. If not, the DC will not be negotiated and
// the handshake will use the private key (or private key method) associated
// with the certificate.
OPENSSL_EXPORT int SSL_set1_delegated_credential(
    SSL *ssl, CRYPTO_BUFFER *dc, EVP_PKEY *pkey,
    const SSL_PRIVATE_KEY_METHOD *key_method);

// SSL_delegated_credential_used returns one if a delegated credential was used
// and zero otherwise.
OPENSSL_EXPORT int SSL_delegated_credential_used(const SSL *ssl);


// QUIC integration.
//
// QUIC acts as an underlying transport for the TLS 1.3 handshake. The following
// functions allow a QUIC implementation to serve as the underlying transport as
// described in RFC 9001.
//
// When configured for QUIC, |SSL_do_handshake| will drive the handshake as
// before, but it will not use the configured |BIO|. It will call functions on
// |SSL_QUIC_METHOD| to configure secrets and send data. If data is needed from
// the peer, it will return |SSL_ERROR_WANT_READ|. As the caller receives data
// it can decrypt, it calls |SSL_provide_quic_data|. Subsequent
// |SSL_do_handshake| calls will then consume that data and progress the
// handshake. After the handshake is complete, the caller should continue to
// call |SSL_provide_quic_data| for any post-handshake data, followed by
// |SSL_process_quic_post_handshake| to process it. It is an error to call
// |SSL_read| and |SSL_write| in QUIC.
//
// 0-RTT behaves similarly to |TLS_method|'s usual behavior. |SSL_do_handshake|
// returns early as soon as the client (respectively, server) is allowed to send
// 0-RTT (respectively, half-RTT) data. The caller should then call
// |SSL_do_handshake| again to consume the remaining handshake messages and
// confirm the handshake. As a client, |SSL_ERROR_EARLY_DATA_REJECTED| and
// |SSL_reset_early_data_reject| behave as usual.
//
// See https://www.rfc-editor.org/rfc/rfc9001.html#section-4.1 for more details.
//
// To avoid DoS attacks, the QUIC implementation must limit the amount of data
// being queued up. The implementation can call
// |SSL_quic_max_handshake_flight_len| to get the maximum buffer length at each
// encryption level.
//
// QUIC implementations must additionally configure transport parameters with
// |SSL_set_quic_transport_params|. |SSL_get_peer_quic_transport_params| may be
// used to query the value received from the peer. BoringSSL handles this
// extension as an opaque byte string. The caller is responsible for serializing
// and parsing them. See https://www.rfc-editor.org/rfc/rfc9000#section-7.4 for
// details.
//
// QUIC additionally imposes restrictions on 0-RTT. In particular, the QUIC
// transport layer requires that if a server accepts 0-RTT data, then the
// transport parameters sent on the resumed connection must not lower any limits
// compared to the transport parameters that the server sent on the connection
// where the ticket for 0-RTT was issued. In effect, the server must remember
// the transport parameters with the ticket. Application protocols running on
// QUIC may impose similar restrictions, for example HTTP/3's restrictions on
// SETTINGS frames.
//
// BoringSSL implements this check by doing a byte-for-byte comparison of an
// opaque context passed in by the server. This context must be the same on the
// connection where the ticket was issued and the connection where that ticket
// is used for 0-RTT. If there is a mismatch, or the context was not set,
// BoringSSL will reject early data (but not reject the resumption attempt).
// This context is set via |SSL_set_quic_early_data_context| and should cover
// both transport parameters and any application state.
// |SSL_set_quic_early_data_context| must be called on the server with a
// non-empty context if the server is to support 0-RTT in QUIC.
//
// BoringSSL does not perform any client-side checks on the transport
// parameters received from a server that also accepted early data. It is up to
// the caller to verify that the received transport parameters do not lower any
// limits, and to close the QUIC connection if that is not the case. The same
// holds for any application protocol state remembered for 0-RTT, e.g. HTTP/3
// SETTINGS.

// ssl_encryption_level_t represents a specific QUIC encryption level used to
// transmit handshake messages.
enum ssl_encryption_level_t {
  ssl_encryption_initial = 0,
  ssl_encryption_early_data,
  ssl_encryption_handshake,
  ssl_encryption_application
};

// ssl_quic_method_st (aka |SSL_QUIC_METHOD|) describes custom QUIC hooks.
struct ssl_quic_method_st {
  // set_read_secret configures the read secret and cipher suite for the given
  // encryption level. It returns one on success and zero to terminate the
  // handshake with an error. It will be called at most once per encryption
  // level.
  //
  // BoringSSL will not release read keys before QUIC may use them. Once a level
  // has been initialized, QUIC may begin processing data from it. Handshake
  // data should be passed to |SSL_provide_quic_data| and application data (if
  // |level| is |ssl_encryption_early_data| or |ssl_encryption_application|) may
  // be processed according to the rules of the QUIC protocol.
  //
  // QUIC ACKs packets at the same encryption level they were received at,
  // except that client |ssl_encryption_early_data| (0-RTT) packets trigger
  // server |ssl_encryption_application| (1-RTT) ACKs. BoringSSL will always
  // install ACK-writing keys with |set_write_secret| before the packet-reading
  // keys with |set_read_secret|. This ensures the caller can always ACK any
  // packet it decrypts. Note this means the server installs 1-RTT write keys
  // before 0-RTT read keys.
  //
  // The converse is not true. An encryption level may be configured with write
  // secrets a roundtrip before the corresponding secrets for reading ACKs is
  // available.
  int (*set_read_secret)(SSL *ssl, enum ssl_encryption_level_t level,
                         const SSL_CIPHER *cipher, const uint8_t *secret,
                         size_t secret_len);
  // set_write_secret behaves like |set_read_secret| but configures the write
  // secret and cipher suite for the given encryption level. It will be called
  // at most once per encryption level.
  //
  // BoringSSL will not release write keys before QUIC may use them. If |level|
  // is |ssl_encryption_early_data| or |ssl_encryption_application|, QUIC may
  // begin sending application data at |level|. However, note that BoringSSL
  // configures server |ssl_encryption_application| write keys before the client
  // Finished. This allows QUIC to send half-RTT data, but the handshake is not
  // confirmed at this point and, if requesting client certificates, the client
  // is not yet authenticated.
  //
  // See |set_read_secret| for additional invariants between packets and their
  // ACKs.
  //
  // Note that, on 0-RTT reject, the |ssl_encryption_early_data| write secret
  // may use a different cipher suite from the other keys.
  int (*set_write_secret)(SSL *ssl, enum ssl_encryption_level_t level,
                          const SSL_CIPHER *cipher, const uint8_t *secret,
                          size_t secret_len);
  // add_handshake_data adds handshake data to the current flight at the given
  // encryption level. It returns one on success and zero on error.
  //
  // BoringSSL will pack data from a single encryption level together, but a
  // single handshake flight may include multiple encryption levels. Callers
  // should defer writing data to the network until |flush_flight| to better
  // pack QUIC packets into transport datagrams.
  //
  // If |level| is not |ssl_encryption_initial|, this function will not be
  // called before |level| is initialized with |set_write_secret|.
  int (*add_handshake_data)(SSL *ssl, enum ssl_encryption_level_t level,
                            const uint8_t *data, size_t len);
  // flush_flight is called when the current flight is complete and should be
  // written to the transport. Note a flight may contain data at several
  // encryption levels. It returns one on success and zero on error.
  int (*flush_flight)(SSL *ssl);
  // send_alert sends a fatal alert at the specified encryption level. It
  // returns one on success and zero on error.
  //
  // If |level| is not |ssl_encryption_initial|, this function will not be
  // called before |level| is initialized with |set_write_secret|.
  int (*send_alert)(SSL *ssl, enum ssl_encryption_level_t level, uint8_t alert);
};

// SSL_quic_max_handshake_flight_len returns returns the maximum number of bytes
// that may be received at the given encryption level. This function should be
// used to limit buffering in the QUIC implementation.
//
// See https://www.rfc-editor.org/rfc/rfc9000#section-7.5
OPENSSL_EXPORT size_t SSL_quic_max_handshake_flight_len(
    const SSL *ssl, enum ssl_encryption_level_t level);

// SSL_quic_read_level returns the current read encryption level.
//
// TODO(davidben): Is it still necessary to expose this function to callers?
// QUICHE does not use it.
OPENSSL_EXPORT enum ssl_encryption_level_t SSL_quic_read_level(const SSL *ssl);

// SSL_quic_write_level returns the current write encryption level.
//
// TODO(davidben): Is it still necessary to expose this function to callers?
// QUICHE does not use it.
OPENSSL_EXPORT enum ssl_encryption_level_t SSL_quic_write_level(const SSL *ssl);

// SSL_provide_quic_data provides data from QUIC at a particular encryption
// level |level|. It returns one on success and zero on error. Note this
// function will return zero if the handshake is not expecting data from |level|
// at this time. The QUIC implementation should then close the connection with
// an error.
OPENSSL_EXPORT int SSL_provide_quic_data(SSL *ssl,
                                         enum ssl_encryption_level_t level,
                                         const uint8_t *data, size_t len);


// SSL_process_quic_post_handshake processes any data that QUIC has provided
// after the handshake has completed. This includes NewSessionTicket messages
// sent by the server. It returns one on success and zero on error.
OPENSSL_EXPORT int SSL_process_quic_post_handshake(SSL *ssl);

// SSL_CTX_set_quic_method configures the QUIC hooks. This should only be
// configured with a minimum version of TLS 1.3. |quic_method| must remain valid
// for the lifetime of |ctx|. It returns one on success and zero on error.
OPENSSL_EXPORT int SSL_CTX_set_quic_method(SSL_CTX *ctx,
                                           const SSL_QUIC_METHOD *quic_method);

// SSL_set_quic_method configures the QUIC hooks. This should only be
// configured with a minimum version of TLS 1.3. |quic_method| must remain valid
// for the lifetime of |ssl|. It returns one on success and zero on error.
OPENSSL_EXPORT int SSL_set_quic_method(SSL *ssl,
                                       const SSL_QUIC_METHOD *quic_method);

// SSL_set_quic_transport_params configures |ssl| to send |params| (of length
// |params_len|) in the quic_transport_parameters extension in either the
// ClientHello or EncryptedExtensions handshake message. It is an error to set
// transport parameters if |ssl| is not configured for QUIC. The buffer pointed
// to by |params| only need be valid for the duration of the call to this
// function. This function returns 1 on success and 0 on failure.
OPENSSL_EXPORT int SSL_set_quic_transport_params(SSL *ssl,
                                                 const uint8_t *params,
                                                 size_t params_len);

// SSL_get_peer_quic_transport_params provides the caller with the value of the
// quic_transport_parameters extension sent by the peer. A pointer to the buffer
// containing the TransportParameters will be put in |*out_params|, and its
// length in |*params_len|. This buffer will be valid for the lifetime of the
// |SSL|. If no params were received from the peer, |*out_params_len| will be 0.
OPENSSL_EXPORT void SSL_get_peer_quic_transport_params(
    const SSL *ssl, const uint8_t **out_params, size_t *out_params_len);

// SSL_set_quic_use_legacy_codepoint configures whether to use the legacy QUIC
// extension codepoint 0xffa5 as opposed to the official value 57. Call with
// |use_legacy| set to 1 to use 0xffa5 and call with 0 to use 57. By default,
// the standard code point is used.
OPENSSL_EXPORT void SSL_set_quic_use_legacy_codepoint(SSL *ssl, int use_legacy);

// SSL_set_quic_early_data_context configures a context string in QUIC servers
// for accepting early data. If a resumption connection offers early data, the
// server will check if the value matches that of the connection which minted
// the ticket. If not, resumption still succeeds but early data is rejected.
// This should include all QUIC Transport Parameters except ones specified that
// the client MUST NOT remember. This should also include any application
// protocol-specific state. For HTTP/3, this should be the serialized server
// SETTINGS frame and the QUIC Transport Parameters (except the stateless reset
// token).
//
// This function may be called before |SSL_do_handshake| or during server
// certificate selection. It returns 1 on success and 0 on failure.
OPENSSL_EXPORT int SSL_set_quic_early_data_context(SSL *ssl,
                                                   const uint8_t *context,
                                                   size_t context_len);


// Early data.
//
// WARNING: 0-RTT support in BoringSSL is currently experimental and not fully
// implemented. It may cause interoperability or security failures when used.
//
// Early data, or 0-RTT, is a feature in TLS 1.3 which allows clients to send
// data on the first flight during a resumption handshake. This can save a
// round-trip in some application protocols.
//
// WARNING: A 0-RTT handshake has different security properties from normal
// handshake, so it is off by default unless opted in. In particular, early data
// is replayable by a network attacker. Callers must account for this when
// sending or processing data before the handshake is confirmed. See RFC 8446
// for more information.
//
// As a server, if early data is accepted, |SSL_do_handshake| will complete as
// soon as the ClientHello is processed and server flight sent. |SSL_write| may
// be used to send half-RTT data. |SSL_read| will consume early data and
// transition to 1-RTT data as appropriate. Prior to the transition,
// |SSL_in_init| will report the handshake is still in progress. Callers may use
// it or |SSL_in_early_data| to defer or reject requests as needed.
//
// Early data as a client is more complex. If the offered session (see
// |SSL_set_session|) is 0-RTT-capable, the handshake will return after sending
// the ClientHello. The predicted peer certificates and ALPN protocol will be
// available via the usual APIs. |SSL_write| will write early data, up to the
// session's limit. Writes past this limit and |SSL_read| will complete the
// handshake before continuing. Callers may also call |SSL_do_handshake| again
// to complete the handshake sooner.
//
// If the server accepts early data, the handshake will succeed. |SSL_read| and
// |SSL_write| will then act as in a 1-RTT handshake. The peer certificates and
// ALPN protocol will be as predicted and need not be re-queried.
//
// If the server rejects early data, |SSL_do_handshake| (and thus |SSL_read| and
// |SSL_write|) will then fail with |SSL_get_error| returning
// |SSL_ERROR_EARLY_DATA_REJECTED|. The caller should treat this as a connection
// error and most likely perform a high-level retry. Note the server may still
// have processed the early data due to attacker replays.
//
// To then continue the handshake on the original connection, use
// |SSL_reset_early_data_reject|. The connection will then behave as one which
// had not yet completed the handshake. This allows a faster retry than making a
// fresh connection. |SSL_do_handshake| will complete the full handshake,
// possibly resulting in different peer certificates, ALPN protocol, and other
// properties. The caller must disregard any values from before the reset and
// query again.
//
// Finally, to implement the fallback described in RFC 8446 appendix D.3, retry
// on a fresh connection without 0-RTT if the handshake fails with
// |SSL_R_WRONG_VERSION_ON_EARLY_DATA|.

// SSL_CTX_set_early_data_enabled sets whether early data is allowed to be used
// with resumptions using |ctx|.
OPENSSL_EXPORT void SSL_CTX_set_early_data_enabled(SSL_CTX *ctx, int enabled);

// SSL_set_early_data_enabled sets whether early data is allowed to be used
// with resumptions using |ssl|. See |SSL_CTX_set_early_data_enabled| for more
// information.
OPENSSL_EXPORT void SSL_set_early_data_enabled(SSL *ssl, int enabled);

// SSL_in_early_data returns one if |ssl| has a pending handshake that has
// progressed enough to send or receive early data. Clients may call |SSL_write|
// to send early data, but |SSL_read| will complete the handshake before
// accepting application data. Servers may call |SSL_read| to read early data
// and |SSL_write| to send half-RTT data.
OPENSSL_EXPORT int SSL_in_early_data(const SSL *ssl);

// SSL_SESSION_early_data_capable returns whether early data would have been
// attempted with |session| if enabled.
OPENSSL_EXPORT int SSL_SESSION_early_data_capable(const SSL_SESSION *session);

// SSL_SESSION_copy_without_early_data returns a copy of |session| with early
// data disabled. If |session| already does not support early data, it returns
// |session| with the reference count increased. The caller takes ownership of
// the result and must release it with |SSL_SESSION_free|.
//
// This function may be used on the client to clear early data support from
// existing sessions when the server rejects early data. In particular,
// |SSL_R_WRONG_VERSION_ON_EARLY_DATA| requires a fresh connection to retry, and
// the client would not want 0-RTT enabled for the next connection attempt.
OPENSSL_EXPORT SSL_SESSION *SSL_SESSION_copy_without_early_data(
    SSL_SESSION *session);

// SSL_early_data_accepted returns whether early data was accepted on the
// handshake performed by |ssl|.
OPENSSL_EXPORT int SSL_early_data_accepted(const SSL *ssl);

// SSL_reset_early_data_reject resets |ssl| after an early data reject. All
// 0-RTT state is discarded, including any pending |SSL_write| calls. The caller
// should treat |ssl| as a logically fresh connection, usually by driving the
// handshake to completion using |SSL_do_handshake|.
//
// It is an error to call this function on an |SSL| object that is not signaling
// |SSL_ERROR_EARLY_DATA_REJECTED|.
OPENSSL_EXPORT void SSL_reset_early_data_reject(SSL *ssl);

// SSL_get_ticket_age_skew returns the difference, in seconds, between the
// client-sent ticket age and the server-computed value in TLS 1.3 server
// connections which resumed a session.
OPENSSL_EXPORT int32_t SSL_get_ticket_age_skew(const SSL *ssl);

// An ssl_early_data_reason_t describes why 0-RTT was accepted or rejected.
// These values are persisted to logs. Entries should not be renumbered and
// numeric values should never be reused.
enum ssl_early_data_reason_t {
  // The handshake has not progressed far enough for the 0-RTT status to be
  // known.
  ssl_early_data_unknown = 0,
  // 0-RTT is disabled for this connection.
  ssl_early_data_disabled = 1,
  // 0-RTT was accepted.
  ssl_early_data_accepted = 2,
  // The negotiated protocol version does not support 0-RTT.
  ssl_early_data_protocol_version = 3,
  // The peer declined to offer or accept 0-RTT for an unknown reason.
  ssl_early_data_peer_declined = 4,
  // The client did not offer a session.
  ssl_early_data_no_session_offered = 5,
  // The server declined to resume the session.
  ssl_early_data_session_not_resumed = 6,
  // The session does not support 0-RTT.
  ssl_early_data_unsupported_for_session = 7,
  // The server sent a HelloRetryRequest.
  ssl_early_data_hello_retry_request = 8,
  // The negotiated ALPN protocol did not match the session.
  ssl_early_data_alpn_mismatch = 9,
  // The connection negotiated Channel ID, which is incompatible with 0-RTT.
  ssl_early_data_channel_id = 10,
  // Value 11 is reserved. (It has historically |ssl_early_data_token_binding|.)
  // The client and server ticket age were too far apart.
  ssl_early_data_ticket_age_skew = 12,
  // QUIC parameters differ between this connection and the original.
  ssl_early_data_quic_parameter_mismatch = 13,
  // The application settings did not match the session.
  ssl_early_data_alps_mismatch = 14,
  // The value of the largest entry.
  ssl_early_data_unsupported_with_custom_extension = 15,
  ssl_early_data_reason_max_value =
      ssl_early_data_unsupported_with_custom_extension
};

// SSL_get_early_data_reason returns details why 0-RTT was accepted or rejected
// on |ssl|. This is primarily useful on the server.
OPENSSL_EXPORT enum ssl_early_data_reason_t SSL_get_early_data_reason(
    const SSL *ssl);

// SSL_early_data_reason_string returns a string representation for |reason|, or
// NULL if |reason| is unknown. This function may be used for logging.
OPENSSL_EXPORT const char *SSL_early_data_reason_string(
    enum ssl_early_data_reason_t reason);


// Encrypted ClientHello.
//
// ECH is a mechanism for encrypting the entire ClientHello message in TLS 1.3.
// This can prevent observers from seeing cleartext information about the
// connection, such as the server_name extension.
//
// By default, BoringSSL will treat the server name, session ticket, and client
// certificate as secret, but most other parameters, such as the ALPN protocol
// list will be treated as public and sent in the cleartext ClientHello. Other
// APIs may be added for applications with different secrecy requirements.
//
// ECH support in BoringSSL is still experimental and under development.
//
// See https://tools.ietf.org/html/draft-ietf-tls-esni-13.

// SSL_set_enable_ech_grease configures whether the client will send a GREASE
// ECH extension when no supported ECHConfig is available.
OPENSSL_EXPORT void SSL_set_enable_ech_grease(SSL *ssl, int enable);

// SSL_set1_ech_config_list configures |ssl| to, as a client, offer ECH with the
// specified configuration. |ech_config_list| should contain a serialized
// ECHConfigList structure. It returns one on success and zero on error.
//
// This function returns an error if the input is malformed. If the input is
// valid but none of the ECHConfigs implement supported parameters, it will
// return success and proceed without ECH.
//
// If a supported ECHConfig is found, |ssl| will encrypt the true ClientHello
// parameters. If the server cannot decrypt it, e.g. due to a key mismatch, ECH
// has a recovery flow. |ssl| will handshake using the cleartext parameters,
// including a public name in the ECHConfig. If using
// |SSL_CTX_set_custom_verify|, callers should use |SSL_get0_ech_name_override|
// to verify the certificate with the public name. If using the built-in
// verifier, the |X509_STORE_CTX| will be configured automatically.
//
// If no other errors are found in this handshake, it will fail with
// |SSL_R_ECH_REJECTED|. Since it didn't use the true parameters, the connection
// cannot be used for application data. Instead, callers should handle this
// error by calling |SSL_get0_ech_retry_configs| and retrying the connection
// with updated ECH parameters. If the retry also fails with
// |SSL_R_ECH_REJECTED|, the caller should report a connection failure.
OPENSSL_EXPORT int SSL_set1_ech_config_list(SSL *ssl,
                                            const uint8_t *ech_config_list,
                                            size_t ech_config_list_len);

// SSL_get0_ech_name_override, if |ssl| is a client and the server rejected ECH,
// sets |*out_name| and |*out_name_len| to point to a buffer containing the ECH
// public name. Otherwise, the buffer will be empty.
//
// When offering ECH as a client, this function should be called during the
// certificate verification callback (see |SSL_CTX_set_custom_verify|). If
// |*out_name_len| is non-zero, the caller should verify the certificate against
// the result, interpreted as a DNS name, rather than the true server name. In
// this case, the handshake will never succeed and is only used to authenticate
// retry configs. See also |SSL_get0_ech_retry_configs|.
OPENSSL_EXPORT void SSL_get0_ech_name_override(const SSL *ssl,
                                               const char **out_name,
                                               size_t *out_name_len);

// SSL_get0_ech_retry_configs sets |*out_retry_configs| and
// |*out_retry_configs_len| to a buffer containing a serialized ECHConfigList.
// If the server did not provide an ECHConfigList, |*out_retry_configs_len| will
// be zero.
//
// When handling an |SSL_R_ECH_REJECTED| error code as a client, callers should
// use this function to recover from potential key mismatches. If the result is
// non-empty, the caller should retry the connection, passing this buffer to
// |SSL_set1_ech_config_list|. If the result is empty, the server has rolled
// back ECH support, and the caller should retry without ECH.
//
// This function must only be called in response to an |SSL_R_ECH_REJECTED|
// error code. Calling this function on |ssl|s that have not authenticated the
// rejection handshake will assert in debug builds and otherwise return an
// unparsable list.
OPENSSL_EXPORT void SSL_get0_ech_retry_configs(
    const SSL *ssl, const uint8_t **out_retry_configs,
    size_t *out_retry_configs_len);

// SSL_marshal_ech_config constructs a new serialized ECHConfig. On success, it
// sets |*out| to a newly-allocated buffer containing the result and |*out_len|
// to the size of the buffer. The caller must call |OPENSSL_free| on |*out| to
// release the memory. On failure, it returns zero.
//
// The |config_id| field is a single byte identifier for the ECHConfig. Reusing
// config IDs is allowed, but if multiple ECHConfigs with the same config ID are
// active at a time, server load may increase. See
// |SSL_ECH_KEYS_has_duplicate_config_id|.
//
// The public key and KEM algorithm are taken from |key|. |public_name| is the
// DNS name used to authenticate the recovery flow. |max_name_len| should be the
// length of the longest name in the ECHConfig's anonymity set and influences
// client padding decisions.
OPENSSL_EXPORT int SSL_marshal_ech_config(uint8_t **out, size_t *out_len,
                                          uint8_t config_id,
                                          const EVP_HPKE_KEY *key,
                                          const char *public_name,
                                          size_t max_name_len);

// SSL_ECH_KEYS_new returns a newly-allocated |SSL_ECH_KEYS| or NULL on error.
OPENSSL_EXPORT SSL_ECH_KEYS *SSL_ECH_KEYS_new(void);

// SSL_ECH_KEYS_up_ref increments the reference count of |keys|.
OPENSSL_EXPORT void SSL_ECH_KEYS_up_ref(SSL_ECH_KEYS *keys);

// SSL_ECH_KEYS_free releases memory associated with |keys|.
OPENSSL_EXPORT void SSL_ECH_KEYS_free(SSL_ECH_KEYS *keys);

// SSL_ECH_KEYS_add decodes |ech_config| as an ECHConfig and appends it with
// |key| to |keys|. If |is_retry_config| is non-zero, this config will be
// returned to the client on configuration mismatch. It returns one on success
// and zero on error.
//
// This function should be called successively to register each ECHConfig in
// decreasing order of preference. This configuration must be completed before
// setting |keys| on an |SSL_CTX| with |SSL_CTX_set1_ech_keys|. After that
// point, |keys| is immutable; no more ECHConfig values may be added.
//
// See also |SSL_CTX_set1_ech_keys|.
OPENSSL_EXPORT int SSL_ECH_KEYS_add(SSL_ECH_KEYS *keys, int is_retry_config,
                                    const uint8_t *ech_config,
                                    size_t ech_config_len,
                                    const EVP_HPKE_KEY *key);

// SSL_ECH_KEYS_has_duplicate_config_id returns one if |keys| has duplicate
// config IDs or zero otherwise. Duplicate config IDs still work, but may
// increase server load due to trial decryption.
OPENSSL_EXPORT int SSL_ECH_KEYS_has_duplicate_config_id(
    const SSL_ECH_KEYS *keys);

// SSL_ECH_KEYS_marshal_retry_configs serializes the retry configs in |keys| as
// an ECHConfigList. On success, it sets |*out| to a newly-allocated buffer
// containing the result and |*out_len| to the size of the buffer. The caller
// must call |OPENSSL_free| on |*out| to release the memory. On failure, it
// returns zero.
//
// This output may be advertised to clients in DNS.
OPENSSL_EXPORT int SSL_ECH_KEYS_marshal_retry_configs(const SSL_ECH_KEYS *keys,
                                                      uint8_t **out,
                                                      size_t *out_len);

// SSL_CTX_set1_ech_keys configures |ctx| to use |keys| to decrypt encrypted
// ClientHellos. It returns one on success, and zero on failure. If |keys| does
// not contain any retry configs, this function will fail. Retry configs are
// marked as such when they are added to |keys| with |SSL_ECH_KEYS_add|.
//
// Once |keys| has been passed to this function, it is immutable. Unlike most
// |SSL_CTX| configuration functions, this function may be called even if |ctx|
// already has associated connections on multiple threads. This may be used to
// rotate keys in a long-lived server process.
//
// The configured ECHConfig values should also be advertised out-of-band via DNS
// (see draft-ietf-dnsop-svcb-https). Before advertising an ECHConfig in DNS,
// deployments should ensure all instances of the service are configured with
// the ECHConfig and corresponding private key.
//
// Only the most recent fully-deployed ECHConfigs should be advertised in DNS.
// |keys| may contain a newer set if those ECHConfigs are mid-deployment. It
// should also contain older sets, until the DNS change has rolled out and the
// old records have expired from caches.
//
// If there is a mismatch, |SSL| objects associated with |ctx| will complete the
// handshake using the cleartext ClientHello and send updated ECHConfig values
// to the client. The client will then retry to recover, but with a latency
// penalty. This recovery flow depends on the public name in the ECHConfig.
// Before advertising an ECHConfig in DNS, deployments must ensure all instances
// of the service can present a valid certificate for the public name.
//
// BoringSSL negotiates ECH before certificate selection callbacks are called,
// including |SSL_CTX_set_select_certificate_cb|. If ECH is negotiated, the
// reported |SSL_CLIENT_HELLO| structure and |SSL_get_servername| function will
// transparently reflect the inner ClientHello. Callers should select parameters
// based on these values to correctly handle ECH as well as the recovery flow.
OPENSSL_EXPORT int SSL_CTX_set1_ech_keys(SSL_CTX *ctx, SSL_ECH_KEYS *keys);

// SSL_ech_accepted returns one if |ssl| negotiated ECH and zero otherwise.
OPENSSL_EXPORT int SSL_ech_accepted(const SSL *ssl);


// Alerts.
//
// TLS uses alerts to signal error conditions. Alerts have a type (warning or
// fatal) and description. OpenSSL internally handles fatal alerts with
// dedicated error codes (see |SSL_AD_REASON_OFFSET|). Except for close_notify,
// warning alerts are silently ignored and may only be surfaced with
// |SSL_CTX_set_info_callback|.

// SSL_AD_REASON_OFFSET is the offset between error reasons and |SSL_AD_*|
// values. Any error code under |ERR_LIB_SSL| with an error reason above this
// value corresponds to an alert description. Consumers may add or subtract
// |SSL_AD_REASON_OFFSET| to convert between them.
//
// make_errors.go reserves error codes above 1000 for manually-assigned errors.
// This value must be kept in sync with reservedReasonCode in make_errors.h
#define SSL_AD_REASON_OFFSET 1000

// SSL_AD_* are alert descriptions.
#define SSL_AD_CLOSE_NOTIFY SSL3_AD_CLOSE_NOTIFY
#define SSL_AD_UNEXPECTED_MESSAGE SSL3_AD_UNEXPECTED_MESSAGE
#define SSL_AD_BAD_RECORD_MAC SSL3_AD_BAD_RECORD_MAC
#define SSL_AD_DECRYPTION_FAILED TLS1_AD_DECRYPTION_FAILED
#define SSL_AD_RECORD_OVERFLOW TLS1_AD_RECORD_OVERFLOW
#define SSL_AD_DECOMPRESSION_FAILURE SSL3_AD_DECOMPRESSION_FAILURE
#define SSL_AD_HANDSHAKE_FAILURE SSL3_AD_HANDSHAKE_FAILURE
#define SSL_AD_NO_CERTIFICATE SSL3_AD_NO_CERTIFICATE  // Legacy SSL 3.0 value
#define SSL_AD_BAD_CERTIFICATE SSL3_AD_BAD_CERTIFICATE
#define SSL_AD_UNSUPPORTED_CERTIFICATE SSL3_AD_UNSUPPORTED_CERTIFICATE
#define SSL_AD_CERTIFICATE_REVOKED SSL3_AD_CERTIFICATE_REVOKED
#define SSL_AD_CERTIFICATE_EXPIRED SSL3_AD_CERTIFICATE_EXPIRED
#define SSL_AD_CERTIFICATE_UNKNOWN SSL3_AD_CERTIFICATE_UNKNOWN
#define SSL_AD_ILLEGAL_PARAMETER SSL3_AD_ILLEGAL_PARAMETER
#define SSL_AD_UNKNOWN_CA TLS1_AD_UNKNOWN_CA
#define SSL_AD_ACCESS_DENIED TLS1_AD_ACCESS_DENIED
#define SSL_AD_DECODE_ERROR TLS1_AD_DECODE_ERROR
#define SSL_AD_DECRYPT_ERROR TLS1_AD_DECRYPT_ERROR
#define SSL_AD_EXPORT_RESTRICTION TLS1_AD_EXPORT_RESTRICTION
#define SSL_AD_PROTOCOL_VERSION TLS1_AD_PROTOCOL_VERSION
#define SSL_AD_INSUFFICIENT_SECURITY TLS1_AD_INSUFFICIENT_SECURITY
#define SSL_AD_INTERNAL_ERROR TLS1_AD_INTERNAL_ERROR
#define SSL_AD_INAPPROPRIATE_FALLBACK SSL3_AD_INAPPROPRIATE_FALLBACK
#define SSL_AD_USER_CANCELLED TLS1_AD_USER_CANCELLED
#define SSL_AD_NO_RENEGOTIATION TLS1_AD_NO_RENEGOTIATION
#define SSL_AD_MISSING_EXTENSION TLS1_AD_MISSING_EXTENSION
#define SSL_AD_UNSUPPORTED_EXTENSION TLS1_AD_UNSUPPORTED_EXTENSION
#define SSL_AD_CERTIFICATE_UNOBTAINABLE TLS1_AD_CERTIFICATE_UNOBTAINABLE
#define SSL_AD_UNRECOGNIZED_NAME TLS1_AD_UNRECOGNIZED_NAME
#define SSL_AD_BAD_CERTIFICATE_STATUS_RESPONSE \
  TLS1_AD_BAD_CERTIFICATE_STATUS_RESPONSE
#define SSL_AD_BAD_CERTIFICATE_HASH_VALUE TLS1_AD_BAD_CERTIFICATE_HASH_VALUE
#define SSL_AD_UNKNOWN_PSK_IDENTITY TLS1_AD_UNKNOWN_PSK_IDENTITY
#define SSL_AD_CERTIFICATE_REQUIRED TLS1_AD_CERTIFICATE_REQUIRED
#define SSL_AD_NO_APPLICATION_PROTOCOL TLS1_AD_NO_APPLICATION_PROTOCOL
#define SSL_AD_ECH_REQUIRED TLS1_AD_ECH_REQUIRED

// SSL_alert_type_string_long returns a string description of |value| as an
// alert type (warning or fatal).
OPENSSL_EXPORT const char *SSL_alert_type_string_long(int value);

// SSL_alert_desc_string_long returns a string description of |value| as an
// alert description or "unknown" if unknown.
OPENSSL_EXPORT const char *SSL_alert_desc_string_long(int value);

// SSL_send_fatal_alert sends a fatal alert over |ssl| of the specified type,
// which should be one of the |SSL_AD_*| constants. It returns one on success
// and <= 0 on error. The caller should pass the return value into
// |SSL_get_error| to determine how to proceed. Once this function has been
// called, future calls to |SSL_write| will fail.
//
// If retrying a failed operation due to |SSL_ERROR_WANT_WRITE|, subsequent
// calls must use the same |alert| parameter.
OPENSSL_EXPORT int SSL_send_fatal_alert(SSL *ssl, uint8_t alert);


// ex_data functions.
//
// See |ex_data.h| for details.

OPENSSL_EXPORT int SSL_set_ex_data(SSL *ssl, int idx, void *data);
OPENSSL_EXPORT void *SSL_get_ex_data(const SSL *ssl, int idx);
OPENSSL_EXPORT int SSL_get_ex_new_index(long argl, void *argp,
                                        CRYPTO_EX_unused *unused,
                                        CRYPTO_EX_dup *dup_unused,
                                        CRYPTO_EX_free *free_func);

OPENSSL_EXPORT int SSL_SESSION_set_ex_data(SSL_SESSION *session, int idx,
                                           void *data);
OPENSSL_EXPORT void *SSL_SESSION_get_ex_data(const SSL_SESSION *session,
                                             int idx);
OPENSSL_EXPORT int SSL_SESSION_get_ex_new_index(long argl, void *argp,
                                                CRYPTO_EX_unused *unused,
                                                CRYPTO_EX_dup *dup_unused,
                                                CRYPTO_EX_free *free_func);

OPENSSL_EXPORT int SSL_CTX_set_ex_data(SSL_CTX *ctx, int idx, void *data);
OPENSSL_EXPORT void *SSL_CTX_get_ex_data(const SSL_CTX *ctx, int idx);
OPENSSL_EXPORT int SSL_CTX_get_ex_new_index(long argl, void *argp,
                                            CRYPTO_EX_unused *unused,
                                            CRYPTO_EX_dup *dup_unused,
                                            CRYPTO_EX_free *free_func);


// Low-level record-layer state.

// SSL_get_ivs sets |*out_iv_len| to the length of the IVs for the ciphers
// underlying |ssl| and sets |*out_read_iv| and |*out_write_iv| to point to the
// current IVs for the read and write directions. This is only meaningful for
// connections with implicit IVs (i.e. CBC mode with TLS 1.0).
//
// It returns one on success or zero on error.
OPENSSL_EXPORT int SSL_get_ivs(const SSL *ssl, const uint8_t **out_read_iv,
                               const uint8_t **out_write_iv,
                               size_t *out_iv_len);

// SSL_get_key_block_len returns the length of |ssl|'s key block, for TLS 1.2
// and below. It is an error to call this function during a handshake, or if
// |ssl| negotiated TLS 1.3.
OPENSSL_EXPORT size_t SSL_get_key_block_len(const SSL *ssl);

// SSL_generate_key_block generates |out_len| bytes of key material for |ssl|'s
// current connection state, for TLS 1.2 and below. It is an error to call this
// function during a handshake, or if |ssl| negotiated TLS 1.3.
OPENSSL_EXPORT int SSL_generate_key_block(const SSL *ssl, uint8_t *out,
                                          size_t out_len);

// SSL_get_read_traffic_secret retrives |ssl|'s read traffic key for the current
// connection state. This is only valid for TLS 1.3 connections. It is an error
// to call this function during a handshake, or if |ssl| was negotiated with
// TLS 1.2 or lower.
//
// If |secret| is NULL then |*out_len| is
// set to the maximum number of output bytes. Otherwise, on entry,
// |*out_len| must contain the length of the |secret| buffer. If the call
// is successful, the read traffic secret is written to |secret| and |*out_len|
// is set to its length.
//
// It returns one on success, or zero on error.
OPENSSL_EXPORT int SSL_get_read_traffic_secret(
    const SSL *ssl,
    uint8_t *secret, size_t *out_len);

// SSL_get_write_traffic_secret retrieves |ssl|'s write traffic key for the
// current connection state. This is only valid for TLS 1.3 connections. It is
// an error to call this function during a handshake, or if |ssl| was negotiated
// with TLS 1.2 or lower.
//
// If |secret| is NULL then |*out_len| is
// set to the maximum number of output bytes. Otherwise, on entry,
// |*out_len| must contain the length of the |secret| buffer. If the call
// is successful, the write traffic secret is written to |secret| and |*out_len|
// is set to its length.
//
// It returns one on success, or zero on error.
OPENSSL_EXPORT int SSL_get_write_traffic_secret(
    const SSL *ssl,
    uint8_t *secret, size_t *out_len);

// SSL_get_read_sequence returns, in TLS, the expected sequence number of the
// next incoming record in the current epoch. In DTLS, it returns the maximum
// sequence number received in the current epoch and includes the epoch number
// in the two most significant bytes.
OPENSSL_EXPORT uint64_t SSL_get_read_sequence(const SSL *ssl);

// SSL_get_write_sequence returns the sequence number of the next outgoing
// record in the current epoch. In DTLS, it includes the epoch number in the
// two most significant bytes.
OPENSSL_EXPORT uint64_t SSL_get_write_sequence(const SSL *ssl);

// SSL_CTX_set_record_protocol_version returns whether |version| is zero.
OPENSSL_EXPORT int SSL_CTX_set_record_protocol_version(SSL_CTX *ctx,
                                                       int version);


// Handshake hints.
//
// WARNING: Contact the BoringSSL team before using this API. While this
// mechanism was designed to gracefully recover from version skew and
// configuration mismatch, splitting a single TLS server into multiple services
// is complex.
//
// Some server deployments make asynchronous RPC calls in both ClientHello
// dispatch and private key operations. In TLS handshakes where the private key
// operation occurs in the first round-trip, this results in two consecutive RPC
// round-trips. Handshake hints allow the RPC service to predict a signature.
// If correctly predicted, this can skip the second RPC call.
//
// First, the server installs a certificate selection callback (see
// |SSL_CTX_set_select_certificate_cb|). When that is called, it performs the
// RPC as before, but includes the ClientHello and a capabilities string from
// |SSL_serialize_capabilities|.
//
// Next, the RPC service creates its own |SSL| object, applies the results of
// certificate selection, calls |SSL_request_handshake_hints|, and runs the
// handshake. If this successfully computes handshake hints (see
// |SSL_serialize_handshake_hints|), the RPC server should send the hints
// alongside any certificate selection results.
//
// Finally, the server calls |SSL_set_handshake_hints| and applies any
// configuration from the RPC server. It then completes the handshake as before.
// If the hints apply, BoringSSL will use the predicted signature and skip the
// private key callbacks. Otherwise, BoringSSL will call private key callbacks
// to generate a signature as before.
//
// Callers should synchronize configuration across the two services.
// Configuration mismatches and some cases of version skew are not fatal, but
// may result in the hints not applying. Additionally, some handshake flows use
// the private key in later round-trips, such as TLS 1.3 HelloRetryRequest. In
// those cases, BoringSSL will not predict a signature as there is no benefit.
// Callers must allow for handshakes to complete without a predicted signature.

// SSL_serialize_capabilities writes an opaque byte string to |out| describing
// some of |ssl|'s capabilities. It returns one on success and zero on error.
//
// This string is used by BoringSSL internally to reduce the impact of version
// skew.
OPENSSL_EXPORT int SSL_serialize_capabilities(const SSL *ssl, CBB *out);

// SSL_request_handshake_hints configures |ssl| to generate a handshake hint for
// |client_hello|. It returns one on success and zero on error. |client_hello|
// should contain a serialized ClientHello structure, from the |client_hello|
// and |client_hello_len| fields of the |SSL_CLIENT_HELLO| structure.
// |capabilities| should contain the output of |SSL_serialize_capabilities|.
//
// When configured, |ssl| will perform no I/O (so there is no need to configure
// |BIO|s). For QUIC, the caller should still configure an |SSL_QUIC_METHOD|,
// but the callbacks themselves will never be called and may be left NULL or
// report failure. |SSL_provide_quic_data| also should not be called.
//
// If hint generation is successful, |SSL_do_handshake| will stop the handshake
// early with |SSL_get_error| returning |SSL_ERROR_HANDSHAKE_HINTS_READY|. At
// this point, the caller should run |SSL_serialize_handshake_hints| to extract
// the resulting hints.
//
// Hint generation may fail if, e.g., |ssl| was unable to process the
// ClientHello. Callers should then complete the certificate selection RPC and
// continue the original handshake with no hint. It will likely fail, but this
// reports the correct alert to the client and is more robust in case of
// mismatch.
OPENSSL_EXPORT int SSL_request_handshake_hints(SSL *ssl,
                                               const uint8_t *client_hello,
                                               size_t client_hello_len,
                                               const uint8_t *capabilities,
                                               size_t capabilities_len);

// SSL_serialize_handshake_hints writes an opaque byte string to |out|
// containing the handshake hints computed by |out|. It returns one on success
// and zero on error. This function should only be called if
// |SSL_request_handshake_hints| was configured and the handshake terminated
// with |SSL_ERROR_HANDSHAKE_HINTS_READY|.
//
// This string may be passed to |SSL_set_handshake_hints| on another |SSL| to
// avoid an extra signature call.
OPENSSL_EXPORT int SSL_serialize_handshake_hints(const SSL *ssl, CBB *out);

// SSL_set_handshake_hints configures |ssl| to use |hints| as handshake hints.
// It returns one on success and zero on error. The handshake will then continue
// as before, but apply predicted values from |hints| where applicable.
//
// Hints may contain connection and session secrets, so they must not leak and
// must come from a source trusted to terminate the connection. However, they
// will not change |ssl|'s configuration. The caller is responsible for
// serializing and applying options from the RPC server as needed. This ensures
// |ssl|'s behavior is self-consistent and consistent with the caller's local
// decisions.
OPENSSL_EXPORT int SSL_set_handshake_hints(SSL *ssl, const uint8_t *hints,
                                           size_t hints_len);


// Obscure functions.

// SSL_CTX_set_msg_callback installs |cb| as the message callback for |ctx|.
// This callback will be called when sending or receiving low-level record
// headers, complete handshake messages, ChangeCipherSpec, and alerts.
// |write_p| is one for outgoing messages and zero for incoming messages.
//
// For each record header, |cb| is called with |version| = 0 and |content_type|
// = |SSL3_RT_HEADER|. The |len| bytes from |buf| contain the header. Note that
// this does not include the record body. If the record is sealed, the length
// in the header is the length of the ciphertext.
//
// For each handshake message, ChangeCipherSpec, and alert, |version| is the
// protocol version and |content_type| is the corresponding record type. The
// |len| bytes from |buf| contain the handshake message, one-byte
// ChangeCipherSpec body, and two-byte alert, respectively.
//
// In connections that enable ECH, |cb| is additionally called with
// |content_type| = |SSL3_RT_CLIENT_HELLO_INNER| for each ClientHelloInner that
// is encrypted or decrypted. The |len| bytes from |buf| contain the
// ClientHelloInner, including the reconstructed outer extensions and handshake
// header.
//
// For a V2ClientHello, |version| is |SSL2_VERSION|, |content_type| is zero, and
// the |len| bytes from |buf| contain the V2ClientHello structure.
OPENSSL_EXPORT void SSL_CTX_set_msg_callback(
    SSL_CTX *ctx, void (*cb)(int is_write, int version, int content_type,
                             const void *buf, size_t len, SSL *ssl, void *arg));

// SSL_CTX_set_msg_callback_arg sets the |arg| parameter of the message
// callback.
OPENSSL_EXPORT void SSL_CTX_set_msg_callback_arg(SSL_CTX *ctx, void *arg);

// SSL_set_msg_callback installs |cb| as the message callback of |ssl|. See
// |SSL_CTX_set_msg_callback| for when this callback is called.
OPENSSL_EXPORT void SSL_set_msg_callback(
    SSL *ssl, void (*cb)(int write_p, int version, int content_type,
                         const void *buf, size_t len, SSL *ssl, void *arg));

// SSL_set_msg_callback_arg sets the |arg| parameter of the message callback.
OPENSSL_EXPORT void SSL_set_msg_callback_arg(SSL *ssl, void *arg);

// SSL_CTX_set_keylog_callback configures a callback to log key material. This
// is intended for debugging use with tools like Wireshark. The |cb| function
// should log |line| followed by a newline, synchronizing with any concurrent
// access to the log.
//
// The format is described in
// https://developer.mozilla.org/en-US/docs/Mozilla/Projects/NSS/Key_Log_Format.
OPENSSL_EXPORT void SSL_CTX_set_keylog_callback(SSL_CTX *ctx,
                                                void (*cb)(const SSL *ssl,
                                                           const char *line));

// SSL_CTX_get_keylog_callback returns the callback configured by
// |SSL_CTX_set_keylog_callback|.
OPENSSL_EXPORT void (*SSL_CTX_get_keylog_callback(const SSL_CTX *ctx))(
    const SSL *ssl, const char *line);

// SSL_CTX_set_current_time_cb configures a callback to retrieve the current
// time, which should be set in |*out_clock|. This can be used for testing
// purposes; for example, a callback can be configured that returns a time
// set explicitly by the test. The |ssl| pointer passed to |cb| is always null.
OPENSSL_EXPORT void SSL_CTX_set_current_time_cb(
    SSL_CTX *ctx, void (*cb)(const SSL *ssl, struct timeval *out_clock));

// SSL_set_shed_handshake_config allows some of the configuration of |ssl| to be
// freed after its handshake completes.  Once configuration has been shed, APIs
// that query it may fail.  "Configuration" in this context means anything that
// was set by the caller, as distinct from information derived from the
// handshake.  For example, |SSL_get_ciphers| queries how the |SSL| was
// configured by the caller, and fails after configuration has been shed,
// whereas |SSL_get_cipher| queries the result of the handshake, and is
// unaffected by configuration shedding.
//
// If configuration shedding is enabled, it is an error to call |SSL_clear|.
//
// Note that configuration shedding as a client additionally depends on
// renegotiation being disabled (see |SSL_set_renegotiate_mode|). If
// renegotiation is possible, the configuration will be retained. If
// configuration shedding is enabled and renegotiation later disabled after the
// handshake, |SSL_set_renegotiate_mode| will shed configuration then. This may
// be useful for clients which support renegotiation with some ALPN protocols,
// such as HTTP/1.1, and not others, such as HTTP/2.
OPENSSL_EXPORT void SSL_set_shed_handshake_config(SSL *ssl, int enable);

enum ssl_renegotiate_mode_t {
  ssl_renegotiate_never = 0,
  ssl_renegotiate_once,
  ssl_renegotiate_freely,
  ssl_renegotiate_ignore,
  ssl_renegotiate_explicit
};

// SSL_set_renegotiate_mode configures how |ssl|, a client, reacts to
// renegotiation attempts by a server. If |ssl| is a server, peer-initiated
// renegotiations are *always* rejected and this function does nothing.
//
// WARNING: Renegotiation is error-prone, complicates TLS's security properties,
// and increases its attack surface. When enabled, many common assumptions about
// BoringSSL's behavior no longer hold, and the calling application must handle
// more cases. Renegotiation is also incompatible with many application
// protocols, e.g. section 9.2.1 of RFC 7540. Many functions behave in ambiguous
// or undefined ways during a renegotiation.
//
// The renegotiation mode defaults to |ssl_renegotiate_never|, but may be set
// at any point in a connection's lifetime. Set it to |ssl_renegotiate_once| to
// allow one renegotiation, |ssl_renegotiate_freely| to allow all
// renegotiations or |ssl_renegotiate_ignore| to ignore HelloRequest messages.
// Note that ignoring HelloRequest messages may cause the connection to stall
// if the server waits for the renegotiation to complete.
//
// If set to |ssl_renegotiate_explicit|, |SSL_read| and |SSL_peek| calls which
// encounter a HelloRequest will pause with |SSL_ERROR_WANT_RENEGOTIATE|.
// |SSL_write| will continue to work while paused. The caller may call
// |SSL_renegotiate| to begin the renegotiation at a later point. This mode may
// be used if callers wish to eagerly call |SSL_peek| without triggering a
// renegotiation.
//
// If configuration shedding is enabled (see |SSL_set_shed_handshake_config|),
// configuration is released if, at any point after the handshake, renegotiation
// is disabled. It is not possible to switch from disabling renegotiation to
// enabling it on a given connection. Callers that condition renegotiation on,
// e.g., ALPN must enable renegotiation before the handshake and conditionally
// disable it afterwards.
//
// When enabled, renegotiation can cause properties of |ssl|, such as the cipher
// suite, to change during the lifetime of the connection. More over, during a
// renegotiation, not all properties of the new handshake are available or fully
// established. In BoringSSL, most functions, such as |SSL_get_current_cipher|,
// report information from the most recently completed handshake, not the
// pending one. However, renegotiation may rerun handshake callbacks, such as
// |SSL_CTX_set_cert_cb|. Such callbacks must ensure they are acting on the
// desired versions of each property.
//
// BoringSSL does not reverify peer certificates on renegotiation and instead
// requires they match between handshakes, so certificate verification callbacks
// (see |SSL_CTX_set_custom_verify|) may assume |ssl| is in the initial
// handshake and use |SSL_get0_peer_certificates|, etc.
//
// There is no support in BoringSSL for initiating renegotiations as a client
// or server.
OPENSSL_EXPORT void SSL_set_renegotiate_mode(SSL *ssl,
                                             enum ssl_renegotiate_mode_t mode);

// SSL_renegotiate starts a deferred renegotiation on |ssl| if it was configured
// with |ssl_renegotiate_explicit| and has a pending HelloRequest. It returns
// one on success and zero on error.
//
// This function does not do perform any I/O. On success, a subsequent
// |SSL_do_handshake| call will run the handshake. |SSL_write| and
// |SSL_read| will also complete the handshake before sending or receiving
// application data.
OPENSSL_EXPORT int SSL_renegotiate(SSL *ssl);

// SSL_renegotiate_pending returns one if |ssl| is in the middle of a
// renegotiation.
OPENSSL_EXPORT int SSL_renegotiate_pending(SSL *ssl);

// SSL_total_renegotiations returns the total number of renegotiation handshakes
// performed by |ssl|. This includes the pending renegotiation, if any.
OPENSSL_EXPORT int SSL_total_renegotiations(const SSL *ssl);

// SSL_MAX_CERT_LIST_DEFAULT is the default maximum length, in bytes, of a peer
// certificate chain.
#define SSL_MAX_CERT_LIST_DEFAULT (1024 * 100)

// SSL_CTX_get_max_cert_list returns the maximum length, in bytes, of a peer
// certificate chain accepted by |ctx|.
OPENSSL_EXPORT size_t SSL_CTX_get_max_cert_list(const SSL_CTX *ctx);

// SSL_CTX_set_max_cert_list sets the maximum length, in bytes, of a peer
// certificate chain to |max_cert_list|. This affects how much memory may be
// consumed during the handshake.
OPENSSL_EXPORT void SSL_CTX_set_max_cert_list(SSL_CTX *ctx,
                                              size_t max_cert_list);

// SSL_get_max_cert_list returns the maximum length, in bytes, of a peer
// certificate chain accepted by |ssl|.
OPENSSL_EXPORT size_t SSL_get_max_cert_list(const SSL *ssl);

// SSL_set_max_cert_list sets the maximum length, in bytes, of a peer
// certificate chain to |max_cert_list|. This affects how much memory may be
// consumed during the handshake.
OPENSSL_EXPORT void SSL_set_max_cert_list(SSL *ssl, size_t max_cert_list);

// SSL_CTX_set_max_send_fragment sets the maximum length, in bytes, of records
// sent by |ctx|. Beyond this length, handshake messages and application data
// will be split into multiple records. It returns one on success or zero on
// error.
OPENSSL_EXPORT int SSL_CTX_set_max_send_fragment(SSL_CTX *ctx,
                                                 size_t max_send_fragment);

// SSL_set_max_send_fragment sets the maximum length, in bytes, of records sent
// by |ssl|. Beyond this length, handshake messages and application data will
// be split into multiple records. It returns one on success or zero on
// error. The minimum is 512, and the max is 16384.
OPENSSL_EXPORT int SSL_set_max_send_fragment(SSL *ssl,
                                             size_t max_send_fragment);

// ssl_early_callback_ctx (aka |SSL_CLIENT_HELLO|) is passed to certain
// callbacks that are called very early on during the server handshake. At this
// point, much of the SSL* hasn't been filled out and only the ClientHello can
// be depended on.
struct ssl_early_callback_ctx {
  SSL *ssl;
  const uint8_t *client_hello;
  size_t client_hello_len;
  uint16_t version;
  const uint8_t *random;
  size_t random_len;
  const uint8_t *session_id;
  size_t session_id_len;
  const uint8_t *cipher_suites;
  size_t cipher_suites_len;
  const uint8_t *compression_methods;
  size_t compression_methods_len;
  const uint8_t *extensions;
  size_t extensions_len;
} /* SSL_CLIENT_HELLO */;

// ssl_select_cert_result_t enumerates the possible results from selecting a
// certificate with |select_certificate_cb|.
enum ssl_select_cert_result_t {
  // ssl_select_cert_success indicates that the certificate selection was
  // successful.
  ssl_select_cert_success = 1,
  // ssl_select_cert_retry indicates that the operation could not be
  // immediately completed and must be reattempted at a later point.
  ssl_select_cert_retry = 0,
  // ssl_select_cert_error indicates that a fatal error occured and the
  // handshake should be terminated.
  ssl_select_cert_error = -1
};

// SSL_early_callback_ctx_extension_get searches the extensions in
// |client_hello| for an extension of the given type. If not found, it returns
// zero. Otherwise it sets |out_data| to point to the extension contents (not
// including the type and length bytes), sets |out_len| to the length of the
// extension contents and returns one.
OPENSSL_EXPORT int SSL_early_callback_ctx_extension_get(
    const SSL_CLIENT_HELLO *client_hello, uint16_t extension_type,
    const uint8_t **out_data, size_t *out_len);

// SSL_CTX_set_select_certificate_cb sets a callback that is called before most
// ClientHello processing and before the decision whether to resume a session
// is made. The callback may inspect the ClientHello and configure the
// connection. See |ssl_select_cert_result_t| for details of the return values.
//
// In the case that a retry is indicated, |SSL_get_error| will return
// |SSL_ERROR_PENDING_CERTIFICATE| and the caller should arrange for the
// high-level operation on |ssl| to be retried at a later time, which will
// result in another call to |cb|.
//
// |SSL_get_servername| may be used during this callback.
//
// Note: The |SSL_CLIENT_HELLO| is only valid for the duration of the callback
// and is not valid while the handshake is paused.
OPENSSL_EXPORT void SSL_CTX_set_select_certificate_cb(
    SSL_CTX *ctx,
    enum ssl_select_cert_result_t (*cb)(const SSL_CLIENT_HELLO *));

// SSL_CTX_set_dos_protection_cb sets a callback that is called once the
// resumption decision for a ClientHello has been made. It can return one to
// allow the handshake to continue or zero to cause the handshake to abort.
OPENSSL_EXPORT void SSL_CTX_set_dos_protection_cb(
    SSL_CTX *ctx, int (*cb)(const SSL_CLIENT_HELLO *));

// SSL_CTX_set_reverify_on_resume configures whether the certificate
// verification callback will be used to reverify stored certificates
// when resuming a session. This only works with |SSL_CTX_set_custom_verify|.
// For now, this is incompatible with |SSL_VERIFY_NONE| mode, and is only
// respected on clients.
OPENSSL_EXPORT void SSL_CTX_set_reverify_on_resume(SSL_CTX *ctx, int enabled);

// SSL_set_enforce_rsa_key_usage configures whether, when |ssl| is a client
// negotiating TLS 1.2 or below, the keyUsage extension of RSA leaf server
// certificates will be checked for consistency with the TLS usage. In all other
// cases, this check is always enabled.
//
// This parameter may be set late; it will not be read until after the
// certificate verification callback.
OPENSSL_EXPORT void SSL_set_enforce_rsa_key_usage(SSL *ssl, int enabled);

// SSL_was_key_usage_invalid returns one if |ssl|'s handshake succeeded despite
// using TLS parameters which were incompatible with the leaf certificate's
// keyUsage extension. Otherwise, it returns zero.
//
// If |SSL_set_enforce_rsa_key_usage| is enabled or not applicable, this
// function will always return zero because key usages will be consistently
// checked.
OPENSSL_EXPORT int SSL_was_key_usage_invalid(const SSL *ssl);

// SSL_ST_* are possible values for |SSL_state|, the bitmasks that make them up,
// and some historical values for compatibility. Only |SSL_ST_INIT| and
// |SSL_ST_OK| are ever returned.
#define SSL_ST_CONNECT 0x1000
#define SSL_ST_ACCEPT 0x2000
#define SSL_ST_MASK 0x0FFF
#define SSL_ST_INIT (SSL_ST_CONNECT | SSL_ST_ACCEPT)
#define SSL_ST_OK 0x03
#define SSL_ST_RENEGOTIATE (0x04 | SSL_ST_INIT)
#define SSL_ST_BEFORE (0x05 | SSL_ST_INIT)

// OSSL_HANDSHAKE_STATE enumerates possible TLS states returned from
// |SSL_get_state| and |SSL_state|. TLS_ST_* are aliases for |SSL_ST_*| for
// OpenSSL 1.1.0 compatibility.
typedef enum {
  TLS_ST_OK = SSL_ST_OK,
  TLS_ST_BEFORE = SSL_ST_INIT
} OSSL_HANDSHAKE_STATE;

// SSL_CB_* are possible values for the |type| parameter in the info
// callback and the bitmasks that make them up.
#define SSL_CB_LOOP 0x01
#define SSL_CB_EXIT 0x02
#define SSL_CB_READ 0x04
#define SSL_CB_WRITE 0x08
#define SSL_CB_ALERT 0x4000
#define SSL_CB_READ_ALERT (SSL_CB_ALERT | SSL_CB_READ)
#define SSL_CB_WRITE_ALERT (SSL_CB_ALERT | SSL_CB_WRITE)
#define SSL_CB_ACCEPT_LOOP (SSL_ST_ACCEPT | SSL_CB_LOOP)
#define SSL_CB_ACCEPT_EXIT (SSL_ST_ACCEPT | SSL_CB_EXIT)
#define SSL_CB_CONNECT_LOOP (SSL_ST_CONNECT | SSL_CB_LOOP)
#define SSL_CB_CONNECT_EXIT (SSL_ST_CONNECT | SSL_CB_EXIT)
#define SSL_CB_HANDSHAKE_START 0x10
#define SSL_CB_HANDSHAKE_DONE 0x20

// SSL_CTX_set_info_callback configures a callback to be run when various
// events occur during a connection's lifetime. The |type| argument determines
// the type of event and the meaning of the |value| argument. Callbacks must
// ignore unexpected |type| values.
//
// |SSL_CB_READ_ALERT| is signaled for each alert received, warning or fatal.
// The |value| argument is a 16-bit value where the alert level (either
// |SSL3_AL_WARNING| or |SSL3_AL_FATAL|) is in the most-significant eight bits
// and the alert type (one of |SSL_AD_*|) is in the least-significant eight.
//
// |SSL_CB_WRITE_ALERT| is signaled for each alert sent. The |value| argument
// is constructed as with |SSL_CB_READ_ALERT|.
//
// |SSL_CB_HANDSHAKE_START| is signaled when a handshake begins. The |value|
// argument is always one.
//
// |SSL_CB_HANDSHAKE_DONE| is signaled when a handshake completes successfully.
// The |value| argument is always one. If a handshake False Starts, this event
// may be used to determine when the Finished message is received.
//
// The following event types expose implementation details of the handshake
// state machine. Consuming them is deprecated.
//
// |SSL_CB_ACCEPT_LOOP| (respectively, |SSL_CB_CONNECT_LOOP|) is signaled when
// a server (respectively, client) handshake progresses. The |value| argument
// is always one.
//
// |SSL_CB_ACCEPT_EXIT| (respectively, |SSL_CB_CONNECT_EXIT|) is signaled when
// a server (respectively, client) handshake completes, fails, or is paused.
// The |value| argument is one if the handshake succeeded and <= 0
// otherwise.
OPENSSL_EXPORT void SSL_CTX_set_info_callback(SSL_CTX *ctx,
                                              void (*cb)(const SSL *ssl,
                                                         int type, int value));

// SSL_CTX_get_info_callback returns the callback set by
// |SSL_CTX_set_info_callback|.
OPENSSL_EXPORT void (*SSL_CTX_get_info_callback(SSL_CTX *ctx))(const SSL *ssl,
                                                               int type,
                                                               int value);

// SSL_set_info_callback configures a callback to be run at various events
// during a connection's lifetime. See |SSL_CTX_set_info_callback|.
OPENSSL_EXPORT void SSL_set_info_callback(SSL *ssl,
                                          void (*cb)(const SSL *ssl, int type,
                                                     int value));

// SSL_get_info_callback returns the callback set by |SSL_set_info_callback|.
OPENSSL_EXPORT void (*SSL_get_info_callback(const SSL *ssl))(const SSL *ssl,
                                                             int type,
                                                             int value);

// SSL_state_string_long returns the current state of the handshake state
// machine as a string. This may be useful for debugging and logging.
OPENSSL_EXPORT const char *SSL_state_string_long(const SSL *ssl);

#define SSL_SENT_SHUTDOWN 1
#define SSL_RECEIVED_SHUTDOWN 2

// SSL_get_shutdown returns a bitmask with a subset of |SSL_SENT_SHUTDOWN| and
// |SSL_RECEIVED_SHUTDOWN| to query whether close_notify was sent or received,
// respectively.
OPENSSL_EXPORT int SSL_get_shutdown(const SSL *ssl);

// SSL_get_peer_signature_algorithm returns the signature algorithm used by the
// peer. If not applicable, it returns zero.
OPENSSL_EXPORT uint16_t SSL_get_peer_signature_algorithm(const SSL *ssl);

// SSL_get_peer_signature_nid sets |psig_nid| to the NID of the digest used by
// the peer to sign their TLS messages. Returns one on success and zero on
// failure.
OPENSSL_EXPORT int SSL_get_peer_signature_nid(const SSL *ssl, int *psig_nid);

// SSL_get_peer_signature_type_nid sets |psigtype_nid| to the signature type
// used by the peer to sign their TLS messages. The signature type is the NID of
// the public key type used for signing. Returns one on success and zero on
// failure.
OPENSSL_EXPORT int SSL_get_peer_signature_type_nid(const SSL *ssl,
                                                   int *psigtype_nid);

// SSL_get_client_random writes up to |max_out| bytes of the most recent
// handshake's client_random to |out| and returns the number of bytes written.
// If |max_out| is zero, it returns the size of the client_random.
OPENSSL_EXPORT size_t SSL_get_client_random(const SSL *ssl, uint8_t *out,
                                            size_t max_out);

// SSL_get_server_random writes up to |max_out| bytes of the most recent
// handshake's server_random to |out| and returns the number of bytes written.
// If |max_out| is zero, it returns the size of the server_random.
OPENSSL_EXPORT size_t SSL_get_server_random(const SSL *ssl, uint8_t *out,
                                            size_t max_out);

// SSL_get_pending_cipher returns the cipher suite for the current handshake or
// NULL if one has not been negotiated yet or there is no pending handshake.
OPENSSL_EXPORT const SSL_CIPHER *SSL_get_pending_cipher(const SSL *ssl);

// SSL_set_retain_only_sha256_of_client_certs, on a server, sets whether only
// the SHA-256 hash of peer's certificate should be saved in memory and in the
// session. This can save memory, ticket size and session cache space. If
// enabled, |SSL_get_peer_certificate| will return NULL after the handshake
// completes. See |SSL_SESSION_has_peer_sha256| and
// |SSL_SESSION_get0_peer_sha256| to query the hash.
OPENSSL_EXPORT void SSL_set_retain_only_sha256_of_client_certs(SSL *ssl,
                                                               int enable);

// SSL_CTX_set_retain_only_sha256_of_client_certs, on a server, sets whether
// only the SHA-256 hash of peer's certificate should be saved in memory and in
// the session. This can save memory, ticket size and session cache space. If
// enabled, |SSL_get_peer_certificate| will return NULL after the handshake
// completes. See |SSL_SESSION_has_peer_sha256| and
// |SSL_SESSION_get0_peer_sha256| to query the hash.
OPENSSL_EXPORT void SSL_CTX_set_retain_only_sha256_of_client_certs(SSL_CTX *ctx,
                                                                   int enable);

// SSL_CTX_set_grease_enabled configures whether sockets on |ctx| should enable
// GREASE. See RFC 8701.
OPENSSL_EXPORT void SSL_CTX_set_grease_enabled(SSL_CTX *ctx, int enabled);

// SSL_CTX_set_permute_extensions configures whether sockets on |ctx| should
// permute extensions. For now, this is only implemented for the ClientHello.
OPENSSL_EXPORT void SSL_CTX_set_permute_extensions(SSL_CTX *ctx, int enabled);

// SSL_set_permute_extensions configures whether sockets on |ssl| should
// permute extensions. For now, this is only implemented for the ClientHello.
OPENSSL_EXPORT void SSL_set_permute_extensions(SSL *ssl, int enabled);

// SSL_max_seal_overhead returns the maximum overhead, in bytes, of sealing a
// record with |ssl|.
OPENSSL_EXPORT size_t SSL_max_seal_overhead(const SSL *ssl);

// SSL_CTX_set_false_start_allowed_without_alpn configures whether connections
// on |ctx| may use False Start (if |SSL_MODE_ENABLE_FALSE_START| is enabled)
// without negotiating ALPN.
OPENSSL_EXPORT void SSL_CTX_set_false_start_allowed_without_alpn(SSL_CTX *ctx,
                                                                 int allowed);

// SSL_used_hello_retry_request returns one if the TLS 1.3 HelloRetryRequest
// message has been either sent by the server or received by the client. It
// returns zero otherwise.
OPENSSL_EXPORT int SSL_used_hello_retry_request(const SSL *ssl);

// SSL_set_jdk11_workaround configures whether to workaround various bugs in
// JDK 11's TLS 1.3 implementation by disabling TLS 1.3 for such clients.
//
// https://bugs.openjdk.java.net/browse/JDK-8211806
// https://bugs.openjdk.java.net/browse/JDK-8212885
// https://bugs.openjdk.java.net/browse/JDK-8213202
OPENSSL_EXPORT void SSL_set_jdk11_workaround(SSL *ssl, int enable);

// SSL_set_check_client_certificate_type configures whether the client, in
// TLS 1.2 and below, will check its certificate against the server's requested
// certificate types.
//
// By default, this option is enabled. If disabled, certificate selection within
// the library may not function correctly. This flag is provided temporarily in
// case of compatibility issues. It will be removed sometime after June 2024.
OPENSSL_EXPORT void SSL_set_check_client_certificate_type(SSL *ssl, int enable);


// SSL Stat Counters.

// SSL_CTX_sess_connect returns the number of started SSL/TLS handshakes in
// client mode.
OPENSSL_EXPORT int SSL_CTX_sess_connect(const SSL_CTX *ctx);

// SSL_CTX_sess_connect_good returns the number of successfully established
// SSL/TLS sessions in client mode.
OPENSSL_EXPORT int SSL_CTX_sess_connect_good(const SSL_CTX *ctx);

// SSL_CTX_sess_connect_renegotiate returns the number of started renegotiations
// in client mode.
OPENSSL_EXPORT int SSL_CTX_sess_connect_renegotiate(const SSL_CTX *ctx);

// SSL_CTX_sess_accept returns the number of started SSL/TLS handshakes in
// server mode.
OPENSSL_EXPORT int SSL_CTX_sess_accept(const SSL_CTX *ctx);

// SSL_CTX_sess_accept_renegotiate returns zero. AWS-LC does not support server
// side renegotiations.
OPENSSL_EXPORT int SSL_CTX_sess_accept_renegotiate(const SSL_CTX *ctx);

// SSL_CTX_sess_accept_good returns the number of successfully established
// SSL/TLS sessions in server mode.
OPENSSL_EXPORT int SSL_CTX_sess_accept_good(const SSL_CTX *ctx);

// SSL_CTX_sess_hits returns the number of successfully reused sessions, from
// both session cache and session tickets.
OPENSSL_EXPORT int SSL_CTX_sess_hits(const SSL_CTX *ctx);

// SSL_CTX_sess_cb_hits returns the number of successfully retrieved sessions
// from the external session cache in server mode.
OPENSSL_EXPORT int SSL_CTX_sess_cb_hits(const SSL_CTX *ctx);

// SSL_CTX_sess_misses returns the number of sessions proposed by clients that
// were not found in the internal session cache in server mode.
OPENSSL_EXPORT int SSL_CTX_sess_misses(const SSL_CTX *ctx);

// SSL_CTX_sess_timeouts returns the number of sessions proposed by clients and
// either found in the internal or external session cache in server mode, but
// that were invalid due to timeout.
OPENSSL_EXPORT int SSL_CTX_sess_timeouts(const SSL_CTX *ctx);

// SSL_CTX_sess_cache_full returns the number of sessions that were removed
// because the maximum session cache size was exceeded.
OPENSSL_EXPORT int SSL_CTX_sess_cache_full(const SSL_CTX *ctx);


// SSL BIO methods

// BIO_f_ssl returns a |BIO_METHOD| that can wrap an |SSL*| in a |BIO*|. Note
// that this has quite different behaviour from the version in OpenSSL (notably
// that it doesn't try to auto renegotiate). There is also no current support
// for the |BIO_set_ssl*| related functions in OpenSSL or |BIO_puts| with this
// BIO type within AWS-LC.
OPENSSL_EXPORT const BIO_METHOD *BIO_f_ssl(void);

// BIO_set_ssl sets |ssl| as the underlying connection for |bio|, which must
// have been created using |BIO_f_ssl|. If |take_owership| is true, |bio| will
// call |SSL_free| on |ssl| when closed. It returns one on success or something
// other than one on error.
OPENSSL_EXPORT long BIO_set_ssl(BIO *bio, SSL *ssl, int take_owership);

// BIO_get_ssl assigns the internal |SSL| of |bio| to |*ssl|. |*ssl| should
// not be freed. It returns one on success or something other than one on error.
OPENSSL_EXPORT long BIO_get_ssl(BIO *bio, SSL **ssl);

// BIO_new_ssl_connect uses |ctx| to return a newly allocated BIO chain with
// |BIO_new_ssl|, followed by a connect BIO.
//
// Note: This allocates a |BIO| with |BIO_f_ssl| to the user, so the same
// caveats hold true for this function as well. See |BIO_f_ssl| for more
// details.
OPENSSL_EXPORT BIO *BIO_new_ssl_connect(SSL_CTX *ctx);

// BIO_new_ssl returns a newly allocated SSL BIO created with |ctx|. A client
// SSL is created if |client| is non-zero, and a server is created if otherwise.
//
// Note: This allocates a |BIO| with |BIO_f_ssl| to the user, so the same
// caveats hold true for this function as well. See |BIO_f_ssl| for more
// details.
OPENSSL_EXPORT BIO *BIO_new_ssl(SSL_CTX *ctx, int client);


// Deprecated functions.

// SSL_library_init calls |CRYPTO_library_init| and returns one.
OPENSSL_EXPORT int SSL_library_init(void);

// SSL_CIPHER_description writes a description of |cipher| into |buf| and
// returns |buf|. If |buf| is NULL, it returns a newly allocated string, to be
// freed with |OPENSSL_free|, or NULL on error.
//
// The description includes a trailing newline and has the form:
// AES128-SHA              Kx=RSA      Au=RSA  Enc=AES(128)  Mac=SHA1
//
// Consider |SSL_CIPHER_standard_name| or |SSL_CIPHER_get_name| instead.
OPENSSL_EXPORT const char *SSL_CIPHER_description(const SSL_CIPHER *cipher,
                                                  char *buf, int len);

// SSL_CIPHER_get_version returns the string "TLSv1/SSLv3".
OPENSSL_EXPORT const char *SSL_CIPHER_get_version(const SSL_CIPHER *cipher);

// SSLv23_method calls |TLS_method|.
OPENSSL_EXPORT const SSL_METHOD *SSLv23_method(void);

// These version-specific methods behave exactly like |TLS_method| and
// |DTLS_method| except they also call |SSL_CTX_set_min_proto_version| and
// |SSL_CTX_set_max_proto_version| to lock connections to that protocol
// version.
OPENSSL_EXPORT const SSL_METHOD *TLSv1_method(void);
OPENSSL_EXPORT const SSL_METHOD *TLSv1_1_method(void);
OPENSSL_EXPORT const SSL_METHOD *TLSv1_2_method(void);
OPENSSL_EXPORT const SSL_METHOD *DTLSv1_method(void);
OPENSSL_EXPORT const SSL_METHOD *DTLSv1_2_method(void);

// These client- and server-specific methods call their corresponding generic
// methods.
OPENSSL_EXPORT const SSL_METHOD *TLS_server_method(void);
OPENSSL_EXPORT const SSL_METHOD *TLS_client_method(void);
OPENSSL_EXPORT const SSL_METHOD *SSLv23_server_method(void);
OPENSSL_EXPORT const SSL_METHOD *SSLv23_client_method(void);
OPENSSL_EXPORT const SSL_METHOD *TLSv1_server_method(void);
OPENSSL_EXPORT const SSL_METHOD *TLSv1_client_method(void);
OPENSSL_EXPORT const SSL_METHOD *TLSv1_1_server_method(void);
OPENSSL_EXPORT const SSL_METHOD *TLSv1_1_client_method(void);
OPENSSL_EXPORT const SSL_METHOD *TLSv1_2_server_method(void);
OPENSSL_EXPORT const SSL_METHOD *TLSv1_2_client_method(void);
OPENSSL_EXPORT const SSL_METHOD *DTLS_server_method(void);
OPENSSL_EXPORT const SSL_METHOD *DTLS_client_method(void);
OPENSSL_EXPORT const SSL_METHOD *DTLSv1_server_method(void);
OPENSSL_EXPORT const SSL_METHOD *DTLSv1_client_method(void);
OPENSSL_EXPORT const SSL_METHOD *DTLSv1_2_server_method(void);
OPENSSL_EXPORT const SSL_METHOD *DTLSv1_2_client_method(void);

// SSL_clear resets |ssl| to allow another connection and returns one on success
// or zero on failure. It returns most configuration state but releases memory
// associated with the current connection.
//
// Free |ssl| and create a new one instead.
OPENSSL_EXPORT int SSL_clear(SSL *ssl);

// SSL_cutthrough_complete calls |SSL_in_false_start|.
OPENSSL_EXPORT int SSL_cutthrough_complete(const SSL *ssl);

// SSL_num_renegotiations calls |SSL_total_renegotiations|.
OPENSSL_EXPORT int SSL_num_renegotiations(const SSL *ssl);

// SSL_clear_num_renegotiations calls |SSL_total_renegotiations| and resets the
// total number of renegotiation handshakes performed by |ssl| to 0.
OPENSSL_EXPORT int SSL_clear_num_renegotiations(const SSL *ssl);

// SSL_CTX_get_read_ahead returns 1 if |ctx| is not null and read ahead is
// enabled, otherwise it returns 0.
OPENSSL_EXPORT int SSL_CTX_get_read_ahead(const SSL_CTX *ctx);

// SSL_CTX_set_read_ahead enables or disables read ahead on |ctx|:
// if |yes| is 1 it enables read ahead and returns 1,
// if |yes| is 0 it disables read ahead and returns 1,
// if |yes| is any other value nothing is changed and 0 is returned.
//
// When read ahead is enabled all future reads will be up to the buffer size configured
// with |SSL_CTX_set_default_read_buffer_len|, the default buffer size is
// |SSL3_RT_MAX_PLAIN_LENGTH| + |SSL3_RT_MAX_ENCRYPTED_OVERHEAD| = 16704 bytes.
//
// Read ahead should only be enabled on non-blocking IO sources configured with |SSL_set_bio|.
// When read ahead is enabled AWS-LC will make reads for potentially more data than is
// avaliable in the BIO with the assumption a partial read will be returned. If
// a blocking BIO is used and never returns the read could get stuck forever.
OPENSSL_EXPORT int SSL_CTX_set_read_ahead(SSL_CTX *ctx, int yes);

// SSL_get_read_ahead returns 1 if |ssl| is not null and read ahead is enabled
// otherwise it returns 0.
OPENSSL_EXPORT int SSL_get_read_ahead(const SSL *ssl);

// SSL_set_read_ahead enables or disables read ahead on |ssl|:
// if |yes| is 1 it enables read ahead and returns 1,
// if |yes| is 0 it disables read ahead and returns 1,
// if |yes| is any other value nothing is changed and 0 is returned.
//
// When read ahead is enabled all future reads will be for up to the buffer size configured
// with |SSL_CTX_set_default_read_buffer_len|. The default buffer size  is
// |SSL3_RT_MAX_PLAIN_LENGTH| + |SSL3_RT_MAX_ENCRYPTED_OVERHEAD| = 16704 bytes
//
// Read ahead should only be enabled on non-blocking IO sources configured with |SSL_set_bio|,
// when read ahead is enabled AWS-LC will make reads for potentially more data than is
// available in the BIO.
OPENSSL_EXPORT int SSL_set_read_ahead(SSL *ssl, int yes);

// SSL_CTX_set_default_read_buffer_len sets the size of the buffer reads will use on
// |ctx| if read ahead has been enabled. 0 is the minimum and 65535 is the maximum.
// A |len| of 0 is the same behavior as read ahead turned off: each call to
// |SSL_read| reads the amount specified in the TLS Record Header.
OPENSSL_EXPORT int SSL_CTX_set_default_read_buffer_len(SSL_CTX *ctx, size_t len);

// SSL_set_default_read_buffer_len sets the size of the buffer reads will use on
// |ssl| if read ahead has been enabled. 0 is the minimum and 65535 is the maximum.
// A |len| of 0 is the same behavior as read ahead turned off: each call to
// |SSL_read| reads the amount specified in the TLS Record Header.
OPENSSL_EXPORT int SSL_set_default_read_buffer_len(SSL *ssl, size_t len);

// SSL_MODE_HANDSHAKE_CUTTHROUGH is the same as SSL_MODE_ENABLE_FALSE_START.
#define SSL_MODE_HANDSHAKE_CUTTHROUGH SSL_MODE_ENABLE_FALSE_START

// i2d_SSL_SESSION serializes |in|, as described in |i2d_SAMPLE|.
//
// Use |SSL_SESSION_to_bytes| instead.
OPENSSL_EXPORT int i2d_SSL_SESSION(SSL_SESSION *in, uint8_t **pp);

// d2i_SSL_SESSION parses a serialized session from the |length| bytes pointed
// to by |*pp|, as described in |d2i_SAMPLE|.
//
// Use |SSL_SESSION_from_bytes| instead.
OPENSSL_EXPORT SSL_SESSION *d2i_SSL_SESSION(SSL_SESSION **a, const uint8_t **pp,
                                            long length);

// i2d_SSL_SESSION_bio serializes |session| and writes the result to |bio|. It
// returns the number of bytes written on success and <= 0 on error.
OPENSSL_EXPORT int i2d_SSL_SESSION_bio(BIO *bio, const SSL_SESSION *session);

// d2i_SSL_SESSION_bio reads a serialized |SSL_SESSION| from |bio| and returns a
// newly-allocated |SSL_SESSION| or NULL on error. If |out| is not NULL, it also
// frees |*out| and sets |*out| to the new |SSL_SESSION|.
OPENSSL_EXPORT SSL_SESSION *d2i_SSL_SESSION_bio(BIO *bio, SSL_SESSION **out);

// SSL_CTX_set_tlsext_use_srtp calls |SSL_CTX_set_srtp_profiles|. It returns
// zero on success and one on failure.
//
// WARNING: this function is dangerous because it breaks the usual return value
// convention. Use |SSL_CTX_set_srtp_profiles| instead.
OPENSSL_EXPORT int SSL_CTX_set_tlsext_use_srtp(SSL_CTX *ctx,
                                               const char *profiles);

// SSL_set_tlsext_use_srtp calls |SSL_set_srtp_profiles|. It returns zero on
// success and one on failure.
//
// WARNING: this function is dangerous because it breaks the usual return value
// convention. Use |SSL_set_srtp_profiles| instead.
OPENSSL_EXPORT int SSL_set_tlsext_use_srtp(SSL *ssl, const char *profiles);

// SSL_CTX_set1_sigalgs takes |num_values| ints and interprets them as pairs
// where the first is the nid of a hash function and the second is an
// |EVP_PKEY_*| value. It configures the signature algorithm preferences for
// |ctx| based on them and returns one on success or zero on error.
//
// This API is compatible with OpenSSL. However, BoringSSL-specific code should
// prefer |SSL_CTX_set_signing_algorithm_prefs| because it's clearer and it's
// more convenient to codesearch for specific algorithm values.
OPENSSL_EXPORT int SSL_CTX_set1_sigalgs(SSL_CTX *ctx, const int *values,
                                        size_t num_values);

// SSL_set1_sigalgs takes |num_values| ints and interprets them as pairs where
// the first is the nid of a hash function and the second is an |EVP_PKEY_*|
// value. It configures the signature algorithm preferences for |ssl| based on
// them and returns one on success or zero on error.
//
// This API is compatible with OpenSSL. However, BoringSSL-specific code should
// prefer |SSL_CTX_set_signing_algorithm_prefs| because it's clearer and it's
// more convenient to codesearch for specific algorithm values.
OPENSSL_EXPORT int SSL_set1_sigalgs(SSL *ssl, const int *values,
                                    size_t num_values);

// SSL_CTX_set1_sigalgs_list takes a textual specification of a set of signature
// algorithms and configures them on |ctx|. It returns one on success and zero
// on error. See
// https://www.openssl.org/docs/man1.1.0/man3/SSL_CTX_set1_sigalgs_list.html for
// a description of the text format. Also note that TLS 1.3 names (e.g.
// "rsa_pkcs1_md5_sha1") can also be used (as in OpenSSL, although OpenSSL
// doesn't document that).
//
// This API is compatible with OpenSSL. However, BoringSSL-specific code should
// prefer |SSL_CTX_set_signing_algorithm_prefs| because it's clearer and it's
// more convenient to codesearch for specific algorithm values.
OPENSSL_EXPORT int SSL_CTX_set1_sigalgs_list(SSL_CTX *ctx, const char *str);

// SSL_set1_sigalgs_list takes a textual specification of a set of signature
// algorithms and configures them on |ssl|. It returns one on success and zero
// on error. See
// https://www.openssl.org/docs/man1.1.0/man3/SSL_CTX_set1_sigalgs_list.html for
// a description of the text format. Also note that TLS 1.3 names (e.g.
// "rsa_pkcs1_md5_sha1") can also be used (as in OpenSSL, although OpenSSL
// doesn't document that).
//
// This API is compatible with OpenSSL. However, BoringSSL-specific code should
// prefer |SSL_CTX_set_signing_algorithm_prefs| because it's clearer and it's
// more convenient to codesearch for specific algorithm values.
OPENSSL_EXPORT int SSL_set1_sigalgs_list(SSL *ssl, const char *str);

// SSL_SESSION_print prints the contents of |sess| to |bp|.
OPENSSL_EXPORT int SSL_SESSION_print(BIO *bp, const SSL_SESSION *sess);

#define SSL_set_app_data(s, arg) (SSL_set_ex_data(s, 0, (char *)(arg)))
#define SSL_get_app_data(s) (SSL_get_ex_data(s, 0))
#define SSL_SESSION_set_app_data(s, a) \
  (SSL_SESSION_set_ex_data(s, 0, (char *)(a)))
#define SSL_SESSION_get_app_data(s) (SSL_SESSION_get_ex_data(s, 0))
#define SSL_CTX_get_app_data(ctx) (SSL_CTX_get_ex_data(ctx, 0))
#define SSL_CTX_set_app_data(ctx, arg) \
  (SSL_CTX_set_ex_data(ctx, 0, (char *)(arg)))

#define OpenSSL_add_ssl_algorithms() SSL_library_init()
#define SSLeay_add_ssl_algorithms() SSL_library_init()

#define SSL_get_cipher(ssl) SSL_CIPHER_get_name(SSL_get_current_cipher(ssl))
#define SSL_get_cipher_bits(ssl, out_alg_bits) \
  SSL_CIPHER_get_bits(SSL_get_current_cipher(ssl), out_alg_bits)
#define SSL_get_cipher_version(ssl) \
  SSL_CIPHER_get_version(SSL_get_current_cipher(ssl))
#define SSL_get_cipher_name(ssl) \
  SSL_CIPHER_get_name(SSL_get_current_cipher(ssl))
#define SSL_get_time(session) SSL_SESSION_get_time(session)
#define SSL_set_time(session, time) SSL_SESSION_set_time((session), (time))
#define SSL_get_timeout(session) SSL_SESSION_get_timeout(session)
#define SSL_set_timeout(session, timeout) \
  SSL_SESSION_set_timeout((session), (timeout))

// SSL_cache_hit calls |SSL_session_reused|.
OPENSSL_EXPORT int SSL_cache_hit(SSL *ssl);

// SSL_get_default_timeout returns |SSL_DEFAULT_SESSION_TIMEOUT|.
OPENSSL_EXPORT long SSL_get_default_timeout(const SSL *ssl);

// SSL_get_version returns a string describing the TLS version used by |ssl|.
// For example, "TLSv1.2" or "DTLSv1".
OPENSSL_EXPORT const char *SSL_get_version(const SSL *ssl);

// SSL_get_all_version_names outputs a list of possible strings
// |SSL_get_version| may return in this version of BoringSSL. It writes at most
// |max_out| entries to |out| and returns the total number it would have
// written, if |max_out| had been large enough. |max_out| may be initially set
// to zero to size the output.
//
// This function is only intended to help initialize tables in callers that want
// possible strings pre-declared. This list would not be suitable to set a list
// of supported features. It is in no particular order, and may contain
// placeholder, experimental, or deprecated values that do not apply to every
// caller. Future versions of BoringSSL may also return strings not in this
// list, so this does not apply if, say, sending strings across services.
OPENSSL_EXPORT size_t SSL_get_all_version_names(const char **out,
                                                size_t max_out);

// SSL_get_cipher_list returns the name of the |n|th cipher in the output of
// |SSL_get_ciphers| or NULL if out of range. Use |SSL_get_ciphers| instead.
OPENSSL_EXPORT const char *SSL_get_cipher_list(const SSL *ssl, int n);

// SSL_CTX_set_client_cert_cb sets a callback which is called on the client if
// the server requests a client certificate and none is configured. On success,
// the callback should return one and set |*out_x509| to |*out_pkey| to a leaf
// certificate and private key, respectively, passing ownership. It should
// return zero to send no certificate and -1 to fail or pause the handshake. If
// the handshake is paused, |SSL_get_error| will return
// |SSL_ERROR_WANT_X509_LOOKUP|.
//
// The callback may call |SSL_get0_certificate_types| and
// |SSL_get_client_CA_list| for information on the server's certificate request.
//
// Use |SSL_CTX_set_cert_cb| instead. Configuring intermediate certificates with
// this function is confusing. This callback may not be registered concurrently
// with |SSL_CTX_set_cert_cb| or |SSL_set_cert_cb|.
OPENSSL_EXPORT void SSL_CTX_set_client_cert_cb(
    SSL_CTX *ctx, int (*cb)(SSL *ssl, X509 **out_x509, EVP_PKEY **out_pkey));

#define SSL_NOTHING SSL_ERROR_NONE
#define SSL_WRITING SSL_ERROR_WANT_WRITE
#define SSL_READING SSL_ERROR_WANT_READ

// SSL_want returns one of the above values to determine what the most recent
// operation on |ssl| was blocked on. Use |SSL_get_error| instead.
OPENSSL_EXPORT int SSL_want(const SSL *ssl);

#define SSL_want_read(ssl) (SSL_want(ssl) == SSL_READING)
#define SSL_want_write(ssl) (SSL_want(ssl) == SSL_WRITING)

// SSL_get_finished writes up to |count| bytes of the Finished message sent by
// |ssl| to |buf|. It returns the total untruncated length or zero if none has
// been sent yet.
//
// Use |SSL_get_tls_unique| instead.
OPENSSL_EXPORT size_t SSL_get_finished(const SSL *ssl, void *buf, size_t count);

// SSL_get_peer_finished writes up to |count| bytes of the Finished message
// received from |ssl|'s peer to |buf|. It returns the total untruncated length
// or zero if none has been received yet.
//
// Use |SSL_get_tls_unique| instead.
OPENSSL_EXPORT size_t SSL_get_peer_finished(const SSL *ssl, void *buf,
                                            size_t count);

// SSL_alert_type_string returns an unintelligible 1 letter string description
// of |value| as an alert type (W or F). Use |SSL_alert_type_string_long|
// instead.
OPENSSL_EXPORT const char *SSL_alert_type_string(int value);

// SSL_alert_desc_string returns an unintelligible 2 letter string description
// of |value|. Use |SSL_alert_desc_string_long| instead.
OPENSSL_EXPORT const char *SSL_alert_desc_string(int value);

// SSL_state_string returns |SSL_state_string_long|.
OPENSSL_EXPORT const char *SSL_state_string(const SSL *ssl);

// SSL_TXT_* expand to strings.
#define SSL_TXT_MEDIUM "MEDIUM"
#define SSL_TXT_HIGH "HIGH"
#define SSL_TXT_FIPS "FIPS"
#define SSL_TXT_kRSA "kRSA"
#define SSL_TXT_kDHE "kDHE"
#define SSL_TXT_kEDH "kEDH"
#define SSL_TXT_kECDHE "kECDHE"
#define SSL_TXT_kEECDH "kEECDH"
#define SSL_TXT_kPSK "kPSK"
#define SSL_TXT_aRSA "aRSA"
#define SSL_TXT_aECDSA "aECDSA"
#define SSL_TXT_aPSK "aPSK"
#define SSL_TXT_DH "DH"
#define SSL_TXT_DHE "DHE"
#define SSL_TXT_EDH "EDH"
#define SSL_TXT_RSA "RSA"
#define SSL_TXT_ECDH "ECDH"
#define SSL_TXT_ECDHE "ECDHE"
#define SSL_TXT_EECDH "EECDH"
#define SSL_TXT_ECDSA "ECDSA"
#define SSL_TXT_PSK "PSK"
#define SSL_TXT_3DES "3DES"
#define SSL_TXT_RC4 "RC4"
#define SSL_TXT_AES128 "AES128"
#define SSL_TXT_AES256 "AES256"
#define SSL_TXT_AES "AES"
#define SSL_TXT_AES_GCM "AESGCM"
#define SSL_TXT_CHACHA20 "CHACHA20"
#define SSL_TXT_MD5 "MD5"
#define SSL_TXT_SHA1 "SHA1"
#define SSL_TXT_SHA "SHA"
#define SSL_TXT_SHA256 "SHA256"
#define SSL_TXT_SHA384 "SHA384"
#define SSL_TXT_SSLV3 "SSLv3"
#define SSL_TXT_TLSV1 "TLSv1"
#define SSL_TXT_TLSV1_1 "TLSv1.1"
#define SSL_TXT_TLSV1_2 "TLSv1.2"
#define SSL_TXT_TLSV1_3 "TLSv1.3"
#define SSL_TXT_ALL "ALL"
#define SSL_TXT_CMPDEF "COMPLEMENTOFDEFAULT"

typedef struct ssl_conf_ctx_st SSL_CONF_CTX;

// SSL_state returns |SSL_ST_INIT| if a handshake is in progress and |SSL_ST_OK|
// otherwise.
//
// Use |SSL_is_init| instead.
OPENSSL_EXPORT int SSL_state(const SSL *ssl);

#define SSL_get_state(ssl) SSL_state(ssl)

// SSL_set_shutdown causes |ssl| to behave as if the shutdown bitmask (see
// |SSL_get_shutdown|) were |mode|. This may be used to skip sending or
// receiving close_notify in |SSL_shutdown| by causing the implementation to
// believe the events already happened.
//
// Note: |SSL_set_shutdown| cannot be used to unset a bit that has already
// been set in AWS-LC. Doing so will be ignored.
//
// Use |SSL_CTX_set_quiet_shutdown| instead.
OPENSSL_EXPORT void SSL_set_shutdown(SSL *ssl, int mode);

// SSL_CTX_set_tmp_ecdh calls |SSL_CTX_set1_groups| with a one-element list
// containing |ec_key|'s curve. The remainder of |ec_key| is ignored.
OPENSSL_EXPORT int SSL_CTX_set_tmp_ecdh(SSL_CTX *ctx, const EC_KEY *ec_key);

// SSL_set_tmp_ecdh calls |SSL_set1_groups| with a one-element list containing
// |ec_key|'s curve. The remainder of |ec_key| is ignored.
OPENSSL_EXPORT int SSL_set_tmp_ecdh(SSL *ssl, const EC_KEY *ec_key);

#if !defined(OPENSSL_NO_FILESYSTEM)
// SSL_add_dir_cert_subjects_to_stack lists files in directory |dir|. It calls
// |SSL_add_file_cert_subjects_to_stack| on each file and returns one on success
// or zero on error. This function is deprecated.
OPENSSL_EXPORT int SSL_add_dir_cert_subjects_to_stack(STACK_OF(X509_NAME) *out,
                                                      const char *dir);
#endif

// SSL_CTX_enable_tls_channel_id calls |SSL_CTX_set_tls_channel_id_enabled|.
OPENSSL_EXPORT int SSL_CTX_enable_tls_channel_id(SSL_CTX *ctx);

// SSL_enable_tls_channel_id calls |SSL_set_tls_channel_id_enabled|.
OPENSSL_EXPORT int SSL_enable_tls_channel_id(SSL *ssl);

// SSL_get_session returns a non-owning pointer to |ssl|'s session. For
// historical reasons, which session it returns depends on |ssl|'s state.
//
// Prior to the start of the initial handshake, it returns the session the
// caller set with |SSL_set_session|. After the initial handshake has finished
// and if no additional handshakes are in progress, it returns the currently
// active session. Its behavior is undefined while a handshake is in progress.
//
// If trying to add new sessions to an external session cache, use
// |SSL_CTX_sess_set_new_cb| instead. In particular, using the callback is
// required as of TLS 1.3. For compatibility, this function will return an
// unresumable session which may be cached, but will never be resumed.
//
// If querying properties of the connection, use APIs on the |SSL| object.
OPENSSL_EXPORT SSL_SESSION *SSL_get_session(const SSL *ssl);

// SSL_get0_session is an alias for |SSL_get_session|.
#define SSL_get0_session SSL_get_session

// SSL_get1_session acts like |SSL_get_session| but returns a new reference to
// the session.
OPENSSL_EXPORT SSL_SESSION *SSL_get1_session(SSL *ssl);

#define OPENSSL_INIT_NO_LOAD_SSL_STRINGS 0
#define OPENSSL_INIT_LOAD_SSL_STRINGS 0
#define OPENSSL_INIT_SSL_DEFAULT 0

// OPENSSL_init_ssl calls |CRYPTO_library_init| and returns one.
OPENSSL_EXPORT int OPENSSL_init_ssl(uint64_t opts,
                                    const OPENSSL_INIT_SETTINGS *settings);

// The following constants are legacy aliases for RSA-PSS with rsaEncryption
// keys. Use the new names instead.
#define SSL_SIGN_RSA_PSS_SHA256 SSL_SIGN_RSA_PSS_RSAE_SHA256
#define SSL_SIGN_RSA_PSS_SHA384 SSL_SIGN_RSA_PSS_RSAE_SHA384
#define SSL_SIGN_RSA_PSS_SHA512 SSL_SIGN_RSA_PSS_RSAE_SHA512

// SSL_set_tlsext_status_type configures a client to request OCSP stapling if
// |type| is |TLSEXT_STATUSTYPE_ocsp| and disables it otherwise. It returns one
// on success and zero if handshake configuration has already been shed.
//
// Use |SSL_enable_ocsp_stapling| instead.
OPENSSL_EXPORT int SSL_set_tlsext_status_type(SSL *ssl, int type);

// SSL_get_tlsext_status_type returns |TLSEXT_STATUSTYPE_ocsp| if the client
// requested OCSP stapling and |TLSEXT_STATUSTYPE_nothing| otherwise. On the
// client, this reflects whether OCSP stapling was enabled via, e.g.,
// |SSL_set_tlsext_status_type|. On the server, this is determined during the
// handshake. It may be queried in callbacks set by |SSL_CTX_set_cert_cb|. The
// result is undefined after the handshake completes.
OPENSSL_EXPORT int SSL_get_tlsext_status_type(const SSL *ssl);

// SSL_set_tlsext_status_ocsp_resp sets the OCSP response. It returns one on
// success and zero on error. On success, |ssl| takes ownership of |resp|, which
// must have been allocated by |OPENSSL_malloc|.
//
// Use |SSL_set_ocsp_response| instead.
OPENSSL_EXPORT int SSL_set_tlsext_status_ocsp_resp(SSL *ssl, uint8_t *resp,
                                                   size_t resp_len);

// SSL_get_tlsext_status_ocsp_resp sets |*out| to point to the OCSP response
// from the server. It returns the length of the response. If there was no
// response, it sets |*out| to NULL and returns zero.
//
// Use |SSL_get0_ocsp_response| instead.
//
// WARNING: the returned data is not guaranteed to be well formed.
OPENSSL_EXPORT size_t SSL_get_tlsext_status_ocsp_resp(const SSL *ssl,
                                                      const uint8_t **out);

// SSL_CTX_set_tlsext_status_cb configures the legacy OpenSSL OCSP callback and
// returns one. Though the type signature is the same, this callback has
// different behavior for client and server connections:
//
// For clients, the callback is called after certificate verification. It should
// return one for success, zero for a bad OCSP response, and a negative number
// for internal error. Instead, handle this as part of certificate verification.
// (Historically, OpenSSL verified certificates just before parsing stapled OCSP
// responses, but BoringSSL fixes this ordering. All server credentials are
// available during verification.)
//
// Do not use this callback as a server. It is provided for compatibility
// purposes only. For servers, it is called to configure server credentials. It
// should return |SSL_TLSEXT_ERR_OK| on success, |SSL_TLSEXT_ERR_NOACK| to
// ignore OCSP requests, or |SSL_TLSEXT_ERR_ALERT_FATAL| on error. It is usually
// used to fetch OCSP responses on demand, which is not ideal. Instead, treat
// OCSP responses like other server credentials, such as certificates or SCT
// lists. Configure, store, and refresh them eagerly. This avoids downtime if
// the CA's OCSP responder is briefly offline.
OPENSSL_EXPORT int SSL_CTX_set_tlsext_status_cb(SSL_CTX *ctx,
                                                int (*callback)(SSL *ssl,
                                                                void *arg));

// SSL_CTX_get_tlsext_status_cb returns the legacy OpenSSL OCSP callback if set.
OPENSSL_EXPORT int SSL_CTX_get_tlsext_status_cb(SSL_CTX *ctx,
                                                int (**callback)(SSL *,
                                                                 void *));

// SSL_CTX_set_tlsext_status_arg sets additional data for
// |SSL_CTX_set_tlsext_status_cb|'s callback and returns one.
OPENSSL_EXPORT int SSL_CTX_set_tlsext_status_arg(SSL_CTX *ctx, void *arg);

// The following symbols are compatibility aliases for reason codes used when
// receiving an alert from the peer. Use the other names instead, which fit the
// naming convention.
//
// TODO(davidben): Fix references to |SSL_R_TLSV1_CERTIFICATE_REQUIRED| and
// remove the compatibility value. The others come from OpenSSL.
#define SSL_R_TLSV1_UNSUPPORTED_EXTENSION \
  SSL_R_TLSV1_ALERT_UNSUPPORTED_EXTENSION
#define SSL_R_TLSV1_CERTIFICATE_UNOBTAINABLE \
  SSL_R_TLSV1_ALERT_CERTIFICATE_UNOBTAINABLE
#define SSL_R_TLSV1_UNRECOGNIZED_NAME SSL_R_TLSV1_ALERT_UNRECOGNIZED_NAME
#define SSL_R_TLSV1_BAD_CERTIFICATE_STATUS_RESPONSE \
  SSL_R_TLSV1_ALERT_BAD_CERTIFICATE_STATUS_RESPONSE
#define SSL_R_TLSV1_BAD_CERTIFICATE_HASH_VALUE \
  SSL_R_TLSV1_ALERT_BAD_CERTIFICATE_HASH_VALUE
#define SSL_R_TLSV1_CERTIFICATE_REQUIRED SSL_R_TLSV1_ALERT_CERTIFICATE_REQUIRED

// The following symbols are compatibility aliases for |SSL_GROUP_*|.
#define SSL_CURVE_SECP224R1 SSL_GROUP_SECP224R1
#define SSL_CURVE_SECP256R1 SSL_GROUP_SECP256R1
#define SSL_CURVE_SECP384R1 SSL_GROUP_SECP384R1
#define SSL_CURVE_SECP521R1 SSL_GROUP_SECP521R1
#define SSL_CURVE_X25519 SSL_GROUP_X25519

// TLSEXT_nid_unknown is a constant used in OpenSSL for
// |SSL_get_negotiated_group| to return an unrecognized group. AWS-LC never
// returns this value, but we define this constant for compatibility.
#define TLSEXT_nid_unknown 0x1000000

// SSL_get_curve_id calls |SSL_get_group_id|.
OPENSSL_EXPORT uint16_t SSL_get_curve_id(const SSL *ssl);

// SSL_get_curve_name calls |SSL_get_group_name|.
OPENSSL_EXPORT const char *SSL_get_curve_name(uint16_t curve_id);

// SSL_get_all_curve_names calls |SSL_get_all_group_names|.
OPENSSL_EXPORT size_t SSL_get_all_curve_names(const char **out, size_t max_out);

// SSL_CTX_set1_curves calls |SSL_CTX_set1_groups|.
OPENSSL_EXPORT int SSL_CTX_set1_curves(SSL_CTX *ctx, const int *curves,
                                       size_t num_curves);

// SSL_set1_curves calls |SSL_set1_groups|.
OPENSSL_EXPORT int SSL_set1_curves(SSL *ssl, const int *curves,
                                   size_t num_curves);

// SSL_CTX_set1_curves_list calls |SSL_CTX_set1_groups_list|.
OPENSSL_EXPORT int SSL_CTX_set1_curves_list(SSL_CTX *ctx, const char *curves);

// SSL_set1_curves_list calls |SSL_set1_groups_list|.
OPENSSL_EXPORT int SSL_set1_curves_list(SSL *ssl, const char *curves);


// No-op Configuration Flags
//
// The following flags do nothing and are included only to make it easier to
// compile code with AWS-LC. Flags defined within this section are not
// configurable and the state of AWS-LC does not change when used.

// SSL_MODE_AUTO_RETRY is ON by default in AWS-LC and OpenSSL. This will let
// the application automatically retry if the transport is blocking.
// Read/write operations will only return after the handshake and successful
// completion.
#define SSL_MODE_AUTO_RETRY 0

// SSL_MODE_RELEASE_BUFFERS is ON by default in AWS-LC. When a read/write
// buffer is no longer needed for a given SSL, the memory holding it is
// released.
#define SSL_MODE_RELEASE_BUFFERS 0

// SSL_MODE_SEND_CLIENTHELLO_TIME is OFF by default in AWS-LC. Turning this ON
// in OpenSSL sends the current time in the Random fields of the ClientHello
// records is sent.
#define SSL_MODE_SEND_CLIENTHELLO_TIME 0

// SSL_MODE_SEND_SERVERHELLO_TIME is ON by default in AWS-LC. The current time
// in the Random fields of the ServerHello records is sent.
#define SSL_MODE_SEND_SERVERHELLO_TIME 0

// SSL_OP_ALL is OFF by default in AWS-LC. Turning this ON in OpenSSL enables
// all the less harmless bug workarounds that OpenSSL has had historically.
//
// See https://www.openssl.org/docs/manmaster/man3/SSL_CTX_set_options.html
// for more details.
#define SSL_OP_ALL 0

// SSL_OP_ALLOW_UNSAFE_LEGACY_RENEGOTIATION is OFF by default in AWS-LC.
// Turning this ON in OpenSSL allows legacy insecure renegotiation for
// unpatched clients and servers and is intentionally not supported in AWS-LC.
#define SSL_OP_ALLOW_UNSAFE_LEGACY_RENEGOTIATION 0

// SSL_OP_CRYPTOPRO_TLSEXT_BUG is OFF by default in AWS-LC. Turning this ON in
// OpenSSL lets the server add a server-hello extension from early version of
// the cryptopro draft, when the GOST ciphersuite is negotiated. Required for
// interoperability with CryptoPro CSP 3.x.
//
// Note: AWS-LC does not support GOST ciphersuites.
#define SSL_OP_CRYPTOPRO_TLSEXT_BUG 0

// SSL_OP_DONT_INSERT_EMPTY_FRAGMENTS is ON by default in AWS-LC. This
// disables a countermeasure against a SSL 3.0/TLS 1.0 protocol vulnerability
// affecting CBC ciphers, which cannot be handled by some broken SSL
// implementations.
#define SSL_OP_DONT_INSERT_EMPTY_FRAGMENTS 0

// SSL_OP_LEGACY_SERVER_CONNECT is OFF by default in AWS-LC. Turning this ON in
// OpenSSL allows  legacy insecure renegotiation between OpenSSL and unpatched
// servers "only" and is intentionally not supported in AWS-LC.
#define SSL_OP_LEGACY_SERVER_CONNECT 0

// SSL_OP_NO_COMPRESSION is ON by default in AWS-LC. AWS-LC intentionally does
// not support TLS record compressions.
#define SSL_OP_NO_COMPRESSION 0

// SSL_OP_NO_RENEGOTIATION is ON by default in AWS-LC. This disables all
// renegotiation in TLSv1.2 and earlier.
// To enable renegotiation, use |SSL_set_renegotiate_mode|.
#define SSL_OP_NO_RENEGOTIATION 0  // ssl_renegotiate_never is the default

// SSL_OP_NO_SESSION_RESUMPTION_ON_RENEGOTIATION is ON by default in AWS-LC.
// This always starts a new session when performing renegotiation as a server
// (i.e., session resumption requests are only accepted in the initial
// handshake).
// There is no support for renegototiation for a server in AWS-LC.
#define SSL_OP_NO_SESSION_RESUMPTION_ON_RENEGOTIATION 0

// SSL_OP_NO_SSLv2 is ON by default in AWS-LC. There is no support for SSLv2 in
// AWS-LC
#define SSL_OP_NO_SSLv2 0

// SSL_OP_NO_SSLv3 is ON by default in AWS-LC. There is no support for SSLv3 in
// AWS-LC
#define SSL_OP_NO_SSLv3 0

// SSL_OP_SAFARI_ECDHE_ECDSA_BUG is OFF by default in AWS-LC. Turning this ON in
// OpenSSL lets the application not prefer ECDHE-ECDSA ciphers when the client
// appears to be Safari on OSX.
//
// Note: OS X 10.8..10.8.3 broke support for ECDHE-ECDSA ciphers.
#define SSL_OP_SAFARI_ECDHE_ECDSA_BUG 0

// SSL_OP_TLSEXT_PADDING is OFF by default in AWS-LC. Turning this ON in OpenSSL
// adds a padding extension to ensure the ClientHello size is never between 256
// and 511 bytes in length. This is needed as a workaround for F5 terminators.
#define SSL_OP_TLSEXT_PADDING 0

// SSL_OP_TLS_ROLLBACK_BUG is OFF by default in AWS-LC. Turning this ON in
// OpenSSL disables version rollback attack detection and is intentionally not
// supported in AWS-LC.
#define SSL_OP_TLS_ROLLBACK_BUG 0

// SSL_VERIFY_CLIENT_ONCE is OFF by default in AWS-LC. Turning this ON in
// OpenSSL only requests a client certificate on the initial TLS handshake and
// is intentionally not supported in AWS-LC.
#define SSL_VERIFY_CLIENT_ONCE 0

// SSL_OP_TLSEXT_PADDING is OFF by default in AWS-LC. Turning this ON in
// OpenSSL adds a padding extension to ensure the ClientHello size is never
// between 256 and 511 bytes in length. This is needed as a workaround for some
// implementations.
#define SSL_OP_TLSEXT_PADDING 0

// SSL_OP_SAFARI_ECDHE_ECDSA_BUG is OFF by default in AWS-LC. Turning this ON in
// OpenSSL defers ECDHE-ECDSA ciphers when the client appears to be Safari on
// OSX. OSX 10.8 ~ 10.8.3 has broken support for ECDHE-ECDSA ciphers.
#define SSL_OP_SAFARI_ECDHE_ECDSA_BUG 0

// SSL_OP_CRYPTOPRO_TLSEXT_BUG is OFF by default in AWS-LC. Turning this ON in
// OpenSSL adds the server-hello extension from the early version of cryptopro
// draft when GOST ciphersuite is negotiated (which we don't support).
#define SSL_OP_CRYPTOPRO_TLSEXT_BUG 0

// The following have no effect in both AWS-LC and OpenSSL.
#define SSL_OP_EPHEMERAL_RSA 0
#define SSL_OP_MICROSOFT_BIG_SSLV3_BUFFER 0
#define SSL_OP_MICROSOFT_SESS_ID_BUG 0
#define SSL_OP_MSIE_SSLV2_RSA_PADDING 0
#define SSL_OP_NETSCAPE_CA_DN_BUG 0
#define SSL_OP_NETSCAPE_CHALLENGE_BUG 0
#define SSL_OP_NETSCAPE_DEMO_CIPHER_CHANGE_BUG 0
#define SSL_OP_NETSCAPE_REUSE_CIPHER_CHANGE_BUG 0
#define SSL_OP_PKCS1_CHECK_1 0
#define SSL_OP_PKCS1_CHECK_2 0
#define SSL_OP_SINGLE_DH_USE 0
#define SSL_OP_SINGLE_ECDH_USE 0
#define SSL_OP_SSLEAY_080_CLIENT_DH_BUG 0
#define SSL_OP_SSLREF2_REUSE_CERT_TYPE_BUG 0
#define SSL_OP_TLS_BLOCK_PADDING_BUG 0
#define SSL_OP_TLS_D5_BUG 0


// |SSL_COMP| and |COMP_METHOD| No-ops [Deprecated].
//
// Support for SSL compression has been completely removed and the following
// functions are only provided as no-ops for easier compatibility.

typedef void COMP_METHOD;
typedef struct ssl_comp_st SSL_COMP;

// SSL_COMP_get_compression_methods returns NULL.
OPENSSL_EXPORT OPENSSL_DEPRECATED STACK_OF(SSL_COMP) *
SSL_COMP_get_compression_methods(void);

// SSL_COMP_add_compression_method returns one.
OPENSSL_EXPORT OPENSSL_DEPRECATED int SSL_COMP_add_compression_method(
    int id, COMP_METHOD *cm);

// SSL_COMP_get_name returns NULL.
OPENSSL_EXPORT OPENSSL_DEPRECATED const char *SSL_COMP_get_name(
    const COMP_METHOD *comp);

// SSL_COMP_get0_name returns the |name| member of |comp|.
OPENSSL_EXPORT OPENSSL_DEPRECATED const char *SSL_COMP_get0_name(
    const SSL_COMP *comp);

// SSL_COMP_get_id returns the |id| member of |comp|.
OPENSSL_EXPORT OPENSSL_DEPRECATED int SSL_COMP_get_id(const SSL_COMP *comp);

// SSL_COMP_free_compression_methods does nothing.
OPENSSL_EXPORT OPENSSL_DEPRECATED void SSL_COMP_free_compression_methods(void);

// SSL_get_current_compression returns NULL.
OPENSSL_EXPORT OPENSSL_DEPRECATED const COMP_METHOD *
SSL_get_current_compression(SSL *ssl);

// SSL_get_current_expansion returns NULL.
OPENSSL_EXPORT OPENSSL_DEPRECATED const COMP_METHOD *SSL_get_current_expansion(
    SSL *ssl);

struct ssl_comp_st {
  int id;
  const char *name;
  char *method;
};

DEFINE_STACK_OF(SSL_COMP)

// PHA No-ops [Deprecated].

// SSL_verify_client_post_handshake is a no-op function for compatibility with
// OpenSSL. It always returns 0.
OPENSSL_EXPORT OPENSSL_DEPRECATED int SSL_verify_client_post_handshake(SSL *ssl);

// FFDH Ciphersuite No-ops [Deprecated].
//
// AWS-LC does not support the use of FFDH cipher suites in libssl. The
// following functions are only provided as no-ops for easier compatibility.

// SSL_CTX_set_tmp_dh returns 1.
//
// TODO (CryptoAlg-2398): Add |OPENSSL_DEPRECATED|. nginx defines -Werror and
// depends on this.
OPENSSL_EXPORT int SSL_CTX_set_tmp_dh(SSL_CTX *ctx, const DH *dh);

// SSL_set_tmp_dh returns 1.
OPENSSL_EXPORT OPENSSL_DEPRECATED int SSL_set_tmp_dh(SSL *ssl, const DH *dh);

// SSL_CTX_set_tmp_dh_callback does nothing.
OPENSSL_EXPORT OPENSSL_DEPRECATED void SSL_CTX_set_tmp_dh_callback(
    SSL_CTX *ctx, DH *(*cb)(SSL *ssl, int is_export, int keylength));

// SSL_set_tmp_dh_callback does nothing.
OPENSSL_EXPORT OPENSSL_DEPRECATED void SSL_set_tmp_dh_callback(
    SSL *ssl, DH *(*cb)(SSL *ssl, int is_export, int keylength));

// SSL_CTX_set_dh_auto does nothing and returns 0 for error.
OPENSSL_EXPORT OPENSSL_DEPRECATED long SSL_CTX_set_dh_auto(SSL_CTX *ctx,
                                                           int onoff);


// Security Levels No-ops [Deprecated].
//
// OpenSSL has the option to set “security levels”. Security levels can be
// defined either at compile time with "-DOPENSSL_TLS_SECURITY_LEVEL=level" or
// at runtime with |SSL_CTX_set_security_level|. AWS-LC does not support this
// and the security level APIs are no-ops within AWS-LC. AWS-LC intentionally
// limits the knobs a consumer can tweak in regards to security.

// SSL_CTX_get_security_level returns 0. This is only to maintain compatibility
// with OpenSSL and no security assumptions should be based on the number this
// function returns.
//
// Per OpenSSL's definition of Level 0, 1, and 2:
//
// Level 0:
// Everything is permitted. This retains compatibility with previous versions of
// OpenSSL.
//
// Level 1
// The security level corresponds to a minimum of 80 bits of security. Any
// parameters offering below 80 bits of security are excluded. As a result RSA,
// DSA and DH keys shorter than 1024 bits and ECC keys shorter than 160 bits are
// prohibited. All export cipher suites are prohibited since they all offer less
// than 80 bits of security. SSL version 2 is prohibited. Any cipher suite using
// MD5 for the MAC is also prohibited.
//
// Level 2
// Security level set to 112 bits of security. As a result RSA, DSA and DH keys
// shorter than 2048 bits and ECC keys shorter than 224 bits are prohibited. In
// addition to the level 1 exclusions any cipher suite using RC4 is also
// prohibited. SSL version 3 is also not allowed. Compression is disabled.
//
// AWS-LC's libssl doesn't support SSLv2 or SSLv3 and we have no support for MD5
// or RC4 related cipher suites. However, we don't directly prohibit 512 bit RSA
// keys like Level 1 in OpenSSL states. Since this function is only retained for
// OpenSSL compatibility, we set the returned value to 0.
OPENSSL_EXPORT OPENSSL_DEPRECATED int SSL_CTX_get_security_level(
    const SSL_CTX *ctx);

// SSL_CTX_set_security_level does nothing. See documentation in
// |SSL_CTX_get_security_level| about implied security levels for AWS-LC.
OPENSSL_EXPORT OPENSSL_DEPRECATED void SSL_CTX_set_security_level(
    const SSL_CTX *ctx, int level);


// General No-op Functions [Deprecated].

// SSL_set_state does nothing.
OPENSSL_EXPORT OPENSSL_DEPRECATED void SSL_set_state(SSL *ssl, int state);

// SSL_get_shared_ciphers writes an empty string to |buf| and returns a
// pointer to |buf|, or NULL if |len| is less than or equal to zero.
//
// TODO (CryptoAlg-2398): Add |OPENSSL_DEPRECATED|. nginx defines -Werror and
// depends on this.
OPENSSL_EXPORT char *SSL_get_shared_ciphers(const SSL *ssl, char *buf, int len);

// SSL_get_shared_sigalgs returns zero.
OPENSSL_EXPORT OPENSSL_DEPRECATED int SSL_get_shared_sigalgs(
    SSL *ssl, int idx, int *psign, int *phash, int *psignandhash, uint8_t *rsig,
    uint8_t *rhash);

// SSL_CTX_set_ecdh_auto returns one. This is also a no-op in OpenSSL.
#define SSL_CTX_set_ecdh_auto(ctx, onoff) 1

// SSL_set_ecdh_auto returns one. This is also a no-op in OpenSSL.
#define SSL_set_ecdh_auto(ssl, onoff) 1

// ERR_load_SSL_strings does nothing in AWS-LC and OpenSSL.
OPENSSL_EXPORT OPENSSL_DEPRECATED void ERR_load_SSL_strings(void);

// SSL_load_error_strings does nothing in AWS-LC and OpenSSL.
//
// TODO (CryptoAlg-2398): Add |OPENSSL_DEPRECATED|. nginx defines -Werror and
// depends on this.
OPENSSL_EXPORT void SSL_load_error_strings(void);


// SSL TMP_RSA No-ops [Deprecated].
//
// Support for these methods was intentionally removed due to them being the
// center of the FREAK attack. These functions are also no-ops in OpenSSL.
// FREAK Attack: https://freakattack.com/

// SSL_CTX_set_tmp_rsa_callback does nothing.
OPENSSL_EXPORT OPENSSL_DEPRECATED void SSL_CTX_set_tmp_rsa_callback(
    SSL_CTX *ctx, RSA *(*cb)(SSL *ssl, int is_export, int keylength));

// SSL_set_tmp_rsa_callback does nothing.
OPENSSL_EXPORT OPENSSL_DEPRECATED void SSL_set_tmp_rsa_callback(
    SSL *ssl, RSA *(*cb)(SSL *ssl, int is_export, int keylength));

// SSL_CTX_need_tmp_RSA returns zero.
OPENSSL_EXPORT OPENSSL_DEPRECATED int SSL_CTX_need_tmp_RSA(const SSL_CTX *ctx);

// SSL_need_tmp_RSA returns zero.
OPENSSL_EXPORT OPENSSL_DEPRECATED int SSL_need_tmp_RSA(const SSL *ssl);

// SSL_CTX_set_tmp_rsa returns one.
OPENSSL_EXPORT OPENSSL_DEPRECATED int SSL_CTX_set_tmp_rsa(SSL_CTX *ctx,
                                                          const RSA *rsa);

// SSL_set_tmp_rsa returns one.
OPENSSL_EXPORT OPENSSL_DEPRECATED int SSL_set_tmp_rsa(SSL *ssl, const RSA *rsa);


// Nodejs compatibility section (hidden).
//
// These defines exist for node.js, with the hope that we can eliminate the
// need for them over time.

#define SSLerr(function, reason) \
  ERR_put_error(ERR_LIB_SSL, 0, reason, __FILE__, __LINE__)


// Preprocessor compatibility section (hidden).
//
// Historically, a number of APIs were implemented in OpenSSL as macros and
// constants to 'ctrl' functions. To avoid breaking #ifdefs in consumers, this
// section defines a number of legacy macros.
//
// Although using either the CTRL values or their wrapper macros in #ifdefs is
// still supported, the CTRL values may not be passed to |SSL_ctrl| and
// |SSL_CTX_ctrl|. Call the functions (previously wrapper macros) instead.
//
// See PORTING.md in the BoringSSL source tree for a table of corresponding
// functions.
// https://github.com/aws/aws-lc/blob/main/PORTING.md#replacements-for-ctrl-values

#define DTLS_CTRL_GET_TIMEOUT doesnt_exist
#define DTLS_CTRL_HANDLE_TIMEOUT doesnt_exist
#define SSL_CTRL_CHAIN doesnt_exist
#define SSL_CTRL_CHAIN_CERT doesnt_exist
#define SSL_CTRL_CHANNEL_ID doesnt_exist
#define SSL_CTRL_CLEAR_EXTRA_CHAIN_CERTS doesnt_exist
#define SSL_CTRL_CLEAR_MODE doesnt_exist
#define SSL_CTRL_CLEAR_NUM_RENEGOTIATIONS doesnt_exist
#define SSL_CTRL_CLEAR_OPTIONS doesnt_exist
#define SSL_CTRL_EXTRA_CHAIN_CERT doesnt_exist
#define SSL_CTRL_GET_CHAIN_CERTS doesnt_exist
#define SSL_CTRL_GET_CHANNEL_ID doesnt_exist
#define SSL_CTRL_GET_CLIENT_CERT_TYPES doesnt_exist
#define SSL_CTRL_GET_EXTRA_CHAIN_CERTS doesnt_exist
#define SSL_CTRL_GET_MAX_CERT_LIST doesnt_exist
#define SSL_CTRL_GET_NEGOTIATED_GROUP doesnt_exist
#define SSL_CTRL_GET_NUM_RENEGOTIATIONS doesnt_exist
#define SSL_CTRL_GET_READ_AHEAD doesnt_exist
#define SSL_CTRL_GET_RI_SUPPORT doesnt_exist
#define SSL_CTRL_GET_SERVER_TMP_KEY doesnt_exist
#define SSL_CTRL_GET_SESSION_REUSED doesnt_exist
#define SSL_CTRL_GET_SESS_CACHE_MODE doesnt_exist
#define SSL_CTRL_GET_SESS_CACHE_SIZE doesnt_exist
#define SSL_CTRL_GET_TLSEXT_STATUS_REQ_TYPE doesnt_exist
#define SSL_CTRL_GET_TLSEXT_TICKET_KEYS doesnt_exist
#define SSL_CTRL_GET_TOTAL_RENEGOTIATIONS doesnt_exist
#define SSL_CTRL_MODE doesnt_exist
#define SSL_CTRL_NEED_TMP_RSA doesnt_exist
#define SSL_CTRL_OPTIONS doesnt_exist
#define SSL_CTRL_SESS_NUMBER doesnt_exist
#define SSL_CTRL_SET_CURVES doesnt_exist
#define SSL_CTRL_SET_CURVES_LIST doesnt_exist
#define SSL_CTRL_SET_GROUPS doesnt_exist
#define SSL_CTRL_SET_GROUPS_LIST doesnt_exist
#define SSL_CTRL_SET_ECDH_AUTO doesnt_exist
#define SSL_CTRL_SET_MAX_CERT_LIST doesnt_exist
#define SSL_CTRL_SET_MAX_SEND_FRAGMENT doesnt_exist
#define SSL_CTRL_SET_MSG_CALLBACK doesnt_exist
#define SSL_CTRL_SET_MSG_CALLBACK_ARG doesnt_exist
#define SSL_CTRL_SET_MTU doesnt_exist
#define SSL_CTRL_SET_READ_AHEAD doesnt_exist
#define SSL_CTRL_SET_SESS_CACHE_MODE doesnt_exist
#define SSL_CTRL_SET_SESS_CACHE_SIZE doesnt_exist
#define SSL_CTRL_SET_TLSEXT_HOSTNAME doesnt_exist
#define SSL_CTRL_SET_TLSEXT_SERVERNAME_ARG doesnt_exist
#define SSL_CTRL_SET_TLSEXT_SERVERNAME_CB doesnt_exist
#define SSL_CTRL_SET_TLSEXT_STATUS_REQ_CB doesnt_exist
#define SSL_CTRL_SET_TLSEXT_STATUS_REQ_TYPE doesnt_exist
#define SSL_CTRL_SET_TLSEXT_TICKET_KEYS doesnt_exist
#define SSL_CTRL_SET_TLSEXT_TICKET_KEY_CB doesnt_exist
#define SSL_CTRL_SET_TMP_DH doesnt_exist
#define SSL_CTRL_SET_TMP_DH_CB doesnt_exist
#define SSL_CTRL_SET_TMP_ECDH doesnt_exist
#define SSL_CTRL_SET_TMP_ECDH_CB doesnt_exist
#define SSL_CTRL_SET_TMP_RSA doesnt_exist
#define SSL_CTRL_SET_TMP_RSA_CB doesnt_exist

// |BORINGSSL_PREFIX| already makes each of these symbols into macros, so there
// is no need to define conflicting macros.
#if !defined(BORINGSSL_PREFIX)

#define DTLSv1_get_timeout DTLSv1_get_timeout
#define DTLSv1_handle_timeout DTLSv1_handle_timeout
#define SSL_CTX_add0_chain_cert SSL_CTX_add0_chain_cert
#define SSL_CTX_add1_chain_cert SSL_CTX_add1_chain_cert
#define SSL_CTX_add_extra_chain_cert SSL_CTX_add_extra_chain_cert
#define SSL_CTX_build_cert_chain SSL_CTX_build_cert_chain
#define SSL_CTX_clear_extra_chain_certs SSL_CTX_clear_extra_chain_certs
#define SSL_CTX_clear_chain_certs SSL_CTX_clear_chain_certs
#define SSL_CTX_clear_mode SSL_CTX_clear_mode
#define SSL_CTX_clear_options SSL_CTX_clear_options
#define SSL_CTX_get0_chain_certs SSL_CTX_get0_chain_certs
#define SSL_CTX_get_extra_chain_certs SSL_CTX_get_extra_chain_certs
#define SSL_CTX_get_max_cert_list SSL_CTX_get_max_cert_list
#define SSL_CTX_get_mode SSL_CTX_get_mode
#define SSL_CTX_get_options SSL_CTX_get_options
#define SSL_CTX_get_read_ahead SSL_CTX_get_read_ahead
#define SSL_CTX_get_session_cache_mode SSL_CTX_get_session_cache_mode
#define SSL_CTX_get_tlsext_status_cb SSL_CTX_get_tlsext_status_cb
#define SSL_CTX_get_tlsext_ticket_keys SSL_CTX_get_tlsext_ticket_keys
#define SSL_CTX_need_tmp_RSA SSL_CTX_need_tmp_RSA
#define SSL_CTX_sess_get_cache_size SSL_CTX_sess_get_cache_size
#define SSL_CTX_sess_number SSL_CTX_sess_number
#define SSL_CTX_sess_set_cache_size SSL_CTX_sess_set_cache_size
#define SSL_CTX_set0_chain SSL_CTX_set0_chain
#define SSL_CTX_set1_chain SSL_CTX_set1_chain
#define SSL_CTX_set1_curves SSL_CTX_set1_curves
#define SSL_CTX_set1_groups SSL_CTX_set1_groups
#define SSL_CTX_set_max_cert_list SSL_CTX_set_max_cert_list
#define SSL_CTX_set_max_send_fragment SSL_CTX_set_max_send_fragment
#define SSL_CTX_set_min_proto_version SSL_CTX_set_min_proto_version
#define SSL_CTX_set_mode SSL_CTX_set_mode
#define SSL_CTX_set_msg_callback_arg SSL_CTX_set_msg_callback_arg
#define SSL_CTX_set_options SSL_CTX_set_options
#define SSL_CTX_set_read_ahead SSL_CTX_set_read_ahead
#define SSL_CTX_set_session_cache_mode SSL_CTX_set_session_cache_mode
#define SSL_CTX_set_tlsext_servername_arg SSL_CTX_set_tlsext_servername_arg
#define SSL_CTX_set_tlsext_servername_callback \
  SSL_CTX_set_tlsext_servername_callback
#define SSL_CTX_set_tlsext_ticket_key_cb SSL_CTX_set_tlsext_ticket_key_cb
#define SSL_CTX_set_tlsext_ticket_keys SSL_CTX_set_tlsext_ticket_keys
#define SSL_CTX_set_tmp_dh SSL_CTX_set_tmp_dh
#define SSL_CTX_set_tmp_ecdh SSL_CTX_set_tmp_ecdh
#define SSL_CTX_set_tmp_rsa SSL_CTX_set_tmp_rsa
#define SSL_add0_chain_cert SSL_add0_chain_cert
#define SSL_add1_chain_cert SSL_add1_chain_cert
#define SSL_build_cert_chain SSL_build_cert_chain
#define SSL_clear_chain_certs SSL_clear_chain_certs
#define SSL_clear_num_renegotiations SSL_clear_num_renegotiations
#define SSL_clear_mode SSL_clear_mode
#define SSL_clear_options SSL_clear_options
#define SSL_get0_certificate_types SSL_get0_certificate_types
#define SSL_get0_chain_certs SSL_get0_chain_certs
#define SSL_get_max_cert_list SSL_get_max_cert_list
#define SSL_get_mode SSL_get_mode
#define SSL_get_negotiated_group SSL_get_negotiated_group
#define SSL_get_options SSL_get_options
#define SSL_get_secure_renegotiation_support \
  SSL_get_secure_renegotiation_support
#define SSL_need_tmp_RSA SSL_need_tmp_RSA
#define SSL_num_renegotiations SSL_num_renegotiations
#define SSL_session_reused SSL_session_reused
#define SSL_set0_chain SSL_set0_chain
#define SSL_set1_chain SSL_set1_chain
#define SSL_set1_curves SSL_set1_curves
#define SSL_set1_groups SSL_set1_groups
#define SSL_set_max_cert_list SSL_set_max_cert_list
#define SSL_set_max_send_fragment SSL_set_max_send_fragment
#define SSL_set_mode SSL_set_mode
#define SSL_set_msg_callback_arg SSL_set_msg_callback_arg
#define SSL_set_mtu SSL_set_mtu
#define SSL_set_options SSL_set_options
#define SSL_set_tlsext_host_name SSL_set_tlsext_host_name
#define SSL_set_tmp_dh SSL_set_tmp_dh
#define SSL_set_tmp_ecdh SSL_set_tmp_ecdh
#define SSL_set_tmp_rsa SSL_set_tmp_rsa
#define SSL_total_renegotiations SSL_total_renegotiations
#define SSL_in_connect_init SSL_in_connect_init
#define SSL_in_accept_init SSL_in_accept_init

#endif  // !defined(BORINGSSL_PREFIX)


#if defined(__cplusplus)
}  // extern C

#if !defined(BORINGSSL_NO_CXX)

extern "C++" {

BSSL_NAMESPACE_BEGIN

BORINGSSL_MAKE_DELETER(SSL, SSL_free)
BORINGSSL_MAKE_DELETER(SSL_CTX, SSL_CTX_free)
BORINGSSL_MAKE_UP_REF(SSL_CTX, SSL_CTX_up_ref)
BORINGSSL_MAKE_DELETER(SSL_ECH_KEYS, SSL_ECH_KEYS_free)
BORINGSSL_MAKE_UP_REF(SSL_ECH_KEYS, SSL_ECH_KEYS_up_ref)
BORINGSSL_MAKE_DELETER(SSL_SESSION, SSL_SESSION_free)
BORINGSSL_MAKE_UP_REF(SSL_SESSION, SSL_SESSION_up_ref)


// *** DEPRECATED EXPERIMENT — DO NOT USE ***
//
// Split handshakes.
//
// WARNING: This mechanism is deprecated and should not be used. It is very
// fragile and difficult to use correctly. The relationship between
// configuration options across the two halves is ill-defined and not
// self-consistent. Additionally, version skew across the two halves risks
// unusual behavior and connection failure. New development should use the
// handshake hints API. Existing deployments should migrate to handshake hints
// to reduce the risk of service outages.
//
// Split handshakes allows the handshake part of a TLS connection to be
// performed in a different process (or on a different machine) than the data
// exchange. This only applies to servers.
//
// In the first part of a split handshake, an |SSL| (where the |SSL_CTX| has
// been configured with |SSL_CTX_set_handoff_mode|) is used normally. Once the
// ClientHello message has been received, the handshake will stop and
// |SSL_get_error| will indicate |SSL_ERROR_HANDOFF|. At this point (and only
// at this point), |SSL_serialize_handoff| can be called to write the “handoff”
// state of the connection.
//
// Elsewhere, a fresh |SSL| can be used with |SSL_apply_handoff| to continue
// the connection. The connection from the client is fed into this |SSL|, and
// the handshake resumed. When the handshake stops again and |SSL_get_error|
// indicates |SSL_ERROR_HANDBACK|, |SSL_serialize_handback| should be called to
// serialize the state of the handshake again.
//
// Back at the first location, a fresh |SSL| can be used with
// |SSL_apply_handback|. Then the client's connection can be processed mostly
// as normal.
//
// Lastly, when a connection is in the handoff state, whether or not
// |SSL_serialize_handoff| is called, |SSL_decline_handoff| will move it back
// into a normal state where the connection can proceed without impact.
//
// WARNING: Currently only works with TLS 1.0–1.2.
// WARNING: The serialisation formats are not yet stable: version skew may be
//     fatal.
// WARNING: The handback data contains sensitive key material and must be
//     protected.
// WARNING: Some calls on the final |SSL| will not work. Just as an example,
//     calls like |SSL_get0_session_id_context| and |SSL_get_privatekey| won't
//     work because the certificate used for handshaking isn't available.
// WARNING: |SSL_apply_handoff| may trigger “msg” callback calls.

OPENSSL_EXPORT void SSL_CTX_set_handoff_mode(SSL_CTX *ctx, bool on);
OPENSSL_EXPORT void SSL_set_handoff_mode(SSL *SSL, bool on);
OPENSSL_EXPORT bool SSL_serialize_handoff(const SSL *ssl, CBB *out,
                                          SSL_CLIENT_HELLO *out_hello);
OPENSSL_EXPORT bool SSL_decline_handoff(SSL *ssl);
OPENSSL_EXPORT bool SSL_apply_handoff(SSL *ssl, Span<const uint8_t> handoff);
OPENSSL_EXPORT bool SSL_serialize_handback(const SSL *ssl, CBB *out);
OPENSSL_EXPORT bool SSL_apply_handback(SSL *ssl, Span<const uint8_t> handback);

// SSL_get_traffic_secrets sets |*out_read_traffic_secret| and
// |*out_write_traffic_secret| to reference the TLS 1.3 traffic secrets for
// |ssl|. This function is only valid on TLS 1.3 connections that have
// completed the handshake. It returns true on success and false on error.
OPENSSL_EXPORT bool SSL_get_traffic_secrets(
    const SSL *ssl, Span<const uint8_t> *out_read_traffic_secret,
    Span<const uint8_t> *out_write_traffic_secret);

// SSL_CTX_set_aes_hw_override_for_testing sets |override_value| to
// override checking for aes hardware support for testing. If |override_value|
// is set to true, the library will behave as if aes hardware support is
// present. If it is set to false, the library will behave as if aes hardware
// support is not present.
OPENSSL_EXPORT void SSL_CTX_set_aes_hw_override_for_testing(
    SSL_CTX *ctx, bool override_value);

// SSL_set_aes_hw_override_for_testing acts the same as
// |SSL_CTX_set_aes_override_for_testing| but only configures a single |SSL*|.
OPENSSL_EXPORT void SSL_set_aes_hw_override_for_testing(SSL *ssl,
                                                        bool override_value);

BSSL_NAMESPACE_END

}  // extern C++

#endif  // !defined(BORINGSSL_NO_CXX)

#endif

// Define some reason codes from OpenSSL to ease compilation against AWS-LC.
// None of these reason codes are actually returned by AWS-LC.
// Code paths that condition on these codes might break. Grep for
// |ERR_GET_REASON| and |ERR_GET_FUNC| to inspect.

// Per C99 standard 6.8.4.2.1 the controlling expression in the switch statement
// has integer type. Hence it can handle quite large offset values assuming just
// two bytes. Just pick a somewhat large random value to prevent collisions on
// later macro definitions from upstream.
#define SSL_R_BACKWARDS_COMPATABILITY_OFFSET 0x5D21

// See CryptoAlg-954.
#define SSL_R_NO_PROTOCOLS_AVAILABLE (SSL_R_BACKWARDS_COMPATABILITY_OFFSET + 1)
#define SSL_R_BAD_PROTOCOL_VERSION_NUMBER \
  (SSL_R_BACKWARDS_COMPATABILITY_OFFSET + 2)
#define SSL_R_UNSUPPORTED_SSL_VERSION (SSL_R_BACKWARDS_COMPATABILITY_OFFSET + 3)
#define SSL_R_VERSION_TOO_HIGH (SSL_R_BACKWARDS_COMPATABILITY_OFFSET + 4)
#define SSL_R_VERSION_TOO_LOW (SSL_R_BACKWARDS_COMPATABILITY_OFFSET + 5)

#define SSL_R_APP_DATA_IN_HANDSHAKE 100
#define SSL_R_ATTEMPT_TO_REUSE_SESSION_IN_DIFFERENT_CONTEXT 101
#define SSL_R_BAD_ALERT 102
#define SSL_R_BAD_CHANGE_CIPHER_SPEC 103
#define SSL_R_BAD_DATA_RETURNED_BY_CALLBACK 104
#define SSL_R_BAD_DH_P_LENGTH 105
#define SSL_R_BAD_DIGEST_LENGTH 106
#define SSL_R_BAD_ECC_CERT 107
#define SSL_R_BAD_ECPOINT 108
#define SSL_R_BAD_HANDSHAKE_RECORD 109
#define SSL_R_BAD_HELLO_REQUEST 110
#define SSL_R_BAD_LENGTH 111
#define SSL_R_BAD_PACKET_LENGTH 112
#define SSL_R_BAD_RSA_ENCRYPT 113
#define SSL_R_BAD_SIGNATURE 114
#define SSL_R_BAD_SRTP_MKI_VALUE 115
#define SSL_R_BAD_SRTP_PROTECTION_PROFILE_LIST 116
#define SSL_R_BAD_SSL_FILETYPE 117
#define SSL_R_BAD_WRITE_RETRY 118
#define SSL_R_BIO_NOT_SET 119
#define SSL_R_BN_LIB 120
#define SSL_R_BUFFER_TOO_SMALL 121
#define SSL_R_CA_DN_LENGTH_MISMATCH 122
#define SSL_R_CA_DN_TOO_LONG 123
#define SSL_R_CCS_RECEIVED_EARLY 124
#define SSL_R_CERTIFICATE_VERIFY_FAILED 125
#define SSL_R_CERT_CB_ERROR 126
#define SSL_R_CERT_LENGTH_MISMATCH 127
#define SSL_R_CHANNEL_ID_NOT_P256 128
#define SSL_R_CHANNEL_ID_SIGNATURE_INVALID 129
#define SSL_R_CIPHER_OR_HASH_UNAVAILABLE 130
#define SSL_R_CLIENTHELLO_PARSE_FAILED 131
#define SSL_R_CLIENTHELLO_TLSEXT 132
#define SSL_R_CONNECTION_REJECTED 133
#define SSL_R_CONNECTION_TYPE_NOT_SET 134
#define SSL_R_CUSTOM_EXTENSION_ERROR 135
#define SSL_R_DATA_LENGTH_TOO_LONG 136
#define SSL_R_DECODE_ERROR 137
#define SSL_R_DECRYPTION_FAILED 138
#define SSL_R_DECRYPTION_FAILED_OR_BAD_RECORD_MAC 139
#define SSL_R_DH_PUBLIC_VALUE_LENGTH_IS_WRONG 140
#define SSL_R_DH_P_TOO_LONG 141
#define SSL_R_DIGEST_CHECK_FAILED 142
#define SSL_R_DTLS_MESSAGE_TOO_BIG 143
#define SSL_R_ECC_CERT_NOT_FOR_SIGNING 144
#define SSL_R_EMS_STATE_INCONSISTENT 145
#define SSL_R_ENCRYPTED_LENGTH_TOO_LONG 146
#define SSL_R_ERROR_ADDING_EXTENSION 147
#define SSL_R_ERROR_IN_RECEIVED_CIPHER_LIST 148
#define SSL_R_ERROR_PARSING_EXTENSION 149
#define SSL_R_EXCESSIVE_MESSAGE_SIZE 150
#define SSL_R_EXTRA_DATA_IN_MESSAGE 151
#define SSL_R_FRAGMENT_MISMATCH 152
#define SSL_R_GOT_NEXT_PROTO_WITHOUT_EXTENSION 153
#define SSL_R_HANDSHAKE_FAILURE_ON_CLIENT_HELLO 154
#define SSL_R_HTTPS_PROXY_REQUEST 155
#define SSL_R_HTTP_REQUEST 156
#define SSL_R_INAPPROPRIATE_FALLBACK 157
#define SSL_R_INVALID_COMMAND 158
#define SSL_R_INVALID_MESSAGE 159
#define SSL_R_INVALID_SSL_SESSION 160
#define SSL_R_INVALID_TICKET_KEYS_LENGTH 161
#define SSL_R_LENGTH_MISMATCH 162
#define SSL_R_MISSING_EXTENSION 164
#define SSL_R_MISSING_RSA_CERTIFICATE 165
#define SSL_R_MISSING_TMP_DH_KEY 166
#define SSL_R_MISSING_TMP_ECDH_KEY 167
#define SSL_R_MIXED_SPECIAL_OPERATOR_WITH_GROUPS 168
#define SSL_R_MTU_TOO_SMALL 169
#define SSL_R_NEGOTIATED_BOTH_NPN_AND_ALPN 170
#define SSL_R_NESTED_GROUP 171
#define SSL_R_NO_CERTIFICATES_RETURNED 172
#define SSL_R_NO_CERTIFICATE_ASSIGNED 173
#define SSL_R_NO_CERTIFICATE_SET 174
#define SSL_R_NO_CIPHERS_AVAILABLE 175
#define SSL_R_NO_CIPHERS_PASSED 176
#define SSL_R_NO_CIPHER_MATCH 177
#define SSL_R_NO_COMPRESSION_SPECIFIED 178
#define SSL_R_NO_METHOD_SPECIFIED 179
#define SSL_R_NO_PRIVATE_KEY_ASSIGNED 181
#define SSL_R_NO_RENEGOTIATION 182
#define SSL_R_NO_REQUIRED_DIGEST 183
#define SSL_R_NO_SHARED_CIPHER 184
#define SSL_R_NULL_SSL_CTX 185
#define SSL_R_NULL_SSL_METHOD_PASSED 186
#define SSL_R_OLD_SESSION_CIPHER_NOT_RETURNED 187
#define SSL_R_OLD_SESSION_VERSION_NOT_RETURNED 188
#define SSL_R_OUTPUT_ALIASES_INPUT 189
#define SSL_R_PARSE_TLSEXT 190
#define SSL_R_PATH_TOO_LONG 191
#define SSL_R_PEER_DID_NOT_RETURN_A_CERTIFICATE 192
#define SSL_R_PEER_ERROR_UNSUPPORTED_CERTIFICATE_TYPE 193
#define SSL_R_PROTOCOL_IS_SHUTDOWN 194
#define SSL_R_PSK_IDENTITY_NOT_FOUND 195
#define SSL_R_PSK_NO_CLIENT_CB 196
#define SSL_R_PSK_NO_SERVER_CB 197
#define SSL_R_READ_TIMEOUT_EXPIRED 198
#define SSL_R_RECORD_LENGTH_MISMATCH 199
#define SSL_R_RECORD_TOO_LARGE 200
#define SSL_R_RENEGOTIATION_ENCODING_ERR 201
#define SSL_R_RENEGOTIATION_MISMATCH 202
#define SSL_R_REQUIRED_CIPHER_MISSING 203
#define SSL_R_RESUMED_EMS_SESSION_WITHOUT_EMS_EXTENSION 204
#define SSL_R_RESUMED_NON_EMS_SESSION_WITH_EMS_EXTENSION 205
#define SSL_R_SCSV_RECEIVED_WHEN_RENEGOTIATING 206
#define SSL_R_SERVERHELLO_TLSEXT 207
#define SSL_R_SESSION_ID_CONTEXT_UNINITIALIZED 208
#define SSL_R_SESSION_MAY_NOT_BE_CREATED 209
#define SSL_R_SIGNATURE_ALGORITHMS_EXTENSION_SENT_BY_SERVER 210
#define SSL_R_SRTP_COULD_NOT_ALLOCATE_PROFILES 211
#define SSL_R_SRTP_UNKNOWN_PROTECTION_PROFILE 212
#define SSL_R_SSL3_EXT_INVALID_SERVERNAME 213
#define SSL_R_SSL_CTX_HAS_NO_DEFAULT_SSL_VERSION 214
#define SSL_R_SSL_HANDSHAKE_FAILURE 215
#define SSL_R_SSL_SESSION_ID_CONTEXT_TOO_LONG 216
#define SSL_R_TLS_PEER_DID_NOT_RESPOND_WITH_CERTIFICATE_LIST 217
#define SSL_R_TLS_RSA_ENCRYPTED_VALUE_LENGTH_IS_WRONG 218
#define SSL_R_TOO_MANY_EMPTY_FRAGMENTS 219
#define SSL_R_TOO_MANY_WARNING_ALERTS 220
#define SSL_R_UNABLE_TO_FIND_ECDH_PARAMETERS 221
#define SSL_R_UNEXPECTED_EXTENSION 222
#define SSL_R_UNEXPECTED_MESSAGE 223
#define SSL_R_UNEXPECTED_OPERATOR_IN_GROUP 224
#define SSL_R_UNEXPECTED_RECORD 225
#define SSL_R_UNINITIALIZED 226
#define SSL_R_UNKNOWN_ALERT_TYPE 227
#define SSL_R_UNKNOWN_CERTIFICATE_TYPE 228
#define SSL_R_UNKNOWN_CIPHER_RETURNED 229
#define SSL_R_UNKNOWN_CIPHER_TYPE 230
#define SSL_R_UNKNOWN_DIGEST 231
#define SSL_R_UNKNOWN_KEY_EXCHANGE_TYPE 232
#define SSL_R_UNKNOWN_PROTOCOL 233
#define SSL_R_UNKNOWN_SSL_VERSION 234
#define SSL_R_UNKNOWN_STATE 235
#define SSL_R_UNSAFE_LEGACY_RENEGOTIATION_DISABLED 236
#define SSL_R_UNSUPPORTED_CIPHER 237
#define SSL_R_UNSUPPORTED_COMPRESSION_ALGORITHM 238
#define SSL_R_UNSUPPORTED_ELLIPTIC_CURVE 239
#define SSL_R_UNSUPPORTED_PROTOCOL 240
#define SSL_R_WRONG_CERTIFICATE_TYPE 241
#define SSL_R_WRONG_CIPHER_RETURNED 242
#define SSL_R_WRONG_CURVE 243
#define SSL_R_WRONG_MESSAGE_TYPE 244
#define SSL_R_WRONG_SIGNATURE_TYPE 245
#define SSL_R_WRONG_SSL_VERSION 246
#define SSL_R_WRONG_VERSION_NUMBER 247
#define SSL_R_X509_LIB 248
#define SSL_R_X509_VERIFICATION_SETUP_PROBLEMS 249
#define SSL_R_SHUTDOWN_WHILE_IN_INIT 250
#define SSL_R_INVALID_OUTER_RECORD_TYPE 251
#define SSL_R_UNSUPPORTED_PROTOCOL_FOR_CUSTOM_KEY 252
#define SSL_R_NO_COMMON_SIGNATURE_ALGORITHMS 253
#define SSL_R_DOWNGRADE_DETECTED 254
#define SSL_R_EXCESS_HANDSHAKE_DATA 255
#define SSL_R_INVALID_COMPRESSION_LIST 256
#define SSL_R_DUPLICATE_EXTENSION 257
#define SSL_R_MISSING_KEY_SHARE 258
#define SSL_R_INVALID_ALPN_PROTOCOL 259
#define SSL_R_TOO_MANY_KEY_UPDATES 260
#define SSL_R_BLOCK_CIPHER_PAD_IS_WRONG 261
#define SSL_R_NO_CIPHERS_SPECIFIED 262
#define SSL_R_RENEGOTIATION_EMS_MISMATCH 263
#define SSL_R_DUPLICATE_KEY_SHARE 264
#define SSL_R_NO_GROUPS_SPECIFIED 265
#define SSL_R_NO_SHARED_GROUP 266
#define SSL_R_PRE_SHARED_KEY_MUST_BE_LAST 267
#define SSL_R_OLD_SESSION_PRF_HASH_MISMATCH 268
#define SSL_R_INVALID_SCT_LIST 269
#define SSL_R_TOO_MUCH_SKIPPED_EARLY_DATA 270
#define SSL_R_PSK_IDENTITY_BINDER_COUNT_MISMATCH 271
#define SSL_R_CANNOT_PARSE_LEAF_CERT 272
#define SSL_R_SERVER_CERT_CHANGED 273
#define SSL_R_CERTIFICATE_AND_PRIVATE_KEY_MISMATCH 274
#define SSL_R_CANNOT_HAVE_BOTH_PRIVKEY_AND_METHOD 275
#define SSL_R_TICKET_ENCRYPTION_FAILED 276
#define SSL_R_ALPN_MISMATCH_ON_EARLY_DATA 277
#define SSL_R_WRONG_VERSION_ON_EARLY_DATA 278
#define SSL_R_UNEXPECTED_EXTENSION_ON_EARLY_DATA 279
#define SSL_R_NO_SUPPORTED_VERSIONS_ENABLED 280
#define SSL_R_APPLICATION_DATA_INSTEAD_OF_HANDSHAKE 281
#define SSL_R_EMPTY_HELLO_RETRY_REQUEST 282
#define SSL_R_EARLY_DATA_NOT_IN_USE 283
#define SSL_R_HANDSHAKE_NOT_COMPLETE 284
#define SSL_R_NEGOTIATED_TB_WITHOUT_EMS_OR_RI 285
#define SSL_R_SERVER_ECHOED_INVALID_SESSION_ID 286
#define SSL_R_PRIVATE_KEY_OPERATION_FAILED 287
#define SSL_R_SECOND_SERVERHELLO_VERSION_MISMATCH 288
#define SSL_R_OCSP_CB_ERROR 289
#define SSL_R_SSL_SESSION_ID_TOO_LONG 290
#define SSL_R_APPLICATION_DATA_ON_SHUTDOWN 291
#define SSL_R_CERT_DECOMPRESSION_FAILED 292
#define SSL_R_UNCOMPRESSED_CERT_TOO_LARGE 293
#define SSL_R_UNKNOWN_CERT_COMPRESSION_ALG 294
#define SSL_R_INVALID_SIGNATURE_ALGORITHM 295
#define SSL_R_DUPLICATE_SIGNATURE_ALGORITHM 296
#define SSL_R_TLS13_DOWNGRADE 297
#define SSL_R_QUIC_INTERNAL_ERROR 298
#define SSL_R_WRONG_ENCRYPTION_LEVEL_RECEIVED 299
#define SSL_R_TOO_MUCH_READ_EARLY_DATA 300
#define SSL_R_INVALID_DELEGATED_CREDENTIAL 301
#define SSL_R_KEY_USAGE_BIT_INCORRECT 302
#define SSL_R_INCONSISTENT_CLIENT_HELLO 303
#define SSL_R_CIPHER_MISMATCH_ON_EARLY_DATA 304
#define SSL_R_QUIC_TRANSPORT_PARAMETERS_MISCONFIGURED 305
#define SSL_R_UNEXPECTED_COMPATIBILITY_MODE 306
#define SSL_R_NO_APPLICATION_PROTOCOL 307
#define SSL_R_NEGOTIATED_ALPS_WITHOUT_ALPN 308
#define SSL_R_ALPS_MISMATCH_ON_EARLY_DATA 309
#define SSL_R_ECH_SERVER_CONFIG_AND_PRIVATE_KEY_MISMATCH 310
#define SSL_R_ECH_SERVER_CONFIG_UNSUPPORTED_EXTENSION 311
#define SSL_R_UNSUPPORTED_ECH_SERVER_CONFIG 312
#define SSL_R_ECH_SERVER_WOULD_HAVE_NO_RETRY_CONFIGS 313
#define SSL_R_INVALID_CLIENT_HELLO_INNER 314
#define SSL_R_INVALID_ALPN_PROTOCOL_LIST 315
#define SSL_R_COULD_NOT_PARSE_HINTS 316
#define SSL_R_INVALID_ECH_PUBLIC_NAME 317
#define SSL_R_INVALID_ECH_CONFIG_LIST 318
#define SSL_R_ECH_REJECTED 319
#define SSL_R_INVALID_OUTER_EXTENSION 320
#define SSL_R_INCONSISTENT_ECH_NEGOTIATION 321
#define SSL_R_INVALID_ALPS_CODEPOINT 322
#define SSL_R_NOT_REPLACING_CERTIFICATE 323
#define SSL_R_SERIALIZATION_UNSUPPORTED 500
#define SSL_R_SERIALIZATION_INVALID_SSL 501
#define SSL_R_SERIALIZATION_INVALID_SSL_CONFIG 502
#define SSL_R_SERIALIZATION_INVALID_SSL3_STATE 503
#define SSL_R_SERIALIZATION_INVALID_SSL_BUFFER 505
#define SSL_R_SERIALIZATION_INVALID_SSL_AEAD_CONTEXT 506
#define SSL_R_BAD_HYBRID_KEYSHARE 507
#define SSL_R_BAD_KEM_CIPHERTEXT 508
#define SSL_R_SSLV3_ALERT_CLOSE_NOTIFY 1000
#define SSL_R_SSLV3_ALERT_UNEXPECTED_MESSAGE 1010
#define SSL_R_SSLV3_ALERT_BAD_RECORD_MAC 1020
#define SSL_R_TLSV1_ALERT_DECRYPTION_FAILED 1021
#define SSL_R_TLSV1_ALERT_RECORD_OVERFLOW 1022
#define SSL_R_SSLV3_ALERT_DECOMPRESSION_FAILURE 1030
#define SSL_R_SSLV3_ALERT_HANDSHAKE_FAILURE 1040
#define SSL_R_SSLV3_ALERT_NO_CERTIFICATE 1041
#define SSL_R_SSLV3_ALERT_BAD_CERTIFICATE 1042
#define SSL_R_SSLV3_ALERT_UNSUPPORTED_CERTIFICATE 1043
#define SSL_R_SSLV3_ALERT_CERTIFICATE_REVOKED 1044
#define SSL_R_SSLV3_ALERT_CERTIFICATE_EXPIRED 1045
#define SSL_R_SSLV3_ALERT_CERTIFICATE_UNKNOWN 1046
#define SSL_R_SSLV3_ALERT_ILLEGAL_PARAMETER 1047
#define SSL_R_TLSV1_ALERT_UNKNOWN_CA 1048
#define SSL_R_TLSV1_ALERT_ACCESS_DENIED 1049
#define SSL_R_TLSV1_ALERT_DECODE_ERROR 1050
#define SSL_R_TLSV1_ALERT_DECRYPT_ERROR 1051
#define SSL_R_TLSV1_ALERT_EXPORT_RESTRICTION 1060
#define SSL_R_TLSV1_ALERT_PROTOCOL_VERSION 1070
#define SSL_R_TLSV1_ALERT_INSUFFICIENT_SECURITY 1071
#define SSL_R_TLSV1_ALERT_INTERNAL_ERROR 1080
#define SSL_R_TLSV1_ALERT_INAPPROPRIATE_FALLBACK 1086
#define SSL_R_TLSV1_ALERT_USER_CANCELLED 1090
#define SSL_R_TLSV1_ALERT_NO_RENEGOTIATION 1100
#define SSL_R_TLSV1_ALERT_UNSUPPORTED_EXTENSION 1110
#define SSL_R_TLSV1_ALERT_CERTIFICATE_UNOBTAINABLE 1111
#define SSL_R_TLSV1_ALERT_UNRECOGNIZED_NAME 1112
#define SSL_R_TLSV1_ALERT_BAD_CERTIFICATE_STATUS_RESPONSE 1113
#define SSL_R_TLSV1_ALERT_BAD_CERTIFICATE_HASH_VALUE 1114
#define SSL_R_TLSV1_ALERT_UNKNOWN_PSK_IDENTITY 1115
#define SSL_R_TLSV1_ALERT_CERTIFICATE_REQUIRED 1116
#define SSL_R_TLSV1_ALERT_NO_APPLICATION_PROTOCOL 1120
#define SSL_R_TLSV1_ALERT_ECH_REQUIRED 1121
#define SSL_R_SERIALIZATION_INVALID_SERDE_VERSION 1122

#endif  // OPENSSL_HEADER_SSL_H
