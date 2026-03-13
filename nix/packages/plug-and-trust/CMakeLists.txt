cmake_minimum_required(VERSION 3.5.0)

project(plug_and_trust C)

set(PLUG_AND_TRUST_VERSION "${PLUG_AND_TRUST_VERSION}")

set(SIMW_LIB_DIR "${SIMW_LIB_DIR}")
include("${SIMW_LIB_DIR}/simw_lib.cmake")

if("${PTMW_SE05X_Auth}" STREQUAL "None")
  add_library(plugandtrust STATIC ${SIMW_SE_SOURCES})
else()
  add_library(plugandtrust STATIC ${SIMW_SE_SOURCES} ${SIMW_SE_AUTH_SOURCES})
endif()

target_include_directories(plugandtrust PUBLIC ${SIMW_INC_DIR})

if("${PTMW_HostCrypto}" STREQUAL "OPENSSL")
  target_link_libraries(plugandtrust PRIVATE ssl crypto)
endif()

install(TARGETS plugandtrust ARCHIVE DESTINATION lib)

configure_file(
  ${CMAKE_CURRENT_SOURCE_DIR}/plug-and-trust.pc.in
  ${CMAKE_CURRENT_BINARY_DIR}/plug-and-trust.pc
  @ONLY
)

install(FILES ${CMAKE_CURRENT_BINARY_DIR}/plug-and-trust.pc DESTINATION lib/pkgconfig)
