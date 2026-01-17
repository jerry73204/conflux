/*
 * Conflux C++ Library - Visibility Macros
 *
 * License: MIT OR Apache-2.0
 */

#ifndef CONFLUX_VISIBILITY_H
#define CONFLUX_VISIBILITY_H

// Define CONFLUX_EXPORT for shared library symbol visibility
#ifdef _WIN32
#ifdef CONFLUX_BUILDING_DLL
#define CONFLUX_EXPORT __declspec(dllexport)
#else
#define CONFLUX_EXPORT __declspec(dllimport)
#endif
#else
#define CONFLUX_EXPORT __attribute__((visibility("default")))
#endif

#endif  // CONFLUX_VISIBILITY_H
