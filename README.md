# MaaCoC

The CoC Powered by MaaFramework

## Build and Run

    mkdir build
    cmake .. && cmake --build .
    ./MaaCoC

## Notice

The device must be set to a resolution of 1080x1920.

    adb shell wm size 1080x1920

To restore the original setting:

    adb shell wm size reset
