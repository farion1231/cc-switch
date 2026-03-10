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

/* This header is provided in order to make compiling against code that expects
   OpenSSL easier. */

#include <openssl/crypto.h>

// MySQL does regex parsing on the opensslv.h file directly.
// https://github.com/mysql/mysql-server/blob/8.0/cmake/ssl.cmake#L208-L227
// |OPENSSL_VERSION_NUMBER| is defined here again to comply to this. MySQL
// only parses this to define version numbers in their CMake script.
// It does not require it to be active.
#if 0
#define OPENSSL_VERSION_NUMBER 0x1010107f
#endif
