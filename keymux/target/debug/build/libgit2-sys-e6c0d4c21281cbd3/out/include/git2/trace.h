/*
 * Copyright (C) the libgit2 contributors. All rights reserved.
 *
 * This file is part of libgit2, distributed under the GNU GPL v2 with
 * a Linking Exception. For full terms see the included COPYING file.
 */
#ifndef INCLUDE_git_trace_h__
#define INCLUDE_git_trace_h__

#include "common.h"
#include "types.h"

/**
 * @file git2/trace.h
 * @brief Git tracing configuration routines
 * @defgroup git_trace Git tracing configuration routines
 * @ingroup Git
 * @{
 */
GIT_BEGIN_DECL

/**
 * Available tracing levels.  When tracing is set to a particular level,
 * callers will be provided tracing at the given level and all lower levels.
 */
typedef enum {
	/** No tracing will be performed. */
	GIT_TRACE_NONE = 0,

	/** Severe errors that may impact the program's execution */
	GIT_TRACE_FATAL = 1,

	/** Errors that do not impact the program's execution */
	GIT_TRACE_ERROR = 2,

	/** Warnings that suggest abnormal data */
	GIT_TRACE_WARN = 3,

	/** Informational messages about program execution */
	GIT_TRACE_INFO = 4,

	/** Detailed data that allows for debugging */
	GIT_TRACE_DEBUG = 5,

	/** Exceptionally detailed debugging data */
	GIT_TRACE_TRACE = 6
} git_trace_level_t;

/**
 * An instance for a tracing function
 */
typedef void GIT_CALLBACK(git_trace_cb)(git_trace_level_t level, const char *msg);

/**
 * Sets the system tracing configuration to the specified level with the
 * specified callback.  When system events occur at a level equal to, or
 * lower than, the given level they will be reported to the given callback.
 *
 * @param level Level to set tracing to
 * @param cb Function to call with trace data
 * @return 0 or an error code
 */
GIT_EXTERN(int) git_trace_set(git_trace_level_t level, git_trace_cb cb);

/** @} */
GIT_END_DECL
#endif
