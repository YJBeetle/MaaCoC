include(FetchContent)
FetchContent_Declare(
    MaaAgentBinary
    GIT_REPOSITORY https://github.com/MaaXYZ/MaaAgentBinary.git
    GIT_TAG main
)
FetchContent_MakeAvailable(MaaAgentBinary)
message(STATUS "MaaAgentBinary dir: ${maaagentbinary_SOURCE_DIR}")
