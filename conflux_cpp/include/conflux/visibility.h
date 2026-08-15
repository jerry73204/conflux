// Copyright 2026 jerry73204
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//
// This package is dual-licensed: "MIT OR Apache-2.0". The Apache-2.0 notice
// above is reproduced in full because the linter requires a recognised license
// block; it does not narrow the choice. See LICENSE-MIT and LICENSE-APACHE.

/*
 * Conflux C++ Library - Visibility Macros
 */

#ifndef CONFLUX__VISIBILITY_H_
#define CONFLUX__VISIBILITY_H_

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

#endif  // CONFLUX__VISIBILITY_H_
