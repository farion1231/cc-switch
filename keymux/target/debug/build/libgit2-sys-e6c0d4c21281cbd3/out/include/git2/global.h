/*
 * Copyright (C) the libgit2 contributors. All rights reserved.
 *
 * This file is part of libgit2, distributed under the GNU GPL v2 with
 * a Linking Exception. For full terms see the included COPYING file.
 */
#ifndef INCLUDE_git_global_h__
#define INCLUDE_git_global_h__

#include "common.h"

GIT_BEGIN_DECL

/**
 * Init the global state
 *
 * This function must be called before any other libgit2 function in
 * order to set up global state and threading.
 *
 * This function may be called multiple times - it will return the number
 * of times the initialization has been called (including this one) that have
 * not subsequently been shutdown.
 *
 * @return the number of initializations of the library, or an error code.
 */
GIT_EXTERN(int) git_libgit2_init(void);

/**
 * Shutdown the global state
 *
 * Clean up the global state and threading context after calling it as
 * many times as `git_libgit2_init()` was called - it will return the
 * number of remainining initializations that have not been shutdown
 * (after this one).
 * 
 * @return the number of remaining initializations of the library, or an
 * error code.
 */
GIT_EXTERN(int) git_libgit2_shutdown(void);

/** @} */
GIT_END_DECL
#endif

