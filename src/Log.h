#pragma once

#include <cstdio>

#define LOG_LEVEL_DEBUG 0
#define LOG_LEVEL_INFO 1
#define LOG_LEVEL_WARNING 2
#define LOG_LEVEL_ERROR 3
#define LOG_LEVEL_FATAL 4

#ifndef LOG_LEVEL
#define LOG_LEVEL LOG_LEVEL_DEBUG
#endif

#if LOG_LEVEL <= LOG_LEVEL_DEBUG
#define LOG_DEBUG(TAG, FMT, ARGS...) \
    printf("DEBUG: \x1b[30;1m(" TAG ") \x1b[0m" FMT "\n", ##ARGS);
#else
#define LOG_DEBUG(...)
#endif

#if LOG_LEVEL <= LOG_LEVEL_INFO
#define LOG_INFO(TAG, FMT, ARGS...) \
    printf("\x1b[32;1mINFO:  \x1b[30;1m(" TAG ") \x1b[0m" FMT "\n", ##ARGS);
#else
#define LOG_INFO(...)
#endif

#if LOG_WARNING <= LOG_LEVEL_WARNING
#define LOG_WARNING(TAG, FMT, ARGS...) \
    printf("\x1b[33;1mWARN:  \x1b[30;1m(" TAG ") \x1b[0m" FMT "\n", ##ARGS);
#else
#define LOG_DEBUG(...)
#endif

#if LOG_ERROR <= LOG_LEVEL_ERROR
#define LOG_ERROR(TAG, FMT, ARGS...) \
    printf("\x1b[31;1mERROR: \x1b[30;1m(" TAG ") \x1b[0m" FMT "\n", ##ARGS);
#else
#define LOG_DEBUG(...)
#endif

#if LOG_FATAL <= LOG_LEVEL_FATAL
#define LOG_FATAL(TAG, FMT, ARGS...) \
    printf("\x1b[41;1mFATAL: \x1b[0;30;1m(" TAG ") \x1b[0m" FMT "\n", ##ARGS);
#else
#define LOG_DEBUG(...)
#endif
