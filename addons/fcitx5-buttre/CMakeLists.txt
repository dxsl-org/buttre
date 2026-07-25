# fcitx5-buttre — in-process fcitx5 addon bridging to buttre's Rust core
# through the buttre-ffi C ABI (include/buttre_ffi.h).
#
# Deliberately NOT part of the cargo workspace build: this is the only C++
# in the repo (fcitx5 engines must be in-process addons) and it needs
# libfcitx5core-dev + ECM, which plain `cargo build` users don't have.
#
# Dev build (from the repo root):
#   cargo build -p buttre-ffi
#   cmake -S addons/fcitx5-buttre -B ~/.cache/buttre-fcitx5-build \
#         -DBUTTRE_FFI_LIB=$CARGO_TARGET_DIR/debug/libbuttre_ffi.so
#   cmake --build ~/.cache/buttre-fcitx5-build
#   sudo cmake --install ~/.cache/buttre-fcitx5-build
cmake_minimum_required(VERSION 3.16)
project(fcitx5-buttre VERSION 0.7.10 LANGUAGES CXX)

# fcitx5 >= 5.1.x headers use C++20 (string_view::starts_with & friends).
set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

find_package(ECM REQUIRED)
list(APPEND CMAKE_MODULE_PATH ${ECM_MODULE_PATH})
find_package(Fcitx5Core REQUIRED)
include(GNUInstallDirs)

# The Rust side of the bridge. No default: passing the wrong profile's .so
# silently ships stale engine behaviour, so the builder must be explicit.
set(BUTTRE_FFI_LIB "" CACHE FILEPATH "Path to libbuttre_ffi.so (cargo build -p buttre-ffi)")
set(BUTTRE_FFI_INCLUDE "${CMAKE_CURRENT_SOURCE_DIR}/../../include" CACHE PATH "Directory containing buttre_ffi.h")
if(NOT BUTTRE_FFI_LIB)
    message(FATAL_ERROR "Set -DBUTTRE_FFI_LIB=/path/to/libbuttre_ffi.so (build it with: cargo build -p buttre-ffi)")
endif()

add_library(buttre-fcitx5 MODULE src/buttre_engine.cpp)
target_include_directories(buttre-fcitx5 PRIVATE "${BUTTRE_FFI_INCLUDE}")
target_link_libraries(buttre-fcitx5 Fcitx5::Core "${BUTTRE_FFI_LIB}")

# Addon .so name must match Library=libbuttre-fcitx5 in buttre-addon.conf.
# RPATH lets a dev build resolve libbuttre_ffi.so straight from the cargo
# target dir; packaged builds install the .so next to system libs instead.
get_filename_component(BUTTRE_FFI_DIR "${BUTTRE_FFI_LIB}" DIRECTORY)
set_target_properties(buttre-fcitx5 PROPERTIES
    OUTPUT_NAME buttre-fcitx5
    BUILD_RPATH "${BUTTRE_FFI_DIR}"
    INSTALL_RPATH "${BUTTRE_FFI_DIR}")

install(TARGETS buttre-fcitx5 DESTINATION "${CMAKE_INSTALL_LIBDIR}/fcitx5")
install(FILES buttre-addon.conf
        DESTINATION "${CMAKE_INSTALL_DATADIR}/fcitx5/addon" RENAME buttre.conf)
install(FILES buttre-im.conf
        DESTINATION "${CMAKE_INSTALL_DATADIR}/fcitx5/inputmethod" RENAME buttre.conf)
