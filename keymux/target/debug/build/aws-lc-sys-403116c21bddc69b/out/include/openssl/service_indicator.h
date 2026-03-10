// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0 OR ISC

#ifndef AWSLC_HEADER_SERVICE_INDICATOR_H
#define AWSLC_HEADER_SERVICE_INDICATOR_H

#include <openssl/base.h>

#if defined(__cplusplus)
extern "C" {
#endif

// FIPS 140-3 Approved Security Service Indicator


// |FIPS_service_indicator_before_call| and |FIPS_service_indicator_after_call|
// both currently return the same local thread counter which is slowly
// incremented whenever approved services are called.
//
// |FIPS_service_indicator_before_call| is intended to be called right before
// an approved service, while |FIPS_service_indicator_after_call| should be
// called immediately after. If the values returned from these two functions are
// not equal, this means that the service called in between is deemed to be
// approved. If the values are still the same, this means the counter has not
// been incremented, and the service called is otherwise not approved for FIPS.


OPENSSL_EXPORT uint64_t FIPS_service_indicator_before_call(void);
OPENSSL_EXPORT uint64_t FIPS_service_indicator_after_call(void);

OPENSSL_EXPORT const char* awslc_version_string(void);

enum FIPSStatus {
  AWSLC_NOT_APPROVED = 0,
  AWSLC_APPROVED = 1
};

#if defined(AWSLC_FIPS)

#define AWSLC_MODE_STRING "AWS-LC FIPS "

// CALL_SERVICE_AND_CHECK_APPROVED performs an approval check and runs the service.
// The |approved| value passed in will change to |AWSLC_APPROVED| and
// |AWSLC_NOT_APPROVED| accordingly to the approved state of the service ran.
// It is highly recommended that users of the service indicator use this macro
// when interacting with the service indicator.
//
// This macro tests before != after to handle potential uint64_t rollover in
// long-running applications that use the release build of AWS-LC. Debug builds
// use an assert before + 1 == after to ensure in testing the service indicator
// is operating as expected.
#define CALL_SERVICE_AND_CHECK_APPROVED(approved, func)             \
  do {                                                              \
    (approved) = AWSLC_NOT_APPROVED;                                \
    int before = FIPS_service_indicator_before_call();              \
    func;                                                           \
    int after = FIPS_service_indicator_after_call();                \
    if (before != after) {                                          \
        assert(before + 1 == after);                                \
        (approved) = AWSLC_APPROVED;                                \
    }                                                               \
 }                                                                  \
 while(0)

#else

#define AWSLC_MODE_STRING "AWS-LC "

// CALL_SERVICE_AND_CHECK_APPROVED always returns |AWSLC_APPROVED| when AWS-LC
// is not built in FIPS mode for easier consumer compatibility that have both
// FIPS and non-FIPS libraries.
#define CALL_SERVICE_AND_CHECK_APPROVED(approved, func)             \
  do {                                                              \
    (approved) = AWSLC_APPROVED;                                    \
    func;                                                           \
 }                                                                  \
 while(0)                                                           \

#endif // AWSLC_FIPS

#define AWSLC_VERSION_STRING AWSLC_MODE_STRING AWSLC_VERSION_NUMBER_STRING


#if defined(__cplusplus)
}  // extern C
#endif

#endif  // AWSLC_SERVICE_INDICATOR_H
