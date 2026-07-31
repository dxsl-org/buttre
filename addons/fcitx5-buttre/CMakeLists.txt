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

# Packaged layout (the fcitx5-buttre .deb): libbuttre_ffi.so is INSTALLED
# into ${libdir}/buttre and the addon resolves it via $ORIGIN — a package
# must be self-contained on a machine with no cargo target dir. OFF (dev
# build) keeps the cargo-target-dir RPATH so `cmake --install` straight
# from a checkout keeps working.
option(BUTTRE_PACKAGED "Self-contained package layout (\$ORIGIN rpath + install libbuttre_ffi.so)" OFF)

# Addon .so name must match Library=libbuttre-fcitx5 in buttre-addon.conf.
get_filename_component(BUTTRE_FFI_DIR "${BUTTRE_FFI_LIB}" DIRECTORY)
if(BUTTRE_PACKAGED)
    # Addon lands in ${libdir}/fcitx5/, the ffi lib in ${libdir}/buttre/ —
    # one directory up, one over.
    set(BUTTRE_INSTALL_RPATH "\$ORIGIN/../buttre")
else()
    set(BUTTRE_INSTALL_RPATH "${BUTTRE_FFI_DIR}")
endif()
set_target_properties(buttre-fcitx5 PROPERTIES
    OUTPUT_NAME buttre-fcitx5
    BUILD_RPATH "${BUTTRE_FFI_DIR}"
    INSTALL_RPATH "${BUTTRE_INSTALL_RPATH}")

install(TARGETS buttre-fcitx5 DESTINATION "${CMAKE_INSTALL_LIBDIR}/fcitx5")
if(BUTTRE_PACKAGED)
    install(FILES "${BUTTRE_FFI_LIB}" DESTINATION "${CMAKE_INSTALL_LIBDIR}/buttre")
endif()
install(FILES buttre-addon.conf
        DESTINATION "${CMAKE_INSTALL_DATADIR}/fcitx5/addon" RENAME buttre.conf)
install(FILES buttre-im.conf
        DESTINATION "${CMAKE_INSTALL_DATADIR}/fcitx5/inputmethod" RENAME buttre.conf)

# Per-method icons for the panel menu (the same PNGs the tray uses),
# published into the icon theme as buttre-<method>.
set(BUTTRE_ICON_SRC "${CMAKE_CURRENT_SOURCE_DIR}/../../crates/buttre-platform/icons")
foreach(method english telex vni nom)
    install(FILES "${BUTTRE_ICON_SRC}/${method}.png"
            DESTINATION "${CMAKE_INSTALL_DATADIR}/icons/hicolor/128x128/apps"
            RENAME "buttre-${method}.png")
endforeach()

# ── Packaging (CPack, gated on BUTTRE_PACKAGED) ─────────────────────────────
# One separate `fcitx5-buttre` .deb, the same shape every fcitx5 addon in
# the distros ships as (fcitx5-unikey, fcitx5-bamboo) — the MAIN buttre
# package must not depend on fcitx5. DEB only, deliberately: the addon
# links the builder's libFcitx5Core C++ ABI, so a binary built on Ubuntu
# does not load on Fedora's fcitx5 — RPM users compile from source instead
# (header comment above has the three commands).
if(BUTTRE_PACKAGED)
    set(CPACK_GENERATOR "DEB")
    set(CPACK_PACKAGE_NAME "fcitx5-buttre")
    # Version injected by the packaging script from `cargo metadata` so the
    # addon package can never drift from the workspace release version.
    if(NOT CPACK_PACKAGE_VERSION)
        set(CPACK_PACKAGE_VERSION "${PROJECT_VERSION}")
    endif()
    set(CPACK_PACKAGE_CONTACT "buttre project")
    set(CPACK_PACKAGE_DESCRIPTION_SUMMARY "Vietnamese input method engine (buttre) addon for fcitx5")
    # dpkg-shlibdeps derives the real libFcitx5Core/libc dependency set from
    # the compiled .so — hand-written Depends would go stale on the next
    # toolchain bump. `fcitx5` itself is the one dependency shlibdeps cannot
    # see (the daemon, not a linked lib).
    set(CPACK_DEBIAN_PACKAGE_SHLIBDEPS ON)
    set(CPACK_DEBIAN_PACKAGE_DEPENDS "fcitx5")
    set(CPACK_DEBIAN_FILE_NAME DEB-DEFAULT)
    include(CPack)
endif()
