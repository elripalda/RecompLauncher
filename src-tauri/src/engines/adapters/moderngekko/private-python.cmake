# Module builds must use the checksum-pinned interpreter selected by Setup.
if(NOT DEFINED ENV{MODERNGEKKO_PYTHON} OR NOT EXISTS "$ENV{MODERNGEKKO_PYTHON}")
  message(FATAL_ERROR "RecompLauncher private Python is unavailable")
endif()
set(Python3_EXECUTABLE "$ENV{MODERNGEKKO_PYTHON}")
find_package(Python3 REQUIRED COMPONENTS Interpreter)
