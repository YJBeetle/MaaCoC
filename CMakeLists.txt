cmake_minimum_required(VERSION 3.28)

set(CMAKE_CXX_STANDARD 17)

project(MaaCoC VERSION 0.1.0 LANGUAGES CXX)

include(${PROJECT_SOURCE_DIR}/cmake/FetchMaaAgentBinary.cmake)
file(COPY ${maaagentbinary_SOURCE_DIR}/ DESTINATION ${CMAKE_BINARY_DIR}/MaaAgentBinary)

include(${PROJECT_SOURCE_DIR}/cmake/FetchMaaFramework.cmake)
find_package(MaaFramework CONFIG REQUIRED)

file(GLOB_RECURSE SRC_LIST
    ${PROJECT_SOURCE_DIR}/src/*.cpp
    ${PROJECT_SOURCE_DIR}/src/*.hpp
    ${PROJECT_SOURCE_DIR}/src/*.h
)

add_executable(${PROJECT_NAME} ${SRC_LIST})
target_link_libraries(${PROJECT_NAME}
    MaaFramework::MaaFramework
    MaaFramework::MaaToolkit
)
